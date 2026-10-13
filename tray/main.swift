// clockify-agent-tray: a menu bar panel that sends a prompt to headless Claude
// Code (`claude -p`, Sonnet 5.5) in the clockify-agent repo and streams the
// reply. While the panel shows, it also shows the CPU and memory of the run's
// process tree and of the tray itself. `make install` starts it at login
// through ~/Library/LaunchAgents/com.clockifyagent.tray.plist, which passes
// --repo and points stdout/stderr at ~/Library/Logs/clockify-agent-tray.log.
import AppKit

let logURL = FileManager.default.homeDirectoryForCurrentUser
    .appendingPathComponent("Library/Logs/clockify-agent-tray.log")
// MARK: - Claude invocation

let claudeModel = "claude-sonnet-5-5"
let scope = "You are a Clockify time-tracking assistant. Use the installed clockify binary on PATH, as the clockify skill describes. Never build, run or edit this repository's source code."

/// argv after `claude` for one fresh headless run in `repo`. `--` ends the
/// options, so a prompt starting with "-" stays a prompt; no shell is involved.
func claudeArgs(prompt: String, repo: String) -> [String] {
    ["-p",
     "--model", claudeModel,
     "--permission-mode", "bypassPermissions",
     "--setting-sources", "project,local",
     "--strict-mcp-config", "--mcp-config", repo + "/.mcp.json",
     "--tools", "Bash,Read,Skill",
     "--effort", "low",
     "--no-session-persistence",
     "--append-system-prompt", scope,
     "--output-format", "stream-json", "--verbose",
     "--", prompt]
}

/// What the reply shows from the stream: prose, a Bash command, the final result.
enum Event: Equatable {
    case text(String)
    case bash(String)
    case result(String, isError: Bool)
}

/// The events in one stream-json line; [] for lines the reply ignores (init,
/// tool results, rate limits) and for malformed ones.
func parseEvents(_ line: Substring) -> [Event] {
    guard let o = (try? JSONSerialization.jsonObject(with: Data(line.utf8))) as? [String: Any] else { return [] }
    switch o["type"] as? String {
    case "assistant":
        let content = (o["message"] as? [String: Any])?["content"] as? [[String: Any]] ?? []
        return content.compactMap { c in
            switch c["type"] as? String {
            case "text": return (c["text"] as? String).map(Event.text)
            case "tool_use" where c["name"] as? String == "Bash":
                return ((c["input"] as? [String: Any])?["command"] as? String).map(Event.bash)
            default: return nil
            }
        }
    case "result":
        // Error subtypes (error_max_turns, error_during_execution) carry no result text.
        let text = o["result"] as? String ?? o["subtype"] as? String ?? ""
        return [.result(text, isError: o["is_error"] as? Bool ?? false)]
    default:
        return []
    }
}

func size(_ e: Event) -> Int {
    switch e {
    case .text(let s), .bash(let s), .result(let s, _): s.utf8.count
    }
}

/// Keeps the newest events within `max` bytes of text; dropped says older text went.
/// The oldest kept event may lose its head, cut at a character boundary.
func capEvents(_ events: [Event], max: Int) -> (kept: [Event], dropped: Bool) {
    var total = events.reduce(0) { $0 + size($1) }, kept = events[...]
    guard total > max else { return (events, false) }
    while let first = kept.first, total - size(first) >= max {
        total -= size(first)
        kept.removeFirst()
    }
    if total > max, let first = kept.first {
        kept[kept.startIndex] = tail(first, max - (total - size(first)))
    }
    return (Array(kept), true)
}

/// The event with only its last `n` bytes of text, never splitting a character.
func tail(_ e: Event, _ n: Int) -> Event {
    func cut(_ s: String) -> String { String(decoding: s.utf8.suffix(n).drop { $0 & 0xC0 == 0x80 }, as: UTF8.self) }
    switch e {
    case .text(let s): return .text(cut(s))
    case .bash(let s): return .bash(cut(s))
    case .result(let s, let isError): return .result(cut(s), isError: isError)
    }
}

// MARK: - Process usage

/// CPU time in ns and memory footprint in bytes of a process, as Activity Monitor counts them; nil once it is gone.
func usage(of pid: pid_t) -> (cpu: UInt64, mem: UInt64)? {
    var ri = rusage_info_v2(), tb = mach_timebase_info()
    let rc = withUnsafeMutablePointer(to: &ri) {
        $0.withMemoryRebound(to: rusage_info_t?.self, capacity: 1) { proc_pid_rusage(pid, RUSAGE_INFO_V2, $0) }
    }
    guard rc == 0 else { return nil }
    mach_timebase_info(&tb)     // CPU times come in Mach ticks, not ns, on Apple silicon
    return ((ri.ri_user_time + ri.ri_system_time) * UInt64(tb.numer) / UInt64(tb.denom), ri.ri_phys_footprint)
}

/// Percent of one core between two (CPU ns, uptime s) samples; nil without an earlier one.
func cpuPercent(_ a: (cpu: UInt64, at: TimeInterval)?, _ b: (cpu: UInt64, at: TimeInterval)) -> Double? {
    guard let a, b.at > a.at else { return nil }
    return max(0, Double(b.cpu) - Double(a.cpu)) / 1e9 / (b.at - a.at) * 100
}

/// Direct children of `pid`; [] once it is gone. 64 is plenty for one claude run.
func children(of pid: pid_t) -> [pid_t] {
    var buf = [pid_t](repeating: 0, count: 64)
    let n = buf.withUnsafeMutableBytes { proc_listchildpids(pid, $0.baseAddress, Int32($0.count)) }
    return n > 0 ? Array(buf.prefix(Int(n))) : []
}

/// `root`, then its descendants breadth-first; a pid seen twice is skipped.
func descendants(of root: pid_t, children: (pid_t) -> [pid_t]) -> [pid_t] {
    var out: [pid_t] = [], queue = [root], seen = Set<pid_t>()
    while !queue.isEmpty {
        let p = queue.removeFirst()
        guard seen.insert(p).inserted else { continue }
        out.append(p)
        queue += children(p)
    }
    return out
}

/// The process's short name (proc_name), "?" once it is gone.
func procName(_ pid: pid_t) -> String {
    var buf = [CChar](repeating: 0, count: 64)
    return proc_name(pid, &buf, UInt32(buf.count)) > 0 ? String(cString: buf) : "?"
}

/// One sampled process; cpu is nil on its first sample.
struct ProcStat: Equatable {
    let pid: pid_t
    let name: String
    let cpu: Double?
    let mem: UInt64
}

/// "212 MB", or "5.5 MB" under 10 MB.
func mem(_ bytes: UInt64) -> String {
    let mb = Double(bytes) / 1_048_576
    return String(format: mb < 10 ? "%.1f MB" : "%.0f MB", mb)
}

func pctText(_ cpu: Double?) -> String { cpu.map { String(format: "%.1f%%", $0) } ?? "—" }

func claudeLine(_ procs: [ProcStat]?) -> String {
    guard let procs, !procs.isEmpty else { return "Claude: idle" }
    let cpus = procs.compactMap(\.cpu)
    return "Claude: \(procs.count) process\(procs.count == 1 ? "" : "es") · CPU \(pctText(cpus.isEmpty ? nil : cpus.reduce(0, +))) · "
        + mem(procs.reduce(0) { $0 + $1.mem })
}

func trayLine(cpu: Double?, mem m: UInt64) -> String { "Tray: CPU \(pctText(cpu)) · " + mem(m) }

/// The cells of one expanded process row: pid, CPU, memory, name.
func procCells(_ p: ProcStat) -> [String] { ["\(p.pid)", pctText(p.cpu), mem(p.mem), p.name] }

#if SELFTEST
// claudeArgs
let a = claudeArgs(prompt: "--help me log 2h", repo: "/r")
precondition(a.first == "-p" && a.suffix(2) == ["--", "--help me log 2h"], "prompt last, after --: \(a)")
precondition(a.contains("/r/.mcp.json") && a.contains("claude-sonnet-5-5") && a.contains("bypassPermissions"), "flags: \(a)")
precondition(a[a.firstIndex(of: "--tools")! + 1] == "Bash,Read,Skill", "tools")
let odd = "log \"2h\" to 'card'\n-- $(rm -rf ~) 🎯"
precondition(claudeArgs(prompt: odd, repo: "/r").last == odd, "odd prompt stays one verbatim element")

// parseEvents, on lines trimmed from a real run (2026-10-13).
let initLine = #"{"type":"system","subtype":"init","cwd":"/r","tools":["Bash","Read","Skill"],"model":"claude-sonnet-5-5"}"#
let bashLine = #"{"type":"assistant","message":{"model":"claude-sonnet-5-5","role":"assistant","content":[{"type":"tool_use","id":"toolu_01","name":"Bash","input":{"command":"clockify summary today","description":"Run clockify summary for today"}}]}}"#
let textLine = #"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Total today: 0:00."},{"type":"tool_use","id":"t2","name":"mcp__trello__get_card","input":{"card":"AbC123"}}]}}"#
let toolResult = #"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_01","content":"Total 0:00"}]}}"#
let okLine = #"{"type":"result","subtype":"success","is_error":false,"duration_ms":7885,"result":"Total today: 0:00."}"#
let errLine = #"{"type":"result","subtype":"error_max_turns","is_error":true,"duration_ms":100}"#
precondition(parseEvents(Substring(initLine)).isEmpty, "init ignored")
precondition(parseEvents(Substring(bashLine)) == [.bash("clockify summary today")], "bash")
precondition(parseEvents(Substring(textLine)) == [.text("Total today: 0:00.")], "text; MCP tool_use ignored")
precondition(parseEvents(Substring(toolResult)).isEmpty, "tool results ignored")
precondition(parseEvents(Substring(okLine)) == [.result("Total today: 0:00.", isError: false)], "result ok")
precondition(parseEvents(Substring(errLine)) == [.result("error_max_turns", isError: true)], "error result without text")
precondition(parseEvents(#"{"type":"rate_limit_event"}"#).isEmpty && parseEvents("not json").isEmpty && parseEvents("").isEmpty, "ignored lines")

// capEvents
let big = [Event.text(String(repeating: "a", count: 60)), .bash("clockify log x 2h"), .text(String(repeating: "b", count: 30))]
precondition(capEvents(big, max: 1000) == (big, false), "under the cap: untouched")
let c1 = capEvents(big, max: 50)
precondition(c1.kept == [.text("aaa"), big[1], big[2]] && c1.dropped, "oldest text cut to fit: \(c1)")
let c2 = capEvents([.text(String(repeating: "x", count: 10) + "TAIL")], max: 4)
precondition(c2.kept == [.text("TAIL")] && c2.dropped, "one big event keeps its tail: \(c2)")
// A long reply repeats in the final result event; the cap must hold for it too.
let rep = [Event.text(String(repeating: "a", count: 70_000)), .result(String(repeating: "a", count: 70_000), isError: false)]
let c3 = capEvents(rep, max: 65_536)
precondition(c3.kept == [.result(String(repeating: "a", count: 65_536), isError: false)] && c3.dropped, "trailing result capped: \(c3)")
let c4 = capEvents([.text(String(repeating: "x", count: 100)), .text("Done")], max: 50)
precondition(c4.kept == [.text(String(repeating: "x", count: 46)), .text("Done")] && c4.dropped, "big then small: \(c4)")
let c5 = capEvents([.text("xé")], max: 2)
precondition(c5.kept == [.text("é")] && c5.dropped, "multibyte kept whole: \(c5)")
let c6 = capEvents([.text("xé")], max: 1)
precondition(c6.kept == [.text("")] && c6.dropped, "no split character: \(c6)")
for (c, max) in [(c1, 50), (c3, 65_536), (c4, 50), (c5, 2), (c6, 1)] {
    precondition(c.kept.reduce(0) { $0 + size($1) } <= max, "kept within max: \(max)")
}
// descendants: claude(10) → bash(11), trello-mcp(12); bash → clockify(13); 99 unrelated; 13 → 10 is a cycle.
let tree: [pid_t: [pid_t]] = [10: [11, 12], 11: [13], 13: [10], 99: [100]]
precondition(descendants(of: 10) { tree[$0] ?? [] } == [10, 11, 12, 13], "tree")
precondition(descendants(of: 42) { _ in [] } == [42], "lone root")

// usage, cpuPercent, formatting
precondition(usage(of: -1) == nil && usage(of: getpid()) != nil, "usage")
precondition(cpuPercent(nil, (cpu: 5, at: 1)) == nil, "first sample")
precondition(cpuPercent((cpu: 0, at: 0), (cpu: 500_000_000, at: 2)) == 25, "0.5 s CPU in 2 s = 25%")
precondition(mem(5_767_168) == "5.5 MB" && mem(222_298_112) == "212 MB", "mem: \(mem(5_767_168)) \(mem(222_298_112))")
let ps = [ProcStat(pid: 51204, name: "claude", cpu: 6.9, mem: 184_549_376),
          ProcStat(pid: 51231, name: "trello-mcp", cpu: 0.4, mem: 30_408_704),
          ProcStat(pid: 51290, name: "clockify", cpu: 1.1, mem: 7_340_032)]
precondition(claudeLine(nil) == "Claude: idle" && claudeLine([]) == "Claude: idle", "idle")
precondition(claudeLine(ps) == "Claude: 3 processes · CPU 8.4% · 212 MB", "line: \(claudeLine(ps))")
precondition(claudeLine([ProcStat(pid: 1, name: "claude", cpu: nil, mem: 1_048_576)]) == "Claude: 1 process · CPU — · 1.0 MB", "first sample")
precondition(trayLine(cpu: 0, mem: 14_680_064) == "Tray: CPU 0.0% · 14 MB", "tray: \(trayLine(cpu: 0, mem: 14_680_064))")
precondition(procCells(ps[2]) == ["51290", "1.1%", "7.0 MB", "clockify"], "cells: \(procCells(ps[2]))")
precondition(children(of: getpid()).isEmpty && children(of: -1).isEmpty, "children: none yet")
precondition(ProcessInfo.processInfo.processName.hasPrefix(procName(getpid())) && procName(-1) == "?", "procName: \(procName(getpid()))")
print("tray selftest ok")
exit(0)
#endif

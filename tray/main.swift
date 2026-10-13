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

// MARK: - Running claude

/// How a process ended: exited with a status, or was killed by a signal.
enum Exit: Equatable {
    case code(Int32)
    case signal(Int32)

    /// Decodes a waitpid status (Swift has no WIFEXITED/WEXITSTATUS macros).
    init(waitStatus s: Int32) {
        self = s & 0x7f == 0 ? .code((s >> 8) & 0xff) : .signal(s & 0x7f)
    }
}

/// One headless run in its own process group, so Stop reaches claude and every
/// child it started. stdout is split into lines and parsed; stderr goes to the
/// log and its last 20 lines are kept. Callbacks run on the main queue.
final class Run {
    let pid: pid_t
    let started = Date()
    private(set) var stderrTail: [String] = []
    private(set) var stopped = false
    private(set) var exited = false
    var onEvents: ([Event]) -> Void = { _ in }
    var onExit: (Exit) -> Void = { _ in }
    private let out = Pipe(), err = Pipe()
    private var pending = Data(), errPending = Data()
    private var exitStatus: Exit?, outDone = false
    private var watch: DispatchSourceProcess?

    /// Throws the errno posix_spawnp returned (ENOENT: no such program on PATH).
    init(exe: String, args: [String], cwd: String) throws {
        var fa: posix_spawn_file_actions_t?
        var attr: posix_spawnattr_t?
        posix_spawn_file_actions_init(&fa)
        posix_spawnattr_init(&attr)
        defer {
            posix_spawn_file_actions_destroy(&fa)
            posix_spawnattr_destroy(&attr)
        }
        posix_spawn_file_actions_addopen(&fa, 0, "/dev/null", O_RDONLY, 0)
        posix_spawn_file_actions_adddup2(&fa, out.fileHandleForWriting.fileDescriptor, 1)
        posix_spawn_file_actions_adddup2(&fa, err.fileHandleForWriting.fileDescriptor, 2)
        posix_spawn_file_actions_addchdir(&fa, cwd)
        // Own group (pgid = pid) for Stop; CLOEXEC_DEFAULT keeps the tray's other fds out of the child.
        posix_spawnattr_setflags(&attr, Int16(POSIX_SPAWN_SETPGROUP | POSIX_SPAWN_CLOEXEC_DEFAULT))
        posix_spawnattr_setpgroup(&attr, 0)
        var pid: pid_t = 0
        let argv: [UnsafeMutablePointer<CChar>?] = ([exe] + args).map { strdup($0) } + [nil]
        defer { argv.forEach { free($0) } }
        let rc = posix_spawnp(&pid, exe, &fa, &attr, argv, environ)
        // The child has its copies; without closing ours, the reads never see EOF.
        try? out.fileHandleForWriting.close()
        try? err.fileHandleForWriting.close()
        guard rc == 0 else { throw POSIXError(POSIXErrorCode(rawValue: rc) ?? .EIO) }
        self.pid = pid
        out.fileHandleForReading.readabilityHandler = { [weak self] h in
            let d = h.availableData
            DispatchQueue.main.async { self?.stdout(d) }
        }
        err.fileHandleForReading.readabilityHandler = { [weak self] h in
            let d = h.availableData
            if !d.isEmpty { FileHandle.standardError.write(d) }
            DispatchQueue.main.async { self?.stderr(d) }
        }
        let src = DispatchSource.makeProcessSource(identifier: pid, eventMask: .exit, queue: .main)
        src.setEventHandler { [weak self] in
            var status: Int32 = 0
            waitpid(pid, &status, 0)
            self?.exited(Exit(waitStatus: status))
        }
        src.resume()
        watch = src
    }

    /// SIGTERM to the whole group, SIGKILL 3 s later for anything still in it.
    func stop() {
        guard !exited else { return }
        stopped = true
        kill(-pid, SIGTERM)
        let pg = pid
        DispatchQueue.main.asyncAfter(deadline: .now() + 3) { kill(-pg, SIGKILL) }
    }

    private func stdout(_ d: Data) {
        if d.isEmpty {
            out.fileHandleForReading.readabilityHandler = nil
            if !pending.isEmpty { emit(pending[...]) }
            pending = Data()
            outDone = true
            finish()
            return
        }
        pending.append(d)
        while let nl = pending.firstIndex(of: 0x0a) {
            emit(pending[pending.startIndex..<nl])
            pending = pending[(nl + 1)...]
        }
        pending = Data(pending)     // re-base, so indices start at 0 again
    }

    private func emit(_ line: Data) {
        let ev = parseEvents(Substring(decoding: line, as: UTF8.self))
        if !ev.isEmpty { onEvents(ev) }
    }

    private func stderr(_ d: Data) {
        if d.isEmpty { err.fileHandleForReading.readabilityHandler = nil; return }
        errPending.append(d)
        let lines = String(decoding: errPending, as: UTF8.self).split(separator: "\n", omittingEmptySubsequences: false)
        errPending = Data(lines.last.map { String($0) }?.utf8 ?? "".utf8)
        stderrTail = Array((stderrTail + lines.dropLast().map(String.init).filter { !$0.isEmpty }).suffix(20))
    }

    private func exited(_ e: Exit) {
        watch?.cancel()
        watch = nil
        exitStatus = e
        exited = true
        kill(-pid, SIGTERM)     // children claude left behind (an MCP server, a background job)
        // stdout normally hits EOF right away; don't wait on a straggler holding it open.
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { [weak self] in
            guard let self, !outDone else { return }
            outDone = true
            out.fileHandleForReading.readabilityHandler = nil
            finish()
        }
        finish()
    }

    private func finish() {
        guard outDone, let e = exitStatus else { return }
        if !errPending.isEmpty { stderrTail = Array((stderrTail + [String(decoding: errPending, as: UTF8.self)]).suffix(20)); errPending = Data() }
        exitStatus = nil      // once
        onExit(e)
    }
}

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
/// Spins the main run loop until cond holds or timeout passes.
func waitUntil(_ timeout: TimeInterval, _ cond: () -> Bool) -> Bool {
    let end = Date().addingTimeInterval(timeout)
    while !cond() && Date() < end { RunLoop.main.run(until: Date().addingTimeInterval(0.02)) }
    return cond()
}

// Run: stream-json on stdout in two chunks across a line break, stderr kept, exit code.
var got: [Event] = [], ended: Exit?
let script = "printf '%s\\n' '\(textLine)'; printf '%s' '\(okLine.prefix(30))'; sleep 0.1; printf '%s\\n' '\(okLine.dropFirst(30))'; echo oops >&2; exit 3"
let r1 = try! Run(exe: "/bin/sh", args: ["-c", script], cwd: "/tmp")
r1.onEvents = { got += $0 }
r1.onExit = { ended = $0 }
precondition(waitUntil(5) { ended != nil }, "run ends")
precondition(got == [.text("Total today: 0:00."), .result("Total today: 0:00.", isError: false)], "events: \(got)")
precondition(ended == .code(3) && r1.stderrTail == ["oops"], "exit \(String(describing: ended)) stderr \(r1.stderrTail)")

// Run: cwd is applied.
var pwdOut: [Event] = [], pwdEnd: Exit?
let r2 = try! Run(exe: "/bin/sh", args: ["-c", #"printf '{"type":"result","is_error":false,"result":"%s"}\n' "$(pwd -P)""#], cwd: "/private/tmp")
r2.onEvents = { pwdOut += $0 }
r2.onExit = { pwdEnd = $0 }
precondition(waitUntil(5) { pwdEnd != nil } && pwdOut == [.result("/private/tmp", isError: false)], "cwd: \(pwdOut)")

// Run: Stop ends the whole group, children included.
var stopEnd: Exit?
let r3 = try! Run(exe: "/bin/sh", args: ["-c", "sleep 30 & sleep 30"], cwd: "/tmp")
r3.onExit = { stopEnd = $0 }
precondition(waitUntil(2) { children(of: r3.pid).count == 2 }, "two sleeps started: \(children(of: r3.pid))")
r3.stop()
precondition(waitUntil(5) { stopEnd != nil } && r3.stopped, "stop ends the run")
precondition(waitUntil(5) { kill(-r3.pid, 0) == -1 && errno == ESRCH }, "no process left in the group")

// Run: a background child holding stdout open doesn't keep the run going.
var holdEnd: Exit?, holdEvents: [Event] = []
let r4 = try! Run(exe: "/bin/sh", args: ["-c", "sleep 30 & printf '%s\\n' '\(okLine)'; exit 0"], cwd: "/tmp")
r4.onEvents = { holdEvents += $0 }
r4.onExit = { holdEnd = $0 }
precondition(waitUntil(2.5) { holdEnd != nil } && holdEnd == .code(0) && holdEvents.count == 1, "straggler: \(String(describing: holdEnd)) \(holdEvents)")
precondition(waitUntil(2) { kill(-r4.pid, 0) == -1 }, "straggler killed with the group")

// Run: an odd prompt reaches the program as one verbatim argv element (Review Focus 4).
let echoFile = NSTemporaryDirectory() + "ck-argv-\(getpid())"
var echoEnd: Exit?
let r5 = try! Run(exe: "/bin/sh", args: ["-c", #"printf %s "$1" > "$2""#, "sh", odd, echoFile], cwd: "/tmp")
r5.onExit = { echoEnd = $0 }
precondition(waitUntil(5) { echoEnd != nil } && (try? String(contentsOfFile: echoFile, encoding: .utf8)) == odd, "argv verbatim")
try? FileManager.default.removeItem(atPath: echoFile)

// Run: a missing program throws ENOENT.
do {
    _ = try Run(exe: "no-such-claude-binary", args: [], cwd: "/tmp")
    preconditionFailure("missing program must throw")
} catch let e as POSIXError {
    precondition(e.code == .ENOENT, "ENOENT: \(e)")
}

print("tray selftest ok")
exit(0)
#endif

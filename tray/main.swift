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

/// Keeps the newest events within `max` bytes of text; dropped says older ones went.
/// A single event over the cap keeps its tail.
func capEvents(_ events: [Event], max: Int) -> (kept: [Event], dropped: Bool) {
    var total = events.reduce(0) { $0 + size($1) }, kept = events[...]
    while total > max, kept.count > 1 {
        total -= size(kept.removeFirst())
    }
    guard total > max, let only = kept.first, case .text(let s) = only else { return (Array(kept), kept.count < events.count) }
    return ([.text(String(decoding: Array(s.utf8.suffix(max)), as: UTF8.self))], true)
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
precondition(c1.kept == Array(big.dropFirst()) && c1.dropped, "oldest dropped: \(c1)")
let c2 = capEvents([.text(String(repeating: "x", count: 10) + "TAIL")], max: 4)
precondition(c2.kept == [.text("TAIL")] && c2.dropped, "one big event keeps its tail: \(c2)")
print("tray selftest ok")
exit(0)
#endif

# Clockify Agent Tray Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A macOS menu bar app whose panel sends a typed prompt to headless Claude Code (`claude -p`, `claude-sonnet-5-5`, permissions bypassed) in this repo, streams the reply, and shows the CPU and memory of the run's process tree and of the tray itself.

**Architecture:** Two Swift files compiled with one `swiftc` call, in plain AppKit, as in the sibling trays. `tray/main.swift` holds the pure logic (argv, stream-json parsing, process tree, formatting), the process runner `Run` (posix_spawn in its own process group), the menu bar icon and the `Tray` app delegate. `tray/Panel.swift` holds the panel's state model, claude-usage's Swiss palette and layout helpers, and `PanelController`. Pure parts and the runner are checked by a `-D SELFTEST` build (`make tray-test`); there is no XCTest target.

**Tech Stack:** Swift 6 / AppKit (no SwiftUI, no NSPopover, no packages), libproc (`proc_pid_rusage`, `proc_listchildpids`, `proc_name`), posix_spawn, GNU make, launchd LaunchAgent, Claude Code CLI 2.1.x.

**Spec:** `docs/superpowers/specs/2026-10-13-tray-composer-design.md`

**Prototype:** every Swift block below was compiled and run on 2026-10-13 (macOS 27, Darwin 27.0). The full `make tray-test` passes in about 0.5 s; a release build idles at 15 MB footprint and 0.0% CPU with the panel closed; offscreen renders of the panel matched the approved mockup in light and dark.

**Where to work:** a branch `feat/tray-composer` (a worktree is fine for Tasks 2–7). Two steps must run in the main checkout `/Users/loithai/Projects/ai-tools/clockify-agent`. Task 1 Step 3 fixes the user's uncommitted `SKILL.md` edit, and that fix stays uncommitted. Task 8's final `make install` must run there too, because the LaunchAgent's `--repo` is the directory `make install` runs in. The user's uncommitted `main.go` change is not part of this work: never stage or commit it.

## Global Constraints

- macOS 26.0 or later (`posix_spawn_file_actions_addchdir`); `LSMinimumSystemVersion` 26.0.
- Swift + AppKit only. No SwiftUI, no `NSPopover`, no third-party packages, no XCTest. Build with `swiftc` from the Makefile.
- Model `claude-sonnet-5-5`; `--permission-mode bypassPermissions`. The full argv is exactly `claudeArgs` in Task 2, with the prompt last, after `--`.
- App `Clockify Agent.app`, bundle id `com.clockifyagent.tray`, executable `clockify-agent-tray`, LaunchAgent label `com.clockifyagent.tray`, log `~/Library/Logs/clockify-agent-tray.log`.
- Colors are claude-usage's DESIGN.md. Light: paper `#fff`, ink `#111`, muted `#5c5c5c`, red-ink `#da291c`. Dark: paper `#0d0d0d`, ink `#f2f2f2`, muted `#a3a3a3`, red-ink `#ff6a5c`. Fill red `#da291c` in both. Helvetica Neue with tabular digits, square corners, no shadows inside the panel.
- No timers while the panel is closed. Stats sample every 2 s only while it shows. The reply keeps at most 64 KB of text, and stderr keeps its last 20 lines.
- Process usage comes from libproc, never from spawning `ps`.
- Go code is untouched; `go test ./...` stays green.
- Commit messages are a short subject plus at most a few lines; no AI attribution lines.

## Review Focus

1. **Typing in the non-activating panel.** The app never activates, yet keystrokes must reach the prompt, ⌘↩ must send, and Esc must close. Covered by the Task 8 end-to-end checks; no unit test can reach it.
2. **Quit or a reinstall in the middle of a run.** Quit, SIGTERM, and `launchctl bootout` (which `make install` does) must leave no `claude` or `trello-mcp` process behind. Covered by the Task 4 Stop test (the whole group dies) and a Task 8 end-to-end check.
3. **A child that keeps stdout open after claude exits** (a background job started by a Bash call). The run must still end within about 1 s. Covered by the Task 4 straggler test.
4. **Odd prompt text**: quotes, newlines, `$(…)`, emoji, or a leading `--`. Each must reach claude verbatim as one argv element. Covered by the Task 2 claudeArgs test.
5. **Very long output.** The reply must stay capped at 64 KB, and the panel must never grow past the screen. Covered by the Task 2 capEvents tests and a Task 8 end-to-end check.

## Plan refinements over the spec

These were found while prototyping; the spec has been updated to match.

- `parseEvent` returns `[Event]` (`parseEvents`), because one assistant message can carry several blocks. `trimReply` became `capEvents`, which caps events instead of a string.
- The prompt goes last, after `--`. Tested: a prompt starting with `--help` reached Claude as text.
- Bash lines are muted 12 pt and indented 12 pt; there is no hairline, since attributed text can't draw one.
- The footer follows claude-usage's order: an Open Log / Quit row first, then the stat lines.
- Process rows use `proc_name` short names (`claude`, `trello-mcp`, `clockify`).
- The prompt clears after a run ends `DONE`, and stays after a failure so it can be retried. The result text shows only when it is an error or differs from the last prose (it usually repeats it).
- When claude exits, the tray SIGTERMs its process group, which cleans up a leftover MCP server or background job.

---

### Task 1: Load the clockify skill in Claude Code

The tray's runs only use the `clockify` binary correctly when the skill loads. Today `.claude/skills/` doesn't exist, and the user's uncommitted `SKILL.md` edit dropped the frontmatter's closing `---`.

**Files:**
- Create: `.claude/skills/joe-clock` (symlink to `../../.agents/skills/joe-clock`)
- Create: `.gitignore`
- Modify (main checkout only, stays uncommitted): `.agents/skills/joe-clock/SKILL.md`, the line after `description:`

**Interfaces:**
- Consumes: nothing
- Produces: the skill `joe-clock` in `claude -p` runs started in this repo

- [ ] **Step 1: Check the failure**

Run from `/Users/loithai/Projects/ai-tools/clockify-agent`:

```bash
claude -p --model claude-sonnet-5-5 --setting-sources project,local --strict-mcp-config --tools Skill --effort low --no-session-persistence --output-format stream-json --verbose -- "reply ok" 2>/dev/null | head -1 | python3 -c "import json,sys; print([s for s in json.load(sys.stdin)['skills'] if 'clock' in s])"
```

Expected: `[]`

- [ ] **Step 2: Link the skill and add `.gitignore`**

```bash
mkdir -p .claude/skills
ln -s ../../.agents/skills/joe-clock .claude/skills/joe-clock
test -f .claude/skills/joe-clock/SKILL.md && echo linked
```

Expected: `linked`

Create `.gitignore`:

```
# Visual companion sessions from brainstorming
.superpowers/
# make build / make app output
/clockify
build/
*.plist.tmp
```

- [ ] **Step 3: Restore the frontmatter's closing `---` (main checkout, do not commit)**

In `.agents/skills/joe-clock/SKILL.md`, insert a line `---` between the `description: "…"` line and the blank line before `# joe-clock`. The file must start:

```
---
name: clockify
description: "Clockify time-tracking helper for logging, summarizing, …"
---

# joe-clock
```

Change nothing else in the file; the rest is the user's pending edit. If this task runs in a worktree, skip this step there: the committed `SKILL.md` already has the `---`. Then do this step in the main checkout.

- [ ] **Step 4: Check the skill loads**

Re-run the Step 1 command.

Expected: `['joe-clock']`

- [ ] **Step 5: Commit (symlink and .gitignore only)**

```bash
git add .claude/skills/joe-clock .gitignore
git commit -m "chore: link the clockify skill for Claude Code, add .gitignore"
```

`git status` must still show `main.go` and `.agents/skills/joe-clock/SKILL.md` as modified and uncommitted.

---

### Task 2: Claude argv and stream-json parsing

**Files:**
- Create: `tray/main.swift`
- Create: `Makefile`

**Interfaces:**
- Consumes: nothing
- Produces:
  - `let logURL: URL`, `let claudeModel = "claude-sonnet-5-5"`, `let scope: String`
  - `func claudeArgs(prompt: String, repo: String) -> [String]` (argv after `claude`)
  - `enum Event: Equatable { case text(String); case bash(String); case result(String, isError: Bool) }`
  - `func parseEvents(_ line: Substring) -> [Event]`
  - `func capEvents(_ events: [Event], max: Int) -> (kept: [Event], dropped: Bool)`

- [ ] **Step 1: Create the Makefile**

```make
CLI         := clockify
TRAY        := clockify-agent-tray
SWIFT_SRC   := $(wildcard tray/*.swift)
SELFTEST    := $(or $(TMPDIR),/tmp)/$(TRAY)-selftest

.PHONY: build test tray-test check

build: ## compile the CLI
	go build -o $(CLI) .

test: ## Go unit tests
	go test ./...

# No XCTest target; the tray's pure parts and its process runner are checked behind -D SELFTEST, as in gws-mcp and claude-usage.
tray-test: ## check the tray's parsing, formatting, process tree and runner
	swiftc -D SELFTEST -suppress-warnings $(SWIFT_SRC) -o $(SELFTEST)
	$(SELFTEST)

check: test tray-test
```

- [ ] **Step 2: Write the failing tests**

Create `tray/main.swift`. The tests live in the `#if SELFTEST` block at the end of the file; later tasks insert their tests just before `print("tray selftest ok")`.

```swift
// clockify-agent-tray: a menu bar panel that sends a prompt to headless Claude
// Code (`claude -p`, Sonnet 5.5) in the clockify-agent repo and streams the
// reply. While the panel shows, it also shows the CPU and memory of the run's
// process tree and of the tray itself. `make install` starts it at login
// through ~/Library/LaunchAgents/com.clockifyagent.tray.plist, which passes
// --repo and points stdout/stderr at ~/Library/Logs/clockify-agent-tray.log.
import AppKit

let logURL = FileManager.default.homeDirectoryForCurrentUser
    .appendingPathComponent("Library/Logs/clockify-agent-tray.log")
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
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `make tray-test`
Expected: compile error `cannot find 'claudeArgs' in scope` (and the same for `parseEvents` and `capEvents`)

- [ ] **Step 4: Implement**

Insert this section into `tray/main.swift` directly above `#if SELFTEST`:

```swift
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
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `make tray-test`
Expected: last line `tray selftest ok`

- [ ] **Step 6: Commit**

```bash
git add Makefile tray/main.swift
git commit -m "feat(tray): claude argv and stream-json parsing"
```

---

### Task 3: Process usage and stat lines

**Files:**
- Modify: `tray/main.swift`. Add the section above `#if SELFTEST`, and the tests before `print("tray selftest ok")`.

**Interfaces:**
- Consumes: nothing from earlier tasks
- Produces:
  - `func usage(of pid: pid_t) -> (cpu: UInt64, mem: UInt64)?` (CPU ns, footprint bytes)
  - `func cpuPercent(_ a: (cpu: UInt64, at: TimeInterval)?, _ b: (cpu: UInt64, at: TimeInterval)) -> Double?`
  - `func children(of pid: pid_t) -> [pid_t]`
  - `func descendants(of root: pid_t, children: (pid_t) -> [pid_t]) -> [pid_t]`
  - `func procName(_ pid: pid_t) -> String`
  - `struct ProcStat: Equatable { let pid: pid_t; let name: String; let cpu: Double?; let mem: UInt64 }`
  - `func mem(_ bytes: UInt64) -> String`, `func pctText(_ cpu: Double?) -> String`
  - `func claudeLine(_ procs: [ProcStat]?) -> String`, `func trayLine(cpu: Double?, mem m: UInt64) -> String`, `func procCells(_ p: ProcStat) -> [String]`

- [ ] **Step 1: Write the failing tests**

Insert before `print("tray selftest ok")`:

```swift
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `make tray-test`
Expected: compile error `cannot find 'descendants' in scope` (and similar for the other names)

- [ ] **Step 3: Implement**

Insert above `#if SELFTEST`, after the Claude invocation section:

```swift
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `make tray-test`
Expected: last line `tray selftest ok`

- [ ] **Step 5: Commit**

```bash
git add tray/main.swift
git commit -m "feat(tray): process tree usage from libproc and stat lines"
```

---

### Task 4: Process runner

`Run` spawns one program in its own process group, with stdin set to `/dev/null`. It splits stdout into lines for `parseEvents`, sends stderr to the log, and keeps stderr's last 20 lines. Stop signals the whole group. When claude exits, the runner SIGTERMs the group to clean up stragglers. It doesn't wait more than 1 s for stdout's EOF.

**Files:**
- Modify: `tray/main.swift`

**Interfaces:**
- Consumes: `Event`, `parseEvents(_:)` (Task 2)
- Produces:
  - `enum Exit: Equatable { case code(Int32); case signal(Int32); init(waitStatus:) }`
  - `final class Run` with:
    - `let pid: pid_t`, `let started: Date`
    - `private(set) var stderrTail: [String]`, `private(set) var stopped: Bool`, `private(set) var exited: Bool`
    - `var onEvents: ([Event]) -> Void`, `var onExit: (Exit) -> Void` (main queue; `onExit` fires once, after the last `onEvents`)
    - `init(exe: String, args: [String], cwd: String) throws`, which throws `POSIXError` (`.ENOENT` when the program isn't on PATH)
    - `func stop()`

- [ ] **Step 1: Write the failing tests**

Insert before `print("tray selftest ok")`:

```swift
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

// Run: a missing program throws ENOENT.
do {
    _ = try Run(exe: "no-such-claude-binary", args: [], cwd: "/tmp")
    preconditionFailure("missing program must throw")
} catch let e as POSIXError {
    precondition(e.code == .ENOENT, "ENOENT: \(e)")
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `make tray-test`
Expected: compile error `cannot find 'Run' in scope`

- [ ] **Step 3: Implement**

Insert above `#if SELFTEST`, after the Process usage section:

```swift
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `make tray-test`
Expected: the line `oops` (the first test's stderr, passed through to the log as designed), then `tray selftest ok`. It takes about 0.5 s; more than 5 s means a wait timed out.

- [ ] **Step 5: Commit**

```bash
git add tray/main.swift
git commit -m "feat(tray): run claude in its own process group, stream its events"
```

---

### Task 5: Panel state logic

**Files:**
- Create: `tray/Panel.swift` (this task writes its top part; Task 6 appends the views)
- Modify: `tray/main.swift` (tests only)

**Interfaces:**
- Consumes: `Event` (Task 2), `Exit` (Task 4), `ProcStat` (Task 3)
- Produces:
  - `enum Phase: Equatable { case idle; case running(since: Date); case done(TimeInterval); case failed(String); case stopped }`
  - `enum Tone: Equatable { case ink, red, muted }`
  - `func metaText(_ p: Phase, now: Date) -> String`, `func statusWord(_ p: Phase) -> (String, Tone)?`
  - `func phaseAfter(exit: Exit, stopped: Bool, events: [Event], duration: TimeInterval) -> Phase`
  - `func promptTitle(_ s: String) -> String`
  - `final class PanelModel` with the fields and callbacks below. Tray sets `phase`, `asked`, `events`, `trimmed`, `stderrTail`, `repoError`, `claude`, `tray` and `cap`, sets the `on*` callbacks, and calls `statsChanged()` after sampling. PanelController sets `changed`, `replyChanged` and `statsChanged`.

- [ ] **Step 1: Write the failing tests**

Insert before `print("tray selftest ok")` in `tray/main.swift`:

```swift
// Panel's pure parts
let t0 = Date(timeIntervalSince1970: 0)
precondition(metaText(.idle, now: t0) == "Sonnet 5.5 · bypass", "meta idle")
precondition(metaText(.running(since: t0), now: t0.addingTimeInterval(6.4)) == "Running · 6 s", "meta running")
precondition(metaText(.done(14.2), now: t0) == "Done in 14 s" && metaText(.failed("exit 1"), now: t0) == "Failed · exit 1" && metaText(.stopped, now: t0) == "Stopped", "meta end states")
precondition(statusWord(.idle) == nil && statusWord(.running(since: t0))?.0 == "WORKING" && statusWord(.failed("x"))! == ("FAILED", .red), "status words")
precondition(phaseAfter(exit: .code(0), stopped: false, events: [.result("ok", isError: false)], duration: 3) == .done(3), "done")
precondition(phaseAfter(exit: .code(0), stopped: false, events: [.result("no", isError: true)], duration: 3) == .failed("error"), "error result")
precondition(phaseAfter(exit: .code(3), stopped: false, events: [.text("hi")], duration: 3) == .failed("exit 3"), "no result")
precondition(phaseAfter(exit: .signal(9), stopped: false, events: [], duration: 3) == .failed("signal 9"), "killed")
precondition(phaseAfter(exit: .signal(15), stopped: true, events: [], duration: 3) == .stopped, "stopped wins")
precondition(promptTitle("log 2h to https://trello.com/c/AbC123 from 09:00 please") == "log 2h to https://trello.com/c/AbC123 fr…", "title: \(promptTitle("log 2h to https://trello.com/c/AbC123 from 09:00 please"))")
precondition(promptTitle("  what\ndid I log  ") == "what did I log", "title squashes whitespace")
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `make tray-test`
Expected: compile error `cannot find 'metaText' in scope`

- [ ] **Step 3: Implement**

Create `tray/Panel.swift`:

```swift
// The tray's panel in plain AppKit, in claude-usage's Swiss style (its DESIGN.md:
// Helvetica Neue, ink on paper, Swiss red for Send and alarms, square corners,
// rules not boxes). AppKit, not SwiftUI, keeps that framework out of a menu bar
// process. The pure choices sit up top so the SELFTEST block in main.swift can
// check them. Layout helpers are claude-usage's tray/Panel.swift, trimmed.
import AppKit

// MARK: - Pure logic

enum Phase: Equatable {
    case idle
    case running(since: Date)
    case done(TimeInterval)
    case failed(String)
    case stopped
}

enum Tone: Equatable { case ink, red, muted }

/// The header's meta line.
func metaText(_ p: Phase, now: Date) -> String {
    switch p {
    case .idle: "Sonnet 5.5 · bypass"
    case .running(let since): "Running · \(Int(now.timeIntervalSince(since))) s"
    case .done(let d): "Done in \(Int(d.rounded())) s"
    case .failed(let why): "Failed · \(why)"
    case .stopped: "Stopped"
    }
}

/// The reply's status word; nil before the first run.
func statusWord(_ p: Phase) -> (String, Tone)? {
    switch p {
    case .idle: nil
    case .running: ("WORKING", .ink)
    case .done: ("DONE", .ink)
    case .failed: ("FAILED", .red)
    case .stopped: ("STOPPED", .muted)
    }
}

/// How a run ended: Stop wins; then the result event decides; without one, the exit does.
func phaseAfter(exit: Exit, stopped: Bool, events: [Event], duration: TimeInterval) -> Phase {
    if stopped { return .stopped }
    for case .result(_, let isError) in events.reversed() { return isError ? .failed("error") : .done(duration) }
    switch exit {
    case .code(let c): return .failed("exit \(c)")
    case .signal(let s): return .failed("signal \(s)")
    }
}

/// The prompt on one line, cut at 40 characters, for the reply's heading.
func promptTitle(_ s: String) -> String {
    let t = s.split(whereSeparator: \.isWhitespace).joined(separator: " ")
    return t.count > 40 ? t.prefix(40) + "…" : t
}

// MARK: - Model

/// The panel's state. Tray sets it; PanelController shows it while the panel is open.
final class PanelModel {
    var phase = Phase.idle { didSet { changed() } }
    var asked = ""                                       // the run's prompt
    var events: [Event] = [] { didSet { replyChanged() } }
    var trimmed = false                                  // older events were dropped (64 KB cap)
    var stderrTail: [String] = []                        // shown when a run fails without a result
    var repoError: String? { didSet { changed() } }
    var procsOpen = false { didSet { changed() } }
    var claude: [ProcStat]?                              // the run's process tree; Tray calls statsChanged after sampling
    var tray = ""
    var cap: CGFloat = 400                               // tallest the reply may be; Tray sets it from the screen
    // Set by PanelController.
    var changed: () -> Void = {}
    var replyChanged: () -> Void = {}
    var statsChanged: () -> Void = {}
    // Set by Tray.
    var onSend: (String) -> Void = { _ in }
    var onStop: () -> Void = {}
    var onLog: () -> Void = {}
    var onQuit: () -> Void = {}
    var onResize: () -> Void = {}
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `make tray-test`
Expected: last line `tray selftest ok`

- [ ] **Step 5: Commit**

```bash
git add tray/Panel.swift tray/main.swift
git commit -m "feat(tray): panel phases, meta and status text, model"
```

---

### Task 6: Panel views

These are claude-usage's palette and layout helpers, trimmed to what this panel uses (copied from `/Users/loithai/Projects/ai-tools/claude-usage/tray/Panel.swift`), plus `PanelController`. A state change rebuilds the panel. Reply text and stats update in place. The prompt and reply text views are made once and moved into each rebuild, so typed text survives.

**Files:**
- Modify: `tray/Panel.swift` (append)

**Interfaces:**
- Consumes: everything in Task 5; `claudeLine`, `procCells` (Task 3); `Event` (Task 2)
- Produces:
  - `final class PanelController: NSViewController` with `init(_ m: PanelModel)`, `func render()`, `func focusPrompt()` and `func fittingSize() -> NSSize`
  - `func pal(dark: Bool) -> Pal`, `let swissRed`

- [ ] **Step 1: Append the views**

Append to `tray/Panel.swift`:

```swift
// MARK: - Palette

struct Pal {
    let paper, ink, muted, redInk: NSColor
    func text(_ t: Tone) -> NSColor { t == .red ? redInk : t == .muted ? muted : ink }
}

extension NSColor {
    convenience init(hex: UInt32) { self.init(srgbRed: CGFloat(hex >> 16 & 255) / 255, green: CGFloat(hex >> 8 & 255) / 255, blue: CGFloat(hex & 255) / 255, alpha: 1) }
}

let swissRed = NSColor(hex: 0xda291c)

/// claude-usage's DESIGN.md values.
func pal(dark: Bool) -> Pal {
    dark ? Pal(paper: NSColor(hex: 0x0d0d0d), ink: NSColor(hex: 0xf2f2f2), muted: NSColor(hex: 0xa3a3a3), redInk: NSColor(hex: 0xff6a5c))
         : Pal(paper: NSColor(hex: 0xffffff), ink: NSColor(hex: 0x111111), muted: NSColor(hex: 0x5c5c5c), redInk: NSColor(hex: 0xda291c))
}

// MARK: - Pieces

enum W { case r, m, b }

/// Helvetica Neue at a fixed size, with tabular digits.
func hvFont(_ size: CGFloat, _ w: W = .r) -> NSFont {
    let f = NSFont(name: w == .b ? "HelveticaNeue-Bold" : w == .m ? "HelveticaNeue-Medium" : "HelveticaNeue", size: size) ?? .systemFont(ofSize: size)
    let tnum: [NSFontDescriptor.FeatureKey: Int] = [.typeIdentifier: kNumberSpacingType, .selectorIdentifier: kMonospacedNumbersSelector]
    return NSFont(descriptor: f.fontDescriptor.addingAttributes([.featureSettings: [tnum]]), size: size) ?? f
}

/// A text label. `em` is letter-spacing in em; `wrap` is the width it wraps at, else it stays on one line, cut per `cut`.
func label(_ s: String, _ size: CGFloat, _ w: W = .r, em: CGFloat = 0, _ color: NSColor, wrap: CGFloat? = nil,
           align: NSTextAlignment = .left, cut: NSLineBreakMode = .byTruncatingTail) -> NSTextField {
    let ps = NSMutableParagraphStyle()
    ps.alignment = align
    ps.lineBreakMode = wrap == nil ? cut : .byWordWrapping
    let a = NSAttributedString(string: s, attributes: [.font: hvFont(size, w), .kern: em * size, .foregroundColor: color, .paragraphStyle: ps])
    guard let wrap else { return NSTextField(labelWithAttributedString: a) }
    let f = NSTextField(wrappingLabelWithString: "")
    f.isSelectable = false
    f.attributedStringValue = a
    f.preferredMaxLayoutWidth = wrap
    return f
}

/// A regular-weight label.
func label(_ s: String, _ size: CGFloat, _ color: NSColor, wrap: CGFloat? = nil, align: NSTextAlignment = .left) -> NSTextField {
    label(s, size, .r, color, wrap: wrap, align: align)
}

func recolor(_ f: NSTextField, _ c: NSColor) {
    let a = NSMutableAttributedString(attributedString: f.attributedStringValue)
    a.addAttribute(.foregroundColor, value: c, range: NSRange(location: 0, length: a.length))
    f.attributedStringValue = a
}

/// Flipped, so a scroll view shows it from the top.
final class Stack: NSStackView {
    override var isFlipped: Bool { true }
}

/// A column; each item stretches to its width less the insets.
func vstack(_ items: [NSView], _ spacing: CGFloat = 0, inset: NSEdgeInsets = NSEdgeInsetsZero) -> Stack {
    let s = Stack(views: items)
    s.orientation = .vertical
    s.alignment = .leading
    s.spacing = spacing
    s.edgeInsets = inset
    for v in items { v.widthAnchor.constraint(equalTo: s.widthAnchor, constant: -inset.left - inset.right).isActive = true }
    return s
}

/// A row, centered or top-aligned; a `spacer()` or a `stretch`ed item takes the room left.
func hstack(_ items: [NSView], _ spacing: CGFloat = 0, top: Bool = false) -> Stack {
    let s = Stack(views: items)
    s.orientation = .horizontal
    s.alignment = top ? .top : .centerY
    s.distribution = .fill
    s.spacing = spacing
    return s
}

func stretch<V: NSView>(_ v: V) -> V {
    v.setContentHuggingPriority(.init(1), for: .horizontal)
    return v
}

func spacer() -> NSView { stretch(NSView()) }

func fixed<V: NSView>(_ v: V, w: CGFloat? = nil, h: CGFloat? = nil) -> V {
    v.translatesAutoresizingMaskIntoConstraints = false
    if let w { v.widthAnchor.constraint(equalToConstant: w).isActive = true }
    if let h { v.heightAnchor.constraint(equalToConstant: h).isActive = true }
    return v
}

/// A plain block of color: rules, squares.
func block(_ c: NSColor, w: CGFloat? = nil, h: CGFloat? = nil) -> NSView {
    let v = NSView()
    v.wantsLayer = true
    v.layer?.backgroundColor = c.cgColor
    return fixed(v, w: w, h: h)
}

func edges(_ t: CGFloat, _ l: CGFloat, _ b: CGFloat, _ r: CGFloat) -> NSEdgeInsets { NSEdgeInsets(top: t, left: l, bottom: b, right: r) }

/// Sets a layer property without Core Animation's implicit fade.
func still(_ f: () -> Void) {
    CATransaction.begin()
    CATransaction.setDisableActions(true)
    f()
    CATransaction.commit()
}

/// Turns `l` about its center every `period` s, unless Reduce Motion.
func turn(_ l: CALayer, _ period: Double) {
    guard !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion else { return }
    let a = CABasicAnimation(keyPath: "transform.rotation.z")
    a.byValue = -2 * Double.pi     // y-up layers: negative turns clockwise
    a.duration = period
    a.repeatCount = .infinity
    l.add(a, forKey: "turn")
}

/// A fixed-size view drawn with shape sublayers.
final class Arcs: NSView {
    init(_ size: CGFloat) {
        super.init(frame: NSRect(x: 0, y: 0, width: size, height: size))
        wantsLayer = true
        _ = fixed(self, w: size, h: size)
    }

    required init?(coder: NSCoder) { fatalError() }

    /// A ring of radius `r` around the center, from 12 o'clock clockwise to `to` (0...1).
    func arc(_ r: CGFloat, _ lw: CGFloat, _ c: NSColor, to: Double = 1) -> CAShapeLayer {
        let path = CGMutablePath()
        path.addArc(center: CGPoint(x: bounds.midX, y: bounds.midY), radius: r, startAngle: .pi / 2, endAngle: .pi / 2 - 2 * .pi, clockwise: true)
        let l = shape(path, lw, c)
        l.strokeEnd = to
        return l
    }

    func shape(_ path: CGPath, _ lw: CGFloat, _ c: NSColor) -> CAShapeLayer {
        let l = CAShapeLayer()
        l.frame = bounds
        l.path = path
        l.fillColor = nil
        l.strokeColor = c.cgColor
        l.lineWidth = lw
        l.contentsScale = NSScreen.main?.backingScaleFactor ?? 2
        layer?.addSublayer(l)
        return l
    }
}

func spinner(_ size: CGFloat, _ c: NSColor) -> (NSView, CAShapeLayer) {
    let v = Arcs(size)
    let ring = v.arc((size - 2) / 2, 2, c, to: 0.75)
    turn(ring, 0.7)
    return (v, ring)
}

/// The mark: an ink ring with a red hand at 12 and 3 o'clock, in a 32-unit box.
func clockMark(_ size: CGFloat, _ p: Pal) -> NSView {
    let v = Arcs(size), k = size / 32
    _ = v.arc(12 * k, 5 * k, p.ink)
    let hand = CGMutablePath()
    hand.move(to: CGPoint(x: 16 * k, y: 23 * k))       // layer space is y-up
    hand.addLine(to: CGPoint(x: 16 * k, y: 16 * k))
    hand.addLine(to: CGPoint(x: 21 * k, y: 16 * k))
    v.shape(hand, 3.5 * k, p.redInk).lineCap = .square
    v.setAccessibilityElement(false)
    return v
}

/// A borderless button made of its own views; `paint` restyles it when it turns hot (hover or press) or back.
final class Press: NSButton {
    var paint: ((Press, Bool) -> Void)? { didSet { repaint() } }
    override var isEnabled: Bool { didSet { repaint() } }
    private let content: NSView, pad: (CGFloat, CGFloat), minHeight: CGFloat, act: () -> Void
    private var hover = false, down = false
    private lazy var fit = content.fittingSize

    /// `pad` is (leading, trailing). `fill` stretches `content` across; otherwise it sits in the middle.
    init(_ content: NSView, pad: (CGFloat, CGFloat) = (0, 0), minHeight: CGFloat = 0, fill: Bool = false, _ act: @escaping () -> Void) {
        self.content = content
        self.pad = pad
        self.minHeight = minHeight
        self.act = act
        super.init(frame: .zero)
        title = ""
        isBordered = false
        wantsLayer = true
        target = self
        action = #selector(fire)
        translatesAutoresizingMaskIntoConstraints = false
        content.translatesAutoresizingMaskIntoConstraints = false
        addSubview(content)
        NSLayoutConstraint.activate([
            content.centerYAnchor.constraint(equalTo: centerYAnchor),
            content.topAnchor.constraint(greaterThanOrEqualTo: topAnchor),
        ] + (fill ? [
            content.leadingAnchor.constraint(equalTo: leadingAnchor, constant: pad.0),
            content.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -pad.1),
        ] : [
            content.centerXAnchor.constraint(equalTo: centerXAnchor, constant: (pad.0 - pad.1) / 2),
            content.leadingAnchor.constraint(greaterThanOrEqualTo: leadingAnchor, constant: pad.0),
        ]))
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect], owner: self))
    }

    required init?(coder: NSCoder) { fatalError() }

    override var intrinsicContentSize: NSSize { NSSize(width: fit.width + pad.0 + pad.1, height: max(minHeight, fit.height)) }
    override var focusRingMaskBounds: NSRect { bounds }
    override func drawFocusRingMask() { bounds.fill() }
    override func accessibilityChildren() -> [Any]? { nil }
    override func mouseEntered(with e: NSEvent) { hover = true; repaint() }
    override func mouseExited(with e: NSEvent) { hover = false; repaint() }

    override func mouseDown(with e: NSEvent) {
        down = true
        repaint()
        super.mouseDown(with: e)    // tracks until the mouse is up and fires the action
        down = false
        repaint()
    }

    @objc private func fire() { act() }
    private func repaint() { paint?(self, isEnabled && (hover || down)) }
}

enum BtnKind { case outline, red }

/// A boxed button (the design's .btn): 2 pt border, ink fill while hot.
func btn(_ kind: BtnKind, _ p: Pal, _ title: String, size: CGFloat = 13, h: CGFloat = 36, pad: CGFloat = 14,
         spin: Bool = false, gap: CGFloat = 6, enabled: Bool = true, _ act: @escaping () -> Void) -> Press {
    let l = label(title, size, .b, p.ink)
    let s = spin ? spinner(12, p.ink) : nil
    let b = Press(hstack([s?.0, l].compactMap { $0 }, gap), pad: (pad, pad), minHeight: h, act)
    b.layer?.borderWidth = 2
    b.isEnabled = enabled
    b.alphaValue = enabled ? 1 : 0.4
    b.paint = { b, hot in
        let (bg, fg, bd): (NSColor, NSColor, NSColor) = switch (kind, hot) {
        case (.outline, false): (.clear, p.ink, p.ink)
        case (.red, false): (swissRed, .white, swissRed)
        default: (p.ink, p.paper, p.ink)
        }
        b.layer?.backgroundColor = bg.cgColor
        b.layer?.borderColor = bd.cgColor
        recolor(l, fg)
        still { s?.1.strokeColor = fg.cgColor }
    }
    b.setAccessibilityLabel(title)
    return b
}

/// A text-only button (the design's .txt): red ink while hot.
func txtBtn(_ p: Pal, _ title: String, _ w: W, color: NSColor? = nil, pad: (CGFloat, CGFloat), key: String = "", _ act: @escaping () -> Void) -> Press {
    let l = label(title, 13, w, color ?? p.ink)
    let b = Press(l, pad: pad, minHeight: 36, act)
    b.paint = { _, hot in recolor(l, hot ? p.redInk : color ?? p.ink) }
    b.keyEquivalent = key
    b.keyEquivalentModifierMask = .command
    b.setAccessibilityLabel(title)
    return b
}

// MARK: - Panel

/// The panel's content. A state change rebuilds it; reply text and stats update in
/// place. The prompt and reply text views are made once and moved into each rebuild,
/// so typed text, selection and scroll survive.
final class PanelController: NSViewController, NSTextViewDelegate {
    let m: PanelModel
    private var queued = false, replyQueued = false
    private let promptScroll = NSTextView.scrollableTextView(), replyScroll = NSTextView.scrollableTextView()
    private var prompt: NSTextView { promptScroll.documentView as! NSTextView }
    private var reply: NSTextView { replyScroll.documentView as! NSTextView }
    private let promptBox = NSView()      // the prompt's 2 pt border: NSScrollView keeps no border on its own layer
    private lazy var replyHeight = replyScroll.heightAnchor.constraint(equalToConstant: 18)
    private var col: NSView?
    private var meta: NSTextField?, claudeStat: NSTextField?, trayStat: NSTextField?, procRows: Stack?
    private var shownPhase = Phase.idle
    private var p = pal(dark: false)

    init(_ m: PanelModel) {
        self.m = m
        super.init(nibName: nil, bundle: nil)
        m.changed = { [unowned self] in
            guard !queued else { return }
            queued = true
            DispatchQueue.main.async { [unowned self] in
                if queued, view.window?.isVisible == true { render() }
                queued = false
            }
        }
        // Batched: at most one reply update per 100 ms while the panel shows.
        m.replyChanged = { [unowned self] in
            guard !replyQueued else { return }
            replyQueued = true
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) { [unowned self] in
                if replyQueued, view.window?.isVisible == true { flushReply() }
                replyQueued = false
            }
        }
        m.statsChanged = { [unowned self] in updateStats() }
    }

    required init?(coder: NSCoder) { fatalError() }

    override func loadView() {
        let r = Root()
        r.wantsLayer = true
        r.onAppearance = { [unowned self] in if view.window?.isVisible == true { render() } }   // hidden: the next open renders
        view = r
        for s in [promptScroll, replyScroll] {
            s.drawsBackground = false
            s.borderType = .noBorder
            s.autohidesScrollers = true
            s.translatesAutoresizingMaskIntoConstraints = false
        }
        promptBox.wantsLayer = true
        promptBox.layer?.borderWidth = 2
        promptBox.translatesAutoresizingMaskIntoConstraints = false
        promptBox.addSubview(promptScroll)
        NSLayoutConstraint.activate([
            promptBox.heightAnchor.constraint(equalToConstant: 76),
            promptScroll.topAnchor.constraint(equalTo: promptBox.topAnchor, constant: 2),
            promptScroll.bottomAnchor.constraint(equalTo: promptBox.bottomAnchor, constant: -2),
            promptScroll.leadingAnchor.constraint(equalTo: promptBox.leadingAnchor, constant: 2),
            promptScroll.trailingAnchor.constraint(equalTo: promptBox.trailingAnchor, constant: -2),
        ])
        prompt.delegate = self
        prompt.isRichText = false
        prompt.allowsUndo = true
        prompt.drawsBackground = false
        prompt.textContainerInset = NSSize(width: 6, height: 6)
        prompt.isAutomaticQuoteSubstitutionEnabled = false     // card URLs and times stay as typed
        prompt.isAutomaticDashSubstitutionEnabled = false
        prompt.isAutomaticTextReplacementEnabled = false
        prompt.isContinuousSpellCheckingEnabled = false
        prompt.setAccessibilityLabel("Prompt")
        reply.isEditable = false
        reply.isSelectable = true
        reply.drawsBackground = false
        reply.textContainerInset = .zero
        reply.textContainer?.lineFragmentPadding = 0
        reply.setAccessibilityLabel("Reply")
        replyHeight.isActive = true
    }

    func focusPrompt() { view.window?.makeFirstResponder(prompt) }

    func fittingSize() -> NSSize { col?.fittingSize ?? .zero }

    /// Esc in the prompt closes the panel (NSTextView would otherwise start completion).
    func textView(_ v: NSTextView, doCommandBy sel: Selector) -> Bool {
        guard sel == #selector(NSResponder.cancelOperation(_:)) else { return false }
        view.window?.cancelOperation(nil)
        return true
    }

    func render() {
        queued = false
        let dark = view.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        p = pal(dark: dark)
        if m.phase != shownPhase {
            if case .done = m.phase { prompt.string = "" }      // a failed prompt stays, to retry
            if let (w, _) = statusWord(m.phase) {
                NSAccessibility.post(element: NSApp as Any, notification: .announcementRequested,
                                     userInfo: [.announcement: w.capitalized, .priority: NSAccessibilityPriorityLevel.high.rawValue])
            }
            shownPhase = m.phase
        }
        let focused = view.window?.firstResponder === prompt
        var items: [NSView] = [header(), promptSection()]
        if m.phase != .idle { items.append(replySection()) }
        items.append(footer())
        let c = vstack(items, 16, inset: edges(16, 16, 16, 16))
        c.translatesAutoresizingMaskIntoConstraints = false
        view.subviews.forEach { $0.removeFromSuperview() }
        view.addSubview(c)
        NSLayoutConstraint.activate([
            c.widthAnchor.constraint(equalToConstant: 360),
            c.topAnchor.constraint(equalTo: view.topAnchor),
            c.leadingAnchor.constraint(equalTo: view.leadingAnchor),
        ])
        col = c
        view.layer?.backgroundColor = p.paper.cgColor
        if focused { focusPrompt() }
        flushReply()
    }

    func header() -> NSView {
        let failed: Bool = if case .failed = m.phase { true } else { false }
        let meta = label(metaText(m.phase, now: Date()), 12, failed ? p.redInk : p.muted)
        meta.setContentCompressionResistancePriority(.init(100), for: .horizontal)
        self.meta = meta
        let top = hstack([hstack([clockMark(22, p), label("Clockify Agent", 15, .b, em: -0.02, p.ink)], 8), spacer(), meta], 12)
        return vstack([top, block(p.ink, h: 4)], 12)
    }

    func promptSection() -> NSView {
        let running: Bool = if case .running = m.phase { true } else { false }
        promptBox.layer?.borderColor = p.ink.cgColor
        promptBox.alphaValue = running ? 0.4 : 1
        prompt.isEditable = !running
        prompt.font = hvFont(15)
        prompt.textColor = p.ink
        prompt.insertionPointColor = p.ink
        // NSTextView draws this while empty; the key is long-standing but not public API.
        prompt.setValue(NSAttributedString(string: "Log 2h to a Trello card, or ask “what did I log this week?”",
                                           attributes: [.font: hvFont(15), .foregroundColor: p.muted]), forKey: "placeholderAttributedString")
        let action: Press
        if running {
            action = btn(.outline, p, "Stop", spin: true) { [unowned self] in m.onStop() }
        } else {
            action = btn(.red, p, "Send", enabled: m.repoError == nil) { [unowned self] in submit() }
            action.keyEquivalent = "\r"
            action.keyEquivalentModifierMask = .command
        }
        var items: [NSView] = [label("PROMPT", 11, .m, em: 0.06, p.muted), promptBox]
        if let e = m.repoError { items.append(label(e, 13, .b, p.redInk, wrap: 328)) }
        items.append(hstack([running ? spacer() : label("⌘↩ to send · Esc to close", 12, p.muted), spacer(), action], 10))
        return vstack(items, 8)
    }

    func submit() {
        let t = prompt.string.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !t.isEmpty, m.repoError == nil else { return }
        m.onSend(t)
    }

    func replySection() -> NSView {
        let (word, tone) = statusWord(m.phase) ?? ("", .muted)
        var status: [NSView] = []
        if case .running = m.phase { status.append(spinner(10, p.ink).0) } else if tone != .muted { status.append(block(p.text(tone), w: 8, h: 8)) }
        status.append(label(word, 11, .b, em: 0.04, p.text(tone)))
        let title = label("REPLY · " + promptTitle(m.asked), 11, .m, em: 0.06, p.muted)
        title.setContentCompressionResistancePriority(.init(100), for: .horizontal)
        let head = hstack([title, spacer(), hstack(status, 6)], 8)
        return vstack([vstack([head, block(p.ink, h: 1)], 6), replyScroll], 8)
    }

    /// Prose at 13 in ink, each Bash call as a muted "$ command" line; the result
    /// only when it adds something (it usually repeats the last prose); stderr's
    /// tail when a run failed without a result.
    func replyText() -> NSAttributedString {
        let s = NSMutableAttributedString()
        func add(_ t: String, _ size: CGFloat, _ c: NSColor, indent: CGFloat = 0) {
            let ps = NSMutableParagraphStyle()
            ps.paragraphSpacing = 6
            ps.firstLineHeadIndent = indent
            ps.headIndent = indent
            if s.length > 0 { s.append(NSAttributedString(string: "\n")) }
            s.append(NSAttributedString(string: t, attributes: [.font: hvFont(size), .foregroundColor: c, .paragraphStyle: ps]))
        }
        if m.trimmed { add("… earlier output trimmed", 12, p.muted) }
        var lastText: String?, hasResult = false
        for e in m.events {
            switch e {
            case .text(let t): add(t, 13, p.ink); lastText = t
            case .bash(let c): add("$ " + c, 12, p.muted, indent: 12)
            case .result(let t, let isError):
                hasResult = true
                if isError { add(t, 13, p.redInk) } else if t != lastText { add(t, 13, p.ink) }
            }
        }
        if case .failed = m.phase, !hasResult { m.stderrTail.forEach { add($0, 12, p.redInk) } }
        return s
    }

    func flushReply() {
        replyQueued = false
        guard col != nil, let lm = reply.layoutManager, let tc = reply.textContainer else { return }
        reply.textStorage?.setAttributedString(replyText())
        lm.ensureLayout(for: tc)
        let h = ceil(lm.usedRect(for: tc).height)
        replyHeight.constant = min(max(h, 18), max(m.cap, 120))
        replyScroll.hasVerticalScroller = h > m.cap
        reply.scrollToEndOfDocument(nil)
        m.onResize()
    }

    func footer() -> NSView {
        let row = hstack([
            txtBtn(p, "Open Log", .b, pad: (0, 8)) { [unowned self] in m.onLog() },
            spacer(),
            txtBtn(p, "Quit", .r, color: p.muted, pad: (8, 0), key: "q") { [unowned self] in m.onQuit() },
        ], 4)
        let c = label(claudeLine(m.claude), 12, p.muted), t = label(m.tray, 12, p.muted)
        claudeStat = c
        trayStat = t
        let chevron = fixed(NSImageView(image: NSImage(systemSymbolName: m.procsOpen ? "chevron.down" : "chevron.right", accessibilityDescription: nil)!
            .withSymbolConfiguration(.init(pointSize: 8, weight: .bold))!), w: 10)
        chevron.contentTintColor = p.muted
        let toggle = Press(hstack([chevron, c, spacer()], 4), fill: true) { [unowned self] in m.procsOpen.toggle() }
        toggle.paint = { [unowned self] _, hot in
            recolor(c, hot ? p.redInk : p.muted)
            chevron.contentTintColor = hot ? p.redInk : p.muted
        }
        toggle.setAccessibilityLabel("Claude processes")
        toggle.setAccessibilityValue(m.procsOpen ? "expanded" : "collapsed")
        var stats: [NSView] = [toggle]
        if m.procsOpen {
            let rows = vstack(procRowViews(), 2)
            procRows = rows
            stats.append(rows)
        } else {
            procRows = nil
        }
        stats.append(t)
        return vstack([block(p.ink, h: 1), row, vstack(stats, 3)], 3)
    }

    func procRowViews() -> [NSView] {
        guard let procs = m.claude, !procs.isEmpty else { return [] }
        func row(_ cells: [String], _ w: W, _ c: NSColor) -> NSView {
            let r = hstack(zip(cells, [44, 44, 56]).map { fixed(label($0, 12, w, c), w: CGFloat($1)) } + [stretch(label(cells[3], 12, w, c))], 6)
            r.edgeInsets.left = 14
            return r
        }
        return [row(["pid", "CPU", "Mem", "Process"], .b, p.ink)] + procs.map { row(procCells($0), .r, p.muted) }
    }

    /// Updates the stat lines, the meta line and an open process list in place.
    func updateStats() {
        guard view.window?.isVisible == true, let c = claudeStat, let t = trayStat else { return }
        c.attributedStringValue = label(claudeLine(m.claude), 12, p.muted).attributedStringValue
        t.attributedStringValue = label(m.tray, 12, p.muted).attributedStringValue
        if let meta, case .running = m.phase { meta.attributedStringValue = label(metaText(m.phase, now: Date()), 12, p.muted).attributedStringValue }
        if let rows = procRows {
            rows.arrangedSubviews.forEach { $0.removeFromSuperview() }
            procRowViews().forEach { rows.addArrangedSubview($0) }
            m.onResize()
        }
    }
}

private final class Root: NSView {
    var onAppearance: () -> Void = {}
    override var isFlipped: Bool { true }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        onAppearance()
    }
}
```

- [ ] **Step 2: Check it compiles and the tests still pass**

Run: `make tray-test`
Expected: last line `tray selftest ok`

- [ ] **Step 3: Render the four states offscreen and look at them**

This harness lives outside the repo and is not committed. It compiles everything in `tray/main.swift` above `#if SELFTEST`, plus `Panel.swift`, and draws the panel into PNGs.

```bash
SNAP=$(mktemp -d)
sed -n '1,/^#if SELFTEST/p' tray/main.swift | sed '$d' > "$SNAP/core.swift"
cat > "$SNAP/main.swift" <<'EOF'
import AppKit
let app = NSApplication.shared
app.setActivationPolicy(.accessory)
func shot(_ name: String, dark: Bool, _ setup: (PanelModel) -> Void) {
    let m = PanelModel()
    let pc = PanelController(m)
    let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 360, height: 600), styleMask: [.borderless], backing: .buffered, defer: false)
    w.appearance = NSAppearance(named: dark ? .darkAqua : .aqua)
    w.contentViewController = pc
    m.cap = 400
    m.tray = "Tray: CPU 0.0% · 15 MB"
    m.onResize = { let s = pc.fittingSize(); w.setContentSize(s) }
    setup(m)
    w.orderFrontRegardless()
    pc.render()
    RunLoop.main.run(until: Date().addingTimeInterval(0.3))
    pc.render()
    let v = w.contentView!
    v.layoutSubtreeIfNeeded()
    let rep = v.bitmapImageRepForCachingDisplay(in: v.bounds)!
    v.cacheDisplay(in: v.bounds, to: rep)
    try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: "snap/\(name).png"))
    print(name, v.bounds.size)
    w.orderOut(nil)
}
shot("1-idle", dark: false) { _ in }
shot("2-running", dark: false) { m in
    m.asked = "log 2h to https://trello.com/c/AbC123 from 09:00"
    m.phase = .running(since: Date().addingTimeInterval(-6))
    m.events = [.text("Logging 2h from 09:00 to card #442."), .bash("clockify log https://trello.com/c/AbC123 09:00 2h")]
    m.claude = [ProcStat(pid: 51204, name: "claude", cpu: 6.9, mem: 184_549_376), ProcStat(pid: 51231, name: "trello-mcp", cpu: 0.4, mem: 30_408_704), ProcStat(pid: 51290, name: "clockify", cpu: 1.1, mem: 7_340_032)]
    m.procsOpen = true
}
shot("3-done-dark", dark: true) { m in
    m.asked = "log 2h to https://trello.com/c/AbC123 from 09:00"
    m.events = [.bash("clockify log https://trello.com/c/AbC123 09:00 2h"), .text("Created entry for card #442, 09:00–11:02 (2:02). RMS total today: 4:30."), .result("Created entry for card #442, 09:00–11:02 (2:02). RMS total today: 4:30.", isError: false)]
    m.phase = .done(14)
}
shot("4-failed", dark: false) { m in
    m.asked = "log 2h yesterday"
    m.events = [.text("Checking existing entries."), .bash("clockify log --description \"x\" 2h --date 2026-10-12")]
    m.stderrTail = ["Error: --project-id is required when using --description without a card URL"]
    m.phase = .failed("exit 1")
    m.repoError = nil
}
EOF
swiftc -suppress-warnings "$SNAP/core.swift" tray/Panel.swift "$SNAP/main.swift" -o "$SNAP/snap" && (cd "$SNAP" && ./snap) && echo "$SNAP"
```

Expected output:

```
1-idle (360.0, 317.0)
2-running (360.0, 463.0)
3-done-dark (360.0, 413.0)
4-failed (360.0, 432.0)
```

Open the four PNGs in the printed folder and compare them with the approved mockup (`.superpowers/brainstorm/82305-1791856235/content/swiss-panel-v2.html` in the main checkout). Check each of these:
- the 4 pt ink rule under the header
- the 2 pt ink border around the prompt
- red Send, or outlined Stop with a spinner
- the `REPLY · …` heading with its status word
- muted `$ clockify …` lines
- red stderr in the failed state
- Open Log / Quit, then the stat lines
- the process table in the running state
- dark mode inverts paper and ink

- [ ] **Step 4: Commit**

```bash
git add tray/Panel.swift
git commit -m "feat(tray): Swiss panel views in plain AppKit"
```

---

### Task 7: Menu bar app

**Files:**
- Modify: `tray/main.swift`. Add the icon section above `#if SELFTEST`, the tests before `print(…)`, and the App section after `#endif`.
- Create: `tray/Info.plist`
- Modify: `Makefile` (replace it with the version below; it adds `app`)

**Interfaces:**
- Consumes: everything above
- Produces:
  - `enum IconState { case idle, running, failed }`, `func iconImage(_ s: IconState) -> NSImage`, `func iconLabel(_ s: IconState) -> String`
  - `func repoArg(_ args: [String]) -> String?`
  - `final class PanelWindow: NSPanel`, `final class Tray: NSObject, NSApplicationDelegate` (`init(repo: String?)`)
  - `make app`, which builds `build/Clockify Agent.app`

- [ ] **Step 1: Write the failing tests**

Insert before `print("tray selftest ok")`:

```swift
// repoArg
precondition(repoArg(["x", "--repo", "/r"]) == "/r" && repoArg(["x"]) == nil && repoArg(["x", "--repo"]) == nil, "repoArg")
precondition(iconImage(.running).size == NSSize(width: 17, height: 17) && iconImage(.idle).isTemplate, "icon")
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `make tray-test`
Expected: compile error `cannot find 'repoArg' in scope`

- [ ] **Step 3: Implement the icon and the argument helper**

Insert above `#if SELFTEST`, after the Running claude section:

```swift
// MARK: - Menu bar icon

enum IconState { case idle, running, failed }

/// Ring + hand when idle, ring + dot while running, ring + "!" after a failure.
/// A template image, so it follows the menu bar's light and dark.
func iconImage(_ s: IconState) -> NSImage {
    let side: CGFloat = 17
    let img = NSImage(size: NSSize(width: side, height: side), flipped: false) { _ in
        let c = NSPoint(x: side / 2, y: side / 2)
        NSColor.black.set()
        let ring = NSBezierPath()
        ring.appendArc(withCenter: c, radius: 7, startAngle: 0, endAngle: 360)
        ring.lineWidth = 2.2
        ring.stroke()
        switch s {
        case .idle:
            let hand = NSBezierPath()
            hand.move(to: NSPoint(x: c.x, y: c.y + 4.2))
            hand.line(to: c)
            hand.line(to: NSPoint(x: c.x + 3.2, y: c.y))
            hand.lineWidth = 1.8
            hand.stroke()
        case .running:
            NSBezierPath(ovalIn: NSRect(x: c.x - 3, y: c.y - 3, width: 6, height: 6)).fill()
        case .failed:
            NSRect(x: c.x - 0.9, y: c.y - 0.4, width: 1.8, height: 4.4).fill()
            NSRect(x: c.x - 0.9, y: c.y - 3.4, width: 1.8, height: 1.8).fill()
        }
        return true
    }
    img.isTemplate = true
    return img
}

func iconLabel(_ s: IconState) -> String {
    switch s {
    case .idle: "Clockify Agent"
    case .running: "Clockify Agent: Claude is running"
    case .failed: "Clockify Agent: the last run failed"
    }
}

/// --repo <path>, else nil.
func repoArg(_ args: [String]) -> String? {
    guard let i = args.firstIndex(of: "--repo"), i + 1 < args.count else { return nil }
    return args[i + 1]
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `make tray-test`
Expected: last line `tray selftest ok`

- [ ] **Step 5: Add the app**

Append to `tray/main.swift`, after `#endif`:

```swift
// MARK: - App

/// The panel's window, under the status item. Borderless, not an NSPopover (its
/// window alone took 13 MB in claude-usage). Non-activating: it takes the keyboard
/// without activating the app. Esc, a click elsewhere or another app taking the
/// keyboard closes it.
final class PanelWindow: NSPanel {
    var onClose: () -> Void = {}
    override var canBecomeKey: Bool { true }
    override func resignKey() { super.resignKey(); onClose() }
    override func cancelOperation(_ sender: Any?) { onClose() }
}

final class Tray: NSObject, NSApplicationDelegate {
    let repo: String?
    var item: NSStatusItem!
    let model = PanelModel()
    lazy var panel = PanelController(model)
    let window = PanelWindow(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: true)
    var clicks: Any?                // watches clicks in other apps while the panel shows
    var statsTimer: Timer?          // samples CPU and memory while the panel shows
    var traySampled: (cpu: UInt64, at: TimeInterval)?
    var procSampled: [pid_t: (cpu: UInt64, at: TimeInterval)] = [:]
    var run: Run?
    var icons: [IconState: NSImage] = [:]
    var icon = IconState.idle { didSet { if icon != oldValue { showIcon() } } }
    var quitting = false
    var signals: [DispatchSourceSignal] = []

    init(repo: String?) { self.repo = repo }

    func applicationDidFinishLaunching(_ n: Notification) {
        item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        item.button?.target = self
        item.button?.action = #selector(toggle)
        showIcon()
        // Started outside launchd (Finder, `open`) the PATH lacks claude's and clockify's directory.
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        setenv("PATH", "\(home)/.local/bin:/opt/homebrew/bin:" + (ProcessInfo.processInfo.environment["PATH"] ?? "/usr/bin:/bin"), 1)
        var isDir: ObjCBool = false
        if let repo, FileManager.default.fileExists(atPath: repo, isDirectory: &isDir), isDir.boolValue {} else {
            model.repoError = repo.map { "Repo not found: \($0)" } ?? "Start the tray with --repo (make install sets it)."
            NSLog(model.repoError!)
        }
        model.onSend = { [unowned self] in send($0) }
        model.onStop = { [unowned self] in run?.stop() }
        model.onLog = { [unowned self] in closePanel(); NSWorkspace.shared.open(logURL) }
        model.onQuit = { [unowned self] in closePanel(); quit() }
        model.onResize = { [unowned self] in place() }
        window.contentViewController = panel
        window.level = .popUpMenu
        window.hasShadow = true
        window.hidesOnDeactivate = false
        window.isReleasedWhenClosed = false
        window.onClose = { [unowned self] in closePanel() }
        for sig in [SIGTERM, SIGINT] {
            // A no-op handler, not SIG_IGN: an ignored signal stays ignored across
            // exec, and claude would inherit it and shrug off Stop's SIGTERM.
            signal(sig) { _ in }
            let src = DispatchSource.makeSignalSource(signal: sig, queue: .main)
            src.setEventHandler { [unowned self] in quit() }
            src.resume()
            signals.append(src)
        }
    }

    func showIcon() {
        let img = icons[icon] ?? iconImage(icon)
        icons[icon] = img
        item.button?.image = img
        item.button?.toolTip = iconLabel(icon)
        item.button?.setAccessibilityLabel(iconLabel(icon))
    }

    func send(_ prompt: String) {
        guard run == nil, let repo else { return }
        NSLog("prompt: \(prompt)")
        model.asked = prompt
        model.stderrTail = []
        model.events = []
        model.phase = .running(since: Date())
        do {
            let r = try Run(exe: "claude", args: claudeArgs(prompt: prompt, repo: repo), cwd: repo)
            r.onEvents = { [unowned self] ev in
                for case .bash(let c) in ev { NSLog("bash: \(c)") }
                (model.events, model.trimmed) = capEvents(model.events + ev, max: 64 * 1024)
            }
            r.onExit = { [unowned self, unowned r] e in ended(r, e) }
            run = r
            icon = .running
        } catch let e as POSIXError where e.code == .ENOENT {
            NSLog("claude not found on PATH")
            model.events = [.result("claude not found. Install Claude Code or fix PATH in the LaunchAgent.", isError: true)]
            model.phase = .failed("claude not found")
            icon = window.isVisible ? .idle : .failed
        } catch {
            NSLog("claude failed to start: \(error)")
            model.events = [.result("claude failed to start: \(error.localizedDescription)", isError: true)]
            model.phase = .failed("not started")
            icon = window.isVisible ? .idle : .failed
        }
    }

    func ended(_ r: Run, _ e: Exit) {
        NSLog("claude exited (\(e))")
        run = nil
        procSampled = [:]
        model.claude = nil
        model.stderrTail = r.stderrTail
        model.phase = phaseAfter(exit: e, stopped: r.stopped, events: model.events, duration: Date().timeIntervalSince(r.started))
        if case .failed = model.phase, !window.isVisible { icon = .failed } else { icon = .idle }
        if quitting { exit(0) }
    }

    @objc func toggle() {
        if window.isVisible { closePanel() } else { openPanel() }
    }

    func openPanel() {
        guard let b = item.button, let bar = b.window else { return }
        if icon == .failed { icon = .idle }
        let r = bar.convertToScreen(b.convert(b.bounds, to: nil))
        model.cap = ((bar.screen ?? NSScreen.main)?.visibleFrame.height ?? 800) - 360
        traySampled = nil
        procSampled = [:]
        sampleStats()
        statsTimer?.invalidate()
        let t = Timer(timeInterval: 2, repeats: true) { [unowned self] _ in sampleStats() }
        RunLoop.main.add(t, forMode: .common)
        statsTimer = t
        panel.render()                  // sizes the window
        place(under: r)
        window.makeKeyAndOrderFront(nil)
        panel.focusPrompt()
        // The status item's own clicks reach us as global events; its action does the toggling.
        clicks = NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown]) { [unowned self] e in
            if !r.contains(e.locationInWindow) { closePanel() }
        }
    }

    /// Keeps the panel under the status item and on screen; after a resize, keeps its top edge.
    func place(under r: NSRect? = nil) {
        guard let b = item.button, let bar = b.window else { return }
        let r = r ?? bar.convertToScreen(b.convert(b.bounds, to: nil))
        let vis = (bar.screen ?? NSScreen.main)?.visibleFrame ?? .zero
        let s = panel.fittingSize()
        window.setFrame(NSRect(x: min(max(r.midX - s.width / 2, vis.minX), vis.maxX - s.width), y: bar.frame.minY - s.height,
                               width: s.width, height: s.height), display: true)
    }

    func closePanel() {
        guard window.isVisible else { return }
        window.orderOut(nil)
        if let c = clicks { NSEvent.removeMonitor(c) }
        clicks = nil
        statsTimer?.invalidate()
        statsTimer = nil
    }

    /// The tray's own usage and, during a run, the claude process tree's.
    func sampleStats() {
        let at = ProcessInfo.processInfo.systemUptime
        if let u = usage(of: getpid()) {
            model.tray = trayLine(cpu: cpuPercent(traySampled, (u.cpu, at)), mem: u.mem)
            traySampled = (u.cpu, at)
        }
        defer { model.statsChanged() }
        guard let r = run else { model.claude = nil; return }
        var next: [pid_t: (cpu: UInt64, at: TimeInterval)] = [:]
        model.claude = descendants(of: r.pid, children: children(of:)).compactMap { pid in
            guard let u = usage(of: pid) else { return nil }     // gone between listing and sampling
            next[pid] = (u.cpu, at)
            return ProcStat(pid: pid, name: procName(pid), cpu: cpuPercent(procSampled[pid], (u.cpu, at)), mem: u.mem)
        }
        procSampled = next
    }

    func quit() {
        guard let r = run else { exit(0) }
        quitting = true
        r.stop()
        DispatchQueue.main.asyncAfter(deadline: .now() + 3.5) { exit(0) }
    }
}

let app = NSApplication.shared
let tray = Tray(repo: repoArg(CommandLine.arguments))
app.delegate = tray
app.setActivationPolicy(.accessory)
app.run()
```

Create `tray/Info.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<!-- Contents/Info.plist of Clockify Agent.app, copied by `make app`. -->
<plist version="1.0">
<dict>
	<key>CFBundleIdentifier</key>
	<string>com.clockifyagent.tray</string>
	<key>CFBundleName</key>
	<string>Clockify Agent</string>
	<key>CFBundleDisplayName</key>
	<string>Clockify Agent</string>
	<key>CFBundleExecutable</key>
	<string>clockify-agent-tray</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>1.0</string>
	<key>CFBundleVersion</key>
	<string>1</string>
	<!-- posix_spawn_file_actions_addchdir is macOS 26+. -->
	<key>LSMinimumSystemVersion</key>
	<string>26.0</string>
	<key>LSUIElement</key>
	<true/>
</dict>
</plist>
```

Replace `Makefile` with:

```make
CLI         := clockify
TRAY        := clockify-agent-tray
SWIFT_SRC   := $(wildcard tray/*.swift)
SELFTEST    := $(or $(TMPDIR),/tmp)/$(TRAY)-selftest
# A space in the name: quote every path that holds $(APP).
APP         := Clockify Agent.app
# Login Items shows the app's name only for a team-signed agent (an ad-hoc one
# shows as "exec"): sign with TEAM's Apple Development identity, as gws-mcp and
# claude-usage do. "-" (ad-hoc) when this Mac has none.
TEAM        ?= S36W2NCK97
SIGN_ID     ?= $(or $(shell security find-identity -v -p codesigning 2>/dev/null | sed -n 's/.*"\(Apple Development: .*\)"$$/\1/p' | while read -r n; do security find-certificate -c "$$n" -p 2>/dev/null | openssl x509 -noout -subject 2>/dev/null | grep -q "OU=$(TEAM)" && { echo "$$n"; break; }; done),-)

.PHONY: build test tray-test app check

build: ## compile the CLI
	go build -o $(CLI) .

test: ## Go unit tests
	go test ./...

# No XCTest target; the tray's pure parts and its process runner are checked behind -D SELFTEST, as in gws-mcp and claude-usage.
tray-test: ## check the tray's parsing, formatting, process tree and runner
	swiftc -D SELFTEST -suppress-warnings $(SWIFT_SRC) -o $(SELFTEST)
	$(SELFTEST)

app: ## build build/Clockify Agent.app, signed with SIGN_ID
	rm -rf "build/$(APP)" && mkdir -p "build/$(APP)/Contents/MacOS"
	swiftc -O $(SWIFT_SRC) -o "build/$(APP)/Contents/MacOS/$(TRAY)"
	cp tray/Info.plist "build/$(APP)/Contents/"
	codesign --force --sign "$(SIGN_ID)" "build/$(APP)"

check: test tray-test
```

- [ ] **Step 6: Build the app, then smoke-test it and measure its footprint**

```bash
make tray-test && make app
plutil -lint tray/Info.plist
"build/Clockify Agent.app/Contents/MacOS/clockify-agent-tray" --repo "$PWD" > /tmp/ck-tray-smoke.log 2>&1 &
P=$!; sleep 5; footprint $P | sed -n 2p; ps -o %cpu=,time= -p $P; kill $P; wait $P; echo "exit $?"
```

Expected:
- `tray selftest ok`
- `tray/Info.plist: OK`
- a footprint line near `Footprint: 15 MB`
- CPU `0.0` with about `0:00.1` of CPU time
- `exit 0` (SIGTERM goes through `quit()`)

A clock icon appears in the menu bar for those 5 s.

- [ ] **Step 7: Commit**

```bash
git add tray/main.swift tray/Info.plist Makefile
git commit -m "feat(tray): menu bar app, panel window, run and stats wiring"
```

---

### Task 8: Login item, install, docs and end-to-end checks

**Files:**
- Create: `tray/com.clockifyagent.tray.plist`
- Modify: `Makefile` (replace it with the full version; it adds `install` and `uninstall`)
- Modify: `CLAUDE.md` (append a tray section)

**Interfaces:**
- Consumes: `make build`, `make app`
- Produces: `make install`, `make uninstall`, and the login item `com.clockifyagent.tray`

- [ ] **Step 1: Add the LaunchAgent template**

Create `tray/com.clockifyagent.tray.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<!-- Rendered into ~/Library/LaunchAgents by `make install`; @HOME@ and @REPO@ are filled in there. -->
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>com.clockifyagent.tray</string>
	<key>ProgramArguments</key>
	<array>
		<string>@HOME@/Applications/Clockify Agent.app/Contents/MacOS/clockify-agent-tray</string>
		<string>--repo</string>
		<string>@REPO@</string>
	</array>
	<!-- Login Items names this agent after the app. -->
	<key>AssociatedBundleIdentifiers</key>
	<string>com.clockifyagent.tray</string>
	<key>RunAtLoad</key>
	<true/>
	<!-- Relaunch only on a crash: Quit exits 0 and stays down until next login. -->
	<key>KeepAlive</key>
	<dict>
		<key>SuccessfulExit</key>
		<false/>
	</dict>
	<!-- claude and clockify live in ~/.local/bin. -->
	<key>EnvironmentVariables</key>
	<dict>
		<key>PATH</key>
		<string>@HOME@/.local/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
	</dict>
	<!-- Quit during a run waits up to 3.5 s for claude's process group to stop. -->
	<key>ExitTimeOut</key>
	<integer>10</integer>
	<key>StandardOutPath</key>
	<string>@HOME@/Library/Logs/clockify-agent-tray.log</string>
	<key>StandardErrorPath</key>
	<string>@HOME@/Library/Logs/clockify-agent-tray.log</string>
</dict>
</plist>
```

- [ ] **Step 2: Replace the Makefile with the full version**

```make
CLI         := clockify
TRAY        := clockify-agent-tray
SWIFT_SRC   := $(wildcard tray/*.swift)
SELFTEST    := $(or $(TMPDIR),/tmp)/$(TRAY)-selftest
# A space in the name: quote every path that holds $(APP).
APP         := Clockify Agent.app
AGENT       := com.clockifyagent.tray
AGENT_PLIST := $(HOME)/Library/LaunchAgents/$(AGENT).plist
BIN_DIR     := $(HOME)/.local/bin
APP_DIR     := $(HOME)/Applications
# Login Items shows the app's name only for a team-signed agent (an ad-hoc one
# shows as "exec"): sign with TEAM's Apple Development identity, as gws-mcp and
# claude-usage do. "-" (ad-hoc) when this Mac has none.
TEAM        ?= S36W2NCK97
SIGN_ID     ?= $(or $(shell security find-identity -v -p codesigning 2>/dev/null | sed -n 's/.*"\(Apple Development: .*\)"$$/\1/p' | while read -r n; do security find-certificate -c "$$n" -p 2>/dev/null | openssl x509 -noout -subject 2>/dev/null | grep -q "OU=$(TEAM)" && { echo "$$n"; break; }; done),-)

# Unload the login agent and wait until it has exited.
stop-agent = launchctl bootout gui/$$(id -u)/$(AGENT) 2>/dev/null; while launchctl print gui/$$(id -u)/$(AGENT) >/dev/null 2>&1; do sleep 0.2; done

.PHONY: build test tray-test app check install uninstall

build: ## compile the CLI
	go build -o $(CLI) .

test: ## Go unit tests
	go test ./...

# No XCTest target; the tray's pure parts and its process runner are checked behind -D SELFTEST, as in gws-mcp and claude-usage.
tray-test: ## check the tray's parsing, formatting, process tree and runner
	swiftc -D SELFTEST -suppress-warnings $(SWIFT_SRC) -o $(SELFTEST)
	$(SELFTEST)

app: ## build build/Clockify Agent.app, signed with SIGN_ID
	rm -rf "build/$(APP)" && mkdir -p "build/$(APP)/Contents/MacOS"
	swiftc -O $(SWIFT_SRC) -o "build/$(APP)/Contents/MacOS/$(TRAY)"
	cp tray/Info.plist "build/$(APP)/Contents/"
	codesign --force --sign "$(SIGN_ID)" "build/$(APP)"

check: test tray-test

install: build app ## install the CLI, run the tray now and at every login
	sed -e 's|@HOME@|$(HOME)|g' -e 's|@REPO@|$(CURDIR)|g' tray/$(AGENT).plist > $(AGENT).plist.tmp
	plutil -lint $(AGENT).plist.tmp
	@$(stop-agent)
	mkdir -p $(BIN_DIR) "$(APP_DIR)" $(dir $(AGENT_PLIST))
	install -m 755 $(CLI) $(BIN_DIR)/
	rm -rf "$(APP_DIR)/$(APP)" && cp -R "build/$(APP)" "$(APP_DIR)/"
	/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$(APP_DIR)/$(APP)"
	mv $(AGENT).plist.tmp $(AGENT_PLIST)
	launchctl bootstrap gui/$$(id -u) $(AGENT_PLIST)

uninstall: ## stop the tray, remove the login item and the app (the CLI stays)
	@$(stop-agent)
	rm -rf $(AGENT_PLIST) "$(APP_DIR)/$(APP)"
```

- [ ] **Step 3: Check the template renders and lints**

```bash
sed -e "s|@HOME@|$HOME|g" -e "s|@REPO@|$PWD|g" tray/com.clockifyagent.tray.plist > /tmp/ck-agent.plist && plutil -lint /tmp/ck-agent.plist
make -n install | head -3
```

Expected: `/tmp/ck-agent.plist: OK`, then `go build -o clockify .` as the first line of the dry run.

- [ ] **Step 4: Append the tray section to `CLAUDE.md`**

Append at the end of `CLAUDE.md`:

```markdown
## Tray (`tray/`)

Menu bar app "Clockify Agent". Click the clock and type a prompt; Send runs headless Claude Code (`claude -p`, `claude-sonnet-5-5`, `--permission-mode bypassPermissions`) in this repo, and the reply streams into the panel. The footer shows the CPU and memory of the run's process tree and of the tray itself.

- **Test**: `make tray-test` (pure parts and the process runner, behind `-D SELFTEST`); `make check` also runs `go test ./...`
- **Build**: `make app` builds `build/Clockify Agent.app`
- **Install**: `make install` from this checkout: the LaunchAgent's `--repo` is the directory it runs in. It installs `~/.local/bin/clockify`, `~/Applications/Clockify Agent.app` and the login item `com.clockifyagent.tray`. Remove: `make uninstall`.
- **Log**: `~/Library/Logs/clockify-agent-tray.log` (prompts, Bash commands, exit codes, claude's stderr)

Architecture:
- `tray/main.swift`: claude argv and stream-json parsing, process usage via libproc (`proc_pid_rusage`, `proc_listchildpids`), the `Run` process runner (posix_spawn in its own process group), the menu bar icon, the `Tray` app delegate
- `tray/Panel.swift`: panel phases and model, claude-usage's Swiss palette and layout helpers, `PanelController`

Footprint rules: plain AppKit (no SwiftUI, no NSPopover); no timers while the panel is closed; stats every 2 s only while it shows; reply capped at 64 KB. Measured idle with the panel closed: 15 MB, 0.0% CPU.

Runs bypass permissions: Claude runs commands without asking. Only text typed into the panel reaches it.
```

- [ ] **Step 5: Run all checks**

Run: `make check`
Expected: `go test ./...` reports `ok` for every package, then `tray selftest ok`.

- [ ] **Step 6: Commit**

```bash
git add tray/com.clockifyagent.tray.plist Makefile CLAUDE.md
git commit -m "feat(tray): login item, make install/uninstall, docs"
```

- [ ] **Step 7: Install and run the end-to-end checks**

Install from the main checkout, after the branch is merged. To verify before merging, run `make install` in the worktree, then re-run it from the main checkout after the merge, so the LaunchAgent's `--repo` is the permanent path.

```bash
cd /Users/loithai/Projects/ai-tools/clockify-agent && make install
launchctl print gui/$(id -u)/com.clockifyagent.tray | grep -E "state|program" | head -3
```

Expected: `state = running`.

Then check each item by hand, or dispatch the `e2e-tester` agent (aura-pilot, macOS desktop). If you dispatch it, tell the user it may ask them to act on screen.

1. Click the menu bar clock. The panel opens under it with the prompt focused. Type `what did I log today?` without clicking the box first, then press ⌘↩. Expected: the reply streams, a muted `$ clockify summary today` line appears (not `go run`), the status ends `DONE`, and the meta line reads `Done in N s`. (Review Focus 1)
2. Reopen the panel, type something, and press Esc. Expected: the panel closes and the text is still there on reopen.
3. During a run, click `Claude: N processes` in the footer. Expected: rows for `claude`, `trello-mcp` and the `clockify` child, with CPU and memory changing every 2 s.
4. Close the panel mid-run and reopen it. Expected: progress is kept, and the menu bar icon shows the dot while the run is going.
5. Send `show this month's summary by task, then explain every entry in detail`. Expected: the reply area scrolls inside the panel, and the panel's bottom edge stays on screen. (Review Focus 5)
6. Send a prompt, press Stop within 2 s, then run `pgrep -fl "claude -p|trello-mcp"`. Expected: `STOPPED`, and `pgrep` prints nothing. (Review Focus 2)
7. Send a prompt, then run `make install` again while it runs, then `pgrep -fl "claude -p|trello-mcp"`. Expected: no process left; the tray restarts. (Review Focus 2)
8. With the panel closed and no run, check Activity Monitor or `footprint` on `clockify-agent-tray`. Expected: about 15 MB and 0.0% CPU.
9. `tail -20 ~/Library/Logs/clockify-agent-tray.log`. Expected: `prompt: …`, `bash: …` and `claude exited (code(0))` lines.

If item 1 shows keystrokes going to the previously active app instead of the prompt, the non-activating panel did not become key. Stop and report it; don't change the activation policy blindly.

# Clockify Agent Tray — Design Spec

## Context

Logging time today means opening a terminal and a Claude Code session, or typing `clockify log …` by hand. The sibling projects already ship menu bar apps: gws-mcp (`/Users/loithai/Projects/ai-tools/gws-mcp/tray/`) shows the CPU and memory of its servers, and claude-usage (`/Users/loithai/Projects/ai-tools/claude-usage/tray/`) has a Swiss-style AppKit panel tuned for a small footprint.

## Goal

A menu bar app for clockify-agent. Clicking its icon opens a panel with a prompt box. Send runs the prompt through headless Claude Code (`claude -p`, model `claude-sonnet-5-5`) in this repo, so the clockify skill and the `clockify` binary handle it. The panel streams the reply and shows the CPU and memory of the Claude run and of the tray itself. The tray stays cheap: near-zero CPU while idle, and memory in the claude-usage tray's range.

## Decisions

| Topic | Decision |
|---|---|
| Claude path | `claude -p` headless, cwd = this repo, your Claude Code login (no API key) |
| Model | `claude-sonnet-5-5` |
| Permissions | `--permission-mode bypassPermissions`: runs never ask for approval (chosen by the user) |
| Memory between prompts | None: every Send is a fresh run, `--no-session-persistence` |
| Concurrency | One run at a time; Send is replaced by Stop while running |
| UI | Borderless panel under the icon, claude-usage's DESIGN.md (approved mockup: `.superpowers/brainstorm/82305-1791856235/content/swiss-panel-v2.html`) |
| CPU/mem shown | The run's process tree (claude and its descendants) and the tray itself |
| Language | Swift + AppKit, single-module `swiftc` build, as in both sibling trays; macOS 26+ (`posix_spawn_file_actions_addchdir`) |

## Security

`bypassPermissions` lets Claude run any command as the user with no confirm step: a misread prompt can create a wrong Clockify entry, or worse. Mitigations kept small on purpose:

- Only the panel starts a run. Trello card text also reaches Claude: titles, descriptions and comments it reads through the trello MCP, and card titles that come back in `clockify log` output. Treat card text as untrusted. A card written to inject instructions could make a run execute commands, write to Trello (the MCP's comment, card-edit and card-create tools), or upload a local file with its `add_attachment` (for example `~/.config/clockify-agent/config.yaml`, which holds the Clockify API key).
- `--tools Bash,Read,Skill` and `--strict-mcp-config` (trello only) limit which tools load, not what they reach: Bash runs as the user with full file and network access.
- `--append-system-prompt` asks runs to stick to time tracking (below). It is guidance, not enforcement, and injected text can override it.
- The log records every prompt, every Bash command and the exit code. MCP tool calls (Trello reads and writes) are not logged or shown in the panel.

## Claude invocation

```
claude -p <prompt>
  --model claude-sonnet-5-5
  --permission-mode bypassPermissions
  --setting-sources project,local
  --strict-mcp-config --mcp-config <repo>/.mcp.json
  --tools Bash,Read,Skill
  --effort low
  --no-session-persistence
  --append-system-prompt "You are a Clockify time-tracking assistant. Use the installed clockify binary on PATH, as the clockify skill describes. Never build, run or edit this repository's source code."
  --output-format stream-json --verbose
  -- <prompt>
```

The prompt goes last, after `--`, so a prompt starting with `-` stays a prompt (tested with `--help: …`).

- `--setting-sources project,local` skips user settings: 10 plugins and their hooks. The user `env` holds only `CLAUDE_CODE_SUBAGENT_MODEL`, which runs don't need. Auth comes from the keychain, so it is unaffected.
- `--bare` is out: it requires `ANTHROPIC_API_KEY` and ignores the Claude Code login.
- Checked on 2026-10-13 with a read-only `clockify summary today` run: exit 0, 9.8 s, peak footprint 150 MB. The init event showed model `claude-sonnet-5-5`, trello `connected`, and built-in tools `Bash, Read, Skill`.

## Skill fixes (found during that check)

The run listed no clockify skill and ran `go run . summary today` instead of `clockify`. Two causes, both fixed as part of this work:

1. `.claude/skills/` does not exist, so Claude Code never loads `.agents/skills/joe-clock`. Fix: `ln -s ../../.agents/skills/joe-clock .claude/skills/joe-clock`.
2. The uncommitted edit to `.agents/skills/joe-clock/SKILL.md` dropped the frontmatter's closing `---`. Fix: restore the `---` line after `description:`. The rest of the edit stays as it is.

## Files

| File | Purpose |
|---|---|
| `tray/main.swift` | Pure logic (args, stream-json, process tree, formatting) + SELFTEST block, `Tray` app delegate, run and stop, sampling |
| `tray/Panel.swift` | `PanelModel`, palette, fonts, layout helpers (copied from claude-usage's `Panel.swift` and trimmed), panel views |
| `tray/Info.plist` | `Clockify Agent.app`, bundle `com.clockifyagent.tray`, `LSUIElement` |
| `tray/com.clockifyagent.tray.plist` | LaunchAgent template (`@HOME@`, `@REPO@`), as in gws-mcp |
| `Makefile` | `build` (Go CLI), `tray`, `tray-test`, `app`, `install`, `uninstall`, `check` |
| `.gitignore` | `.superpowers/`, `build/` |
| `CLAUDE.md` tray section | What the tray does, build/test/install commands, log path (the repo has no README) |

No app icon in the first version: the app uses the default icon, and a `make icon` target can come later.

## Panel

Width 360, padding 16, 16 between sections, DESIGN.md colors in light and dark, Helvetica Neue with tabular digits, square corners, rules instead of boxes.

- **Header:** clock mark (ink ring, red hand) and "Clockify Agent" at 15 bold. Right side: a 12 muted meta line. It reads `Sonnet 5.5 · bypass` when idle, `Running · 6 s` during a run, then `Done in 14 s`, `Failed · exit 1` or `Stopped`. A 4px ink rule closes the header.
- **Prompt:** 11 uppercase label, an `NSTextView` with a 2px ink border and min height 76 at 15 regular, placeholder text. Below it: `⌘↩ to send · Esc to close` (muted) and **Send**, the primary button: Swiss red with white text. While running, the text is dimmed to 40% and read-only, and Send becomes **Stop**: outlined ink, with a spinner.
- **Reply** (hidden until the first run): an 11 uppercase label with the prompt's first 40 chars, and a status word on the right: spinner + `WORKING`, ink square + `DONE`, red square + `FAILED`, or muted `STOPPED`. A 1px ink rule follows. Then assistant text at 13, with each Bash call as a muted 12 `$ command` line indented 12. The final `result` text comes last, but only when it is an error (red) or differs from the last prose, which it usually repeats. The prompt clears after a run ends `DONE` and stays after a failure, to retry. The area scrolls once it outgrows the screen; header and footer stay put.
- **Footer:** 1px ink rule, then a row with **Open Log** (bold text button) and **Quit** (muted), as in claude-usage. Below it, two muted 12 stat lines. The first is `Claude: idle`, or `› Claude: 3 processes · CPU 8.4% · 212 MB`; clicking it expands one row per process (pid, CPU, memory, `proc_name` short name). The second is `Tray: CPU 0.0% · 15 MB`.
- **Closing:** Esc, a click outside, or another app taking the keyboard closes the panel. A run keeps going while the panel is closed, and reopening shows its progress.
- **Accessibility:** the status word change is announced (`NSAccessibility` announcement), buttons are focusable, and the reply view is selectable.

### Menu bar icon

A template image drawn once per state and set only when the state changes. No animation:

- idle: ring + hand
- running: ring + filled dot
- failed: ring + `!`, until the panel next opens

The tooltip and accessibility label carry the same state in words.

## Footprint rules

From claude-usage's measured choices:

1. AppKit only, no SwiftUI, no `NSPopover` (its window alone took 13 MB there). The panel is an `NSPanel` (`.borderless, .nonactivatingPanel`, `defer: true`), built on first open and kept.
2. No `ps`. Process usage comes from `proc_pid_rusage(RUSAGE_INFO_V2)` (CPU ns, `ri_phys_footprint`). The tree comes from `proc_listchildpids`, walked from the claude pid. CPU % is the delta between two samples, as `cpuPercent` does in claude-usage.
3. One 2 s stats timer, only while the panel is visible. With the panel closed, no timers run, even mid-run.
4. Stream reading happens on a `FileHandle.readabilityHandler`. Parsed events queue up and are flushed to the reply view at most every 100 ms. The view appends; it never re-lays out the whole panel per line.
5. The reply keeps the newest 64 KB of event text (`capEvents`); older events are dropped, with a `… earlier output trimmed` line. Stderr keeps its last 20 lines in a ring buffer.
6. The status item image is cached per state; the icon is never redrawn on a timer.

## Run lifecycle

1. Send: trim the prompt. Empty means no-op. Log `prompt: …`. Spawn claude with `posix_spawn` and `POSIX_SPAWN_SETPGROUP` so Stop can signal the whole tree. Trade-off: if the tray is `kill -9`'d mid-run, launchd won't reap that group, so the run finishes on its own. That's acceptable for a one-shot run, unlike claude-usage's long-lived server. PATH is `$HOME/.local/bin:/opt/homebrew/bin:` plus the inherited PATH. Stdout and stderr are pipes, and cwd is `--repo`.
2. Each stdout line goes to `parseEvents`, which returns `[Event]` because one assistant message can hold several blocks. `.text` appends prose, and `.bash(cmd)` appends a `$ cmd` line and logs `bash: cmd`. `.result(text, isError)` sets the final text, and anything else is ignored.
3. Exit, seen through `DispatchSource.makeProcessSource(.exit)` and `waitpid`. The tray then SIGTERMs the process group, to clean up a leftover MCP server or background job, and waits at most 1 s for stdout's EOF. With a `result` event, the status is `DONE` or `FAILED` per `is_error`. Without one, the status is `FAILED · exit N`, showing the stderr tail. The exit code is logged.
4. Stop: `kill(-pgid, SIGTERM)`, then `SIGKILL` after 3 s if the process is still alive. Status becomes `STOPPED` and partial output stays.
5. Quit during a run: Stop first, then exit once the process has exited, or after 3 s.

## Errors

| Case | Shown in the reply area |
|---|---|
| `claude` not on PATH (`posix_spawn` ENOENT) | `claude not found. Install Claude Code or fix PATH in the LaunchAgent.` |
| Non-zero exit, no `result` event | `FAILED · exit N` + last stderr lines |
| `result.is_error` | Result text in red-ink |
| Stop pressed | `STOPPED`, partial output kept |
| `--repo` missing or not a directory at launch | Panel shows `Repo not found: <path>` and Send is disabled |
| `proc_pid_rusage` fails (process gone) | That pid is dropped from the sum; no error shown |

## Pure units (checked under `-D SELFTEST`)

- `claudeArgs(prompt:repo:) -> [String]`: the argv above. Check: the prompt is passed as one argv element (no shell), and the mcp-config path is under repo.
- `parseEvents(_ line: Substring) -> [Event]`: `.text`, `.bash(String)`, `.result(String, isError: Bool)`. Check against trimmed real lines from the 2026-10-13 run: init gives `[]`, assistant tool_use Bash gives `.bash("go run . summary today")`, assistant text gives `.text`, result success gives `.result(…, false)`. Also an `is_error: true` result, a `rate_limit_event`, and a malformed line, which gives `[]`.
- `descendants(of root: pid_t, children: (pid_t) -> [pid_t]) -> [pid_t]`: breadth-first, root first, cycle-safe. Check with a fake children function: claude → [bash, trello-mcp], bash → [clockify]; an unrelated pid is never included.
- `cpuPercent`, `mem(_ bytes:)`, `claudeLine(_ procs: [ProcSample]?)`, `trayLine(cpu:mem:)`, `procRow(_:)`. Check the exact strings: `Claude: idle`, `Claude: 3 processes · CPU 8.4% · 212 MB`, `Tray: CPU 0.0% · 14 MB`.
- `capEvents(_ events: [Event], max: Int) -> (kept: [Event], dropped: Bool)`: keeps the newest events within 64 KB. A single oversized event keeps its tail, and the panel shows `… earlier output trimmed` when `dropped`.
- `usage(of: -1) == nil`.

## Install

`make install` (mirrors gws-mcp and claude-usage):

1. Build the Go CLI to `~/.local/bin/clockify`, so the binary the skill names is current.
2. Build `build/Clockify Agent.app`: `swiftc -O tray/main.swift tray/Panel.swift`, then copy `Info.plist`. Sign with the team's Apple Development identity, or ad-hoc when the Mac has none. Use the same `SIGN_ID` logic as the sibling Makefiles.
3. Copy the app to `~/Applications`, `lsregister` it, render the LaunchAgent (`@HOME@`, `@REPO@` = repo root), `plutil -lint` it, then bootout the old agent and bootstrap the new one.

The LaunchAgent sets `RunAtLoad` and `KeepAlive` {`SuccessfulExit`: false}, so Quit stays down until next login. PATH is `~/.local/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin`. `ExitTimeOut` is 10 s. Log: `~/Library/Logs/clockify-agent-tray.log`.

`make uninstall` stops the agent and removes the plist and the app; the CLI stays.

## Testing

- `make tray-test`: the SELFTEST binary checks all pure units above.
- `go test ./...` stays green; the Go code is untouched.
- Manual checks after `make install`:
  1. Prompt `what did I log today?`. The reply streams, the `$ clockify summary today` line appears (not `go run`), and the status reads `DONE`.
  2. While a run is going, expand the Claude stats. It lists claude, trello-mcp and the clockify child.
  3. Close and reopen the panel mid-run; progress is kept.
  4. Stop on a long prompt. No claude or trello-mcp process is left (`pgrep -f trello-mcp`).
  5. Idle footprint with the panel closed: CPU 0.0% in Activity Monitor and about 15 MB (a prototype release build measured 15 MB and 0.0% on 2026-10-13). Record the measured number in the CLAUDE.md tray section.

## Out of scope

Finish notifications, global hotkey, prompt history, multi-turn sessions, concurrent runs, app icon, cost display.

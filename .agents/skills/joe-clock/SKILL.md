---
name: clockify
description: "Clockify time-tracking helper for logging, summarizing, and validating time entries against Clockify constraints. Triggers include: log time to a Trello card, log start-and-duration or start-and-end ranges, log durations like 2h/1h30m, dry-run validation, by-task/project summaries, and resolving overlap/14h daily cap issues. This skill is strictly limited to Clockify time tracking and does not handle Slack, Trello browsing, weekly reports, or other PM tasks. Context note: if many skills are loaded, descriptions may be shortened to fit the 2% skills context budget, so disable unused skills/plugins to preserve room for active ones."
---

# joe-clock

Clockify time-tracking helper. Log entries, view summaries, and resolve format questions.

## Binary

`clockify` is globally installed and on `PATH` (typically `~/.local/bin/clockify`).

If missing, rebuild from source:
```bash
cd /Users/loithai/Projects/ai-tools/clockify-agent && make install
```

Verify:
```bash
clockify --help
```

## CLI

```
clockify log [card-url] <duration>
clockify log [card-url] <start> <end>
clockify log [card-url] <start> <duration>
clockify log [card-url> "4h ago" <duration>
clockify log --description "text" <duration>
clockify log --description "text" <start> <duration>

clockify summary <preset>
clockify summary --from YYYY-MM-DD --to YYYY-MM-DD
```

### Log flags
- `--date YYYY-MM-DD` — Date for the entry (default: today)
- `--dry-run` — Print entry details without creating
- `--project-id ID` — Override default project ID (required with `--description`)
- `--task-id ID` — Task ID for the entry
- `--description TEXT` — Description for entry (use instead of card URL)

### Summary flags
- `--by-task` — Show per-task breakdown within each project
- `--from YYYY-MM-DD` — Start date (required if no preset)
- `--to YYYY-MM-DD` — End date (required if no preset)

**Summary presets:** `today`, `yesterday`, `this-week`, `last-week`, `this-month`, `last-month`

## Logging modes

When a Trello card URL is provided, the binary fetches card info via `trello-agent meta` to get the card number, title, and board ID.

| Mode | Syntax | Start | End |
|------|--------|-------|-----|
| A. Duration only | `clockify log <url> 2h` | now - 2h | now |
| B. Start + end | `clockify log <url> 09:00 11:30` | 09:00 | 11:30 |
| C. Start + duration | `clockify log <url> 09:00 2h30m` | 09:00 | 11:30 |
| D. Relative start | `clockify log <url> "4h ago" 30m` | now - 4h | now - 3h30m |

For Mode B and C, start/end times are interpreted in **VNT (UTC+7)**. For Mode A and D, the end time defaults to "now" in VNT.

**No-card mode:** use `--description "free text"` with `--project-id` for ad-hoc entries.

A random 1-5% duration padding is applied automatically to avoid round numbers.

## Duration formats

| Format | Example | Seconds |
|--------|---------|---------|
| `Xh` | `2h` | 7200 |
| `XhYm` | `1h30m` | 5400 |
| `Xm` | `45m` | 2700 |
| `H:MM` | `1:30` | 5400 |
| `H:MM:SS` | `2:12:05` | 7925 |

`1.5h` is NOT supported — use `1h30m` instead.

## Time format

`HH:MM` 24-hour, e.g. `09:00`, `14:30`, `23:45`.

## Date format

`YYYY-MM-DD`, e.g. `2026-06-27`.

## Trello card URLs

Standard patterns:
- `https://trello.com/c/{card-id}` (most common)
- `https://trello.com/c/{card-id}/{slug}`

The binary calls `trello-agent meta <url>` to resolve:
- Card number (e.g., `442`) — used as task name (`#442`)
- Title — used as entry description
- Board ID — looked up in config to find Clockify project ID

## Validators (automatic)

The binary validates BEFORE creating an entry:

1. **Overlap** — Fails if the new interval overlaps any existing entry on the same day.
   - Error: `Entry overlaps with existing entry <name> (HH:MM - HH:MM)`

2. **14h daily cap** — Fails if (existing day total + new entry) > 14h.
   - Error: `Total time for <date> would be X.Xh (exceeds 14h limit)`

3. **Project resolution** — Fails if no card URL and no `--project-id`.
   - Error: `--project-id is required when using --description without a card URL`

If `Could not fetch existing entries` warning appears, the entry is still created (overlap/cap checks skipped).

## Config

Location: `~/.config/clockify-agent/config.yaml`

Loaded in priority order (highest wins):
1. Env vars (`CLOCKIFY_API_KEY`, `CLOCKIFY_WORKSPACE_ID`, `CLOCKIFY_USER_ID`)
2. User TOML config
3. RMS Clockify `.env` (hardcoded path)
4. Trello `.env` (hardcoded path)

Required fields:
- `api_key` — Clockify API token
- `workspace_id` — Clockify workspace ID
- `user_id` (optional) — used for overlap/cap checks
- `[board_to_project]` — Trello board ID → Clockify project ID mapping

Example:
```toml
api_key = "xxx"
workspace_id = "yyy"
user_id = "zzz"

[board_to_project]
"5f0a1b2c3d4e5f6a7b8c9d0e" = "clockify-project-id-1"
```

**Add a new board → project mapping:**
1. Find board ID via `trello-agent meta <board-url>`
2. Find project ID via `clockify log --description "test" 1m --dry-run` (use any project ID, then check Clockify web UI for actual ID) OR via the Clockify web UI
3. Edit `~/.config/clockify-agent/config.yaml` to add the mapping under `[board_to_project]`
4. Test with `clockify log <card-url> 5m --dry-run`

## Environment

- **Timezone:** VNT (UTC+7) for all user-supplied times
- **API:** Direct `https://api.clockify.me/v1` calls, no third-party SDK

## Common tasks

### "Log 2 hours to this card"
```bash
clockify log "https://trello.com/c/ABC12345" 2h
```

### "Log from 9am to 11:30am"
```bash
clockify log "https://trello.com/c/ABC12345" 09:00 11:30
```

### "I started 30 min ago, worked 45 min"
```bash
clockify log "https://trello.com/c/ABC12345" "30m ago" 45m
```

### "Log on a different day (yesterday)"
```bash
clockify log "https://trello.com/c/ABC12345" 2h --date 2026-06-26
```

### "Log ad-hoc time (no card)"
```bash
clockify log --description "Code review for PR #123" 1h --project-id <project-id>
```

### "Dry run first"
```bash
clockify log "https://trello.com/c/ABC12345" 2h --dry-run
```

### "What did I log today?"
```bash
clockify summary today
```

### "How much time this week, by task?"
```bash
clockify summary this-week --by-task
```

### "Custom date range"
```bash
clockify summary --from 2026-06-01 --to 2026-06-15
```

## Output on success

```
Created time entry:
  Card:        442
  Start:       2026-06-27T02:00:00Z
  End:         2026-06-27T04:00:00Z
  Duration:    2:00:00
  Project:     <project-id>
  Task:        <task-id>
  Link:        https://app.clockify.me/workspaces/<ws>/time-entries/<id>

Project total today: <project-name> (4:30)
```

## Error handling

### "Command not found"
```bash
# Reinstall
cd /Users/loithai/Projects/ai-tools/clockify-agent && make install
```

### "failed to fetch card info"
Trello side issue. Try `trello-agent meta <url>` manually to debug.

### "no project ID found"
Add board→project mapping in `~/.config/clockify-agent/config.yaml`.

### "Entry overlaps with existing entry"
Pick a non-overlapping interval, or use `--date YYYY-MM-DD` for a different day.

### "exceeds 14h limit"
Cap is per day in VNT. If you really need to override, edit `src/validator.rs` (`MAX_DAILY_SECONDS`) and rebuild.

### "Clockify API error: 401"
API key invalid/expired. Update `CLOCKIFY_API_KEY` env var or `api_key` in config.

### "Clockify API error: 404"
Workspace or project ID wrong. Verify with Clockify web UI.

## Out of scope

This skill does NOT handle:
- Slack messaging
- Trello card browsing (only `trello-agent meta` is used internally for card resolution)
- Weekly status reports
- General PM workflows

For those, use other tools/skills.

## Build & test

```bash
cd /Users/loithai/Projects/ai-tools/clockify-agent
make install
cargo test
```

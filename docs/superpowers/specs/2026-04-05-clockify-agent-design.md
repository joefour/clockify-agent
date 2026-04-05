# Clockify AI Agent — Design Spec

## 1. Overview

A CLI tool for logging time entries to Clockify. Given a Trello card URL, it fetches card metadata (number, title, status, status-change timestamp), creates a properly formatted time entry, and validates for overlaps and daily hour limits.

**Stack:** Go (same as trello-agent)
**Auth:** Clockify API key via environment variable `CLOCKIFY_API_KEY`

---

## 2. CLI Interface

### Subcommands

All commands start with `clockify log <card-url>`.

```
# A) Duration only
clockify log <card-url> <duration>

# B) Start + End
clockify log <card-url> <start> <end>

# C) Start + Duration
clockify log <card-url> <start> <duration>
```

**Duration format:** `H:MM:SS` or `H:MM` (seconds optional)
**Time format:** `HH:MM` (24-hour)
**Date format:** `YYYY-MM-DD` (defaults to today)

### Global Flags

| Flag | Description | Default |
|------|-------------|---------|
| `--date` | Target date for entry | today |
| `--dry-run` | Print entry without creating | false |
| `--project-id` | Override Clockify project ID | (from config or prompt) |
| `--task-id` | Attach to specific task ID | none |

### Examples

```
# Log 2h 12m 5s on card
clockify log https://trello.com/c/ABC123 2:12:05

# Log 9am to 11:30am on card
clockify log https://trello.com/c/ABC123 09:00 11:30

# Log 2h starting at 2pm
clockify log https://trello.com/c/ABC123 14:00 2:00

# With date override
clockify log https://trello.com/c/ABC123 09:00 11:30 --date 2026-04-04
```

---

## 3. Trello Integration

### New `trello card` Command (trello-agent extension)

Add to trello-agent:
```
trello card <url>
```

**Output (JSON):**
```json
{
  "cardNumber": "1234",
  "title": "Fix login bug",
  "status": "In-progress",
  "statusChangedAt": "2026-04-05T10:30:00Z"
}
```

**Status** maps to Trello list name (e.g., "Ready for dev", "In-progress", "Testing", "Completed").

**Note:** The `statusChangedAt` field tracks when the card was moved into its current status.

---

## 4. Clockify Time Entry Creation

### Entry Fields

| Field | Value Source |
|-------|-------------|
| Project ID | Config or flagged override |
| Task ID | `--task-id` flag if provided |
| Name | Trello card number (e.g., "1234") |
| Description | Trello card title (e.g., "Fix login bug") |
| Time start | Calculated from inputs |
| Time end | Calculated from inputs |

### Duration Calculation

**Input A (duration):** end = start + duration
**Input B (start + end):** duration = end - start
**Input C (start + duration):** end = start + duration

---

## 5. Validation Rules

### 5.1 No Overlapping Entries

Before creating an entry, check all existing entries on the same date in Clockify. Reject if:
```
new_start < existing_end AND new_end > existing_start
```

**Error:** `Entry overlaps with existing entry "<name>" (<start> - <end>)`

### 5.2 Daily 14-Hour Cap

Sum all entries for the target date (including the new one). Reject if total > 14 hours.

**Error:** `Total time for 2026-04-05 would be 15h 30m (exceeds 14h limit)`

### 5.3 Future Time Prevention

Reject entries with start time in the future.

---

## 6. Config

Store in `~/.config/clockify-agent/config.yaml`:
```yaml
api_key: ""  # or set CLOCKIFY_API_KEY env var
workspace_id: ""
default_project_id: ""
trello_url: ""  # base URL for trello-agent calls
```

---

## 7. Error Handling

| Scenario | Behavior |
|----------|----------|
| Invalid card URL | Print error + usage |
| Trello fetch fails | Print error with details |
| Clockify API error | Print error + API message |
| Overlap detected | Reject with conflicting entry info |
| 14h limit exceeded | Reject with daily total |
| Dry run | Print what would be created, exit 0 |

---

## 8. Dependencies

- Go standard library (`net/http`, `encoding/json`, etc.)
- External: Trello card info via `trello card` command (trello-agent)
- External: Clockify REST API

---

## 9. File Structure

```
clockify-agent/
  cmd/
    root.go        # cobra root
    log.go         # "clockify log" subcommand
  internal/
    clockify/
      client.go    # Clockify API calls
      types.go     # API response types
    trello/
      fetcher.go   # calls trello card command
      types.go     # card info types
    validator/
      overlap.go   # overlap check
      daily.go     # 14h cap check
    config/
      config.go    # config loading
  docs/
    superpowers/
      specs/
        2026-04-05-clockify-agent-design.md
```

---

## 10. Out of Scope

- Interactive project picker (CLI flags only)
- Creating new Clockify projects/tasks
- Batch time entry creation
- Webhook / real-time updates

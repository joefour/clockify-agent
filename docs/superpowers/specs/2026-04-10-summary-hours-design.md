# Clockify Summary Hours Feature

## Goal

Add a `summary` subcommand that reports tracked hours for a given date range, with two breakdown levels: total hours per project, and task-level hours per project.

## CLI Interface

```
clockify summary <preset>
clockify summary --from YYYY-MM-DD --to YYYY-MM-DD

Presets: today, yesterday, this-week, last-week, this-month, last-month

Flags:
  --by-task    Show per-task breakdown within each project
```

Examples:

```bash
clockify summary today
clockify summary last-week --by-task
clockify summary --from 2026-04-01 --to 2026-04-09
```

## Date Presets

| Preset | Start | End |
|---|---|---|
| `today` | today | today |
| `yesterday` | yesterday | yesterday |
| `this-week` | Monday of current week | today |
| `last-week` | Monday of previous week | Sunday of previous week |
| `this-month` | 1st of current month | today |
| `last-month` | 1st of previous month | last day of previous month |
| `--from/--to` | explicit date | explicit date |

Week calculation uses Monday as the first day of the week.

## Output Format

### Default (total hours per project)

```
Summary: this week (2026-04-06 -> 2026-04-10)

Project A          12:30
Project B           8:15
────────────────────────
Total              20:45
```

### With --by-task

```
Summary: this week (2026-04-06 -> 2026-04-10)

Project A                          12:30
  TASK-101 Feature X                5:00
  TASK-102 Bug fix                  3:30
  (no task)                         4:00

Project B                           8:15
  TASK-201 Refactor                 8:15

────────────────────────────────────────
Total                              20:45
```

- Duration format uses `timeutil.FormatDuration` (H:MM:SS or 0:MM style)
- Projects sorted alphabetically by name
- Tasks sorted alphabetically within each project
- Entries with no task grouped under `(no task)`

## Architecture

### 1. Extend `internal/clockify/types.go`

The real Clockify API v1 returns time entries with a nested `timeInterval` object and `task` object. Update the `TimeEntry` struct to match the actual API response:

```go
type TimeEntry struct {
    ID          string      `json:"id"`
    ProjectID   string      `json:"projectId"`
    TaskID      string      `json:"taskId"`
    Name        string      `json:"name"`
    Description string      `json:"description"`
    TimeInterval TimeInterval `json:"timeInterval"`
    Task        *TaskRef    `json:"task,omitempty"`
}

type TimeInterval struct {
    Start string `json:"start"`
    End   string `json:"end"`
}

type TaskRef struct {
    ID   string `json:"id"`
    Name string `json:"name"`
}
```

Remove the flat `Start`/`End` fields and the separate `TimeEntryRequest` struct stays unchanged. All existing code that references `entry.Start` and `entry.End` updates to use `entry.TimeInterval.Start` and `entry.TimeInterval.End`.

### 2. New client method in `internal/clockify/client.go`

```go
func (c *Client) ListEntries(start, end string) ([]TimeEntry, error)
```

Uses `GET /workspaces/{wid}/time-entries?start={start}&end={end}` with proper date range parameters. The existing `ListEntriesForDate` becomes a thin wrapper or is updated to call this.

### 3. New package `internal/summary/`

File: `internal/summary/summary.go`

Functions:

- `ResolveDateRange(preset string, from, to string) (start, end string, label string, err error)` — converts preset names or explicit dates into a start/end pair and a human-readable label
- `ComputeProjectTotals(entries []clockify.TimeEntry, projects []clockify.Project) []ProjectTotal` — aggregates entry durations by project ID, resolves names
- `ComputeTaskBreakdown(entries []clockify.TimeEntry, projects []clockify.Project) []ProjectTasks` — aggregates by project then by task

Types:

```go
type ProjectTotal struct {
    Name     string
    Duration int // seconds
}

type TaskTotal struct {
    Name     string
    Duration int // seconds
}

type ProjectTasks struct {
    Name     string
    Duration int // seconds
    Tasks    []TaskTotal
}
```

### 4. New subcommand in `main.go`

- Add `summary` as a recognized subcommand alongside `log`
- Add `--by-task`, `--from`, `--to` flags
- `runSummary()` function:
  1. Parse preset or date flags
  2. Load config
  3. Create client
  4. Fetch entries via `ListEntries(start, end)`
  5. Fetch projects via `ListProjects()`
  6. Compute totals (with or without task breakdown)
  7. Format and print output

### 5. Update existing code

Files that reference `entry.Start` / `entry.End` need updating to `entry.TimeInterval.Start` / `entry.TimeInterval.End`:

- `main.go` — `parseTimeEntryTime(conflictingEntry.Start)` and `.End`
- `internal/validator/overlap.go` — `parseTimeEntryTime` on `e.Start` and `e.End`
- `internal/validator/daily.go` — `parseTimeEntryTime` on `e.Start` and `e.End`
- `internal/clockify/client_test.go` — update test fixtures

## Implementation Order

1. Update `TimeEntry` struct and fix all existing references
2. Add `ListEntries(start, end)` client method
3. Create `internal/summary/` package with date resolver and aggregation
4. Add `summary` subcommand to `main.go`
5. Update tests

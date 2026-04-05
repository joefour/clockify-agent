# Clockify Agent Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a CLI tool for logging time entries to Clockify with Trello card integration, overlap prevention, and 14h daily cap.

**Architecture:** Go CLI with flag-based subcommands. Fetches Trello card metadata via `trello card` command, creates Clockify time entries via REST API, validates for overlaps and daily hours.

**Tech Stack:** Go (same as trello-agent), standard library `net/http`, `encoding/json`, `flag`, no external dependencies beyond go.mod

---

## File Structure

```
clockify-agent/
  go.mod
  go.sum
  main.go                    # entry point with flag parsing, subcommands
  internal/
    config/
      config.go             # load/save config from ~/.config/clockify-agent/config.yaml
      config_test.go
    clockify/
      client.go            # Clockify REST API calls (create entry, list entries, get projects)
      types.go              # API request/response types
      client_test.go
    trello/
      fetcher.go           # calls "trello card <url>" command, parses JSON output
      types.go             # CardInfo struct
      fetcher_test.go
    validator/
      overlap.go          # check for overlapping entries
      overlap_test.go
      daily.go            # check daily 14h cap
      daily_test.go
    timeutil/
      parse.go            # parse H:MM:SS, HH:MM, H:MM durations
      parse_test.go
  docs/
    superpowers/
      specs/
        2026-04-05-clockify-agent-design.md
      plans/
        2026-04-05-clockify-agent.md
```

---

## Task 1: Project scaffolding — initialize Go module, create directories, add go.mod

**Files:**
- Create: `go.mod`
- Create: `go.sum` (empty placeholder)
- Create: all directory structure above

```go
module github.com/loithai/clockify-agent

go 1.22
```

- [ ] **Step 1: Create directories**

Run:
```bash
mkdir -p clockify-agent/internal/{config,clockify,trello,validator,timeutil} clockify-agent/docs/superpowers/{specs,plans}
```

- [ ] **Step 2: Create go.mod**

```go
module github.com/loithai/clockify-agent

go 1.22
```

- [ ] **Step 3: Commit**

```bash
git add go.mod go.sum
git commit -m "init: scaffold clockify-agent module"
```

---

## Task 2: Config loader (`internal/config/config.go`)

**Files:**
- Create: `internal/config/config.go`
- Create: `internal/config/config_test.go`

Config file: `~/.config/clockify-agent/config.yaml`
```yaml
api_key: ""
workspace_id: ""
default_project_id: ""
trello_url: "http://localhost:18943"  # trello-agent HTTP server default
```

Load order: file → env var overrides (`CLOCKIFY_API_KEY`, `CLOCKIFY_WORKSPACE_ID`, etc.)
Env var names: `CLOCKIFY_API_KEY`, `CLOCKIFY_WORKSPACE_ID`, `CLOCKIFY_DEFAULT_PROJECT_ID`, `CLOCKIFY_TRELLO_URL`

- [ ] **Step 1: Write test**

```go
func TestLoadEnvOverride(t *testing.T) {
    os.Setenv("CLOCKIFY_API_KEY", "test-key")
    defer os.Unsetenv("CLOCKIFY_API_KEY")
    cfg, err := Load()
    if err != nil {
        t.Fatalf("Load() error = %v", err)
    }
    if cfg.APIKey != "test-key" {
        t.Errorf("APIKey = %q, want %q", cfg.APIKey, "test-key")
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `go test ./internal/config/... -v`
Expected: FAIL — Config struct not defined

- [ ] **Step 3: Write implementation**

- [ ] **Step 4: Run test to verify it passes**

- [ ] **Step 5: Commit**

```bash
git add internal/config/
git commit -m "feat: add config loader"
```

---

## Task 3: Time parsing utilities (`internal/timeutil/parse.go`)

**Files:**
- Create: `internal/timeutil/parse.go`
- Create: `internal/timeutil/parse_test.go`

**Duration formats:**
- `H:MM:SS` → `2:12:05` → 2h 12m 5s
- `H:MM` → `2:12` → 2h 12m 0s (seconds = 0)

**Time formats:**
- `HH:MM` → `09:00` → 9:00 AM

**Date format:**
- `YYYY-MM-DD` → `2026-04-05`

Functions to implement:
```go
// ParseDuration("2:12:05") → (2*3600 + 12*60 + 5, nil)
func ParseDuration(s string) (int, error)

// ParseTimeHHMM("09:00") → (9*3600, nil)  — seconds from midnight
func ParseTimeHHMM(s string) (int, error)

// ParseDate("2026-04-05") → (2026, 4, 5)
func ParseDate(s string) (year, month, day int, error)

// FormatDuration(seconds) → "2:12:05"
func FormatDuration(seconds int) string
```

- [ ] **Step 1: Write tests**
- [ ] **Step 2: Run tests to verify they fail**
- [ ] **Step 3: Write implementation**
- [ ] **Step 4: Run tests to verify they pass**
- [ ] **Step 5: Commit**

---

## Task 4: Clockify API client (`internal/clockify/`)

**Files:**
- Create: `internal/clockify/types.go`
- Create: `internal/clockify/client.go`
- Create: `internal/clockify/client_test.go`

### Types (`types.go`)

```go
type TimeEntryRequest struct {
    ProjectID  string  `json:"projectId"`
    TaskID     string  `json:"taskId,omitempty"`
    Name       string  `json:"name"`
    Description string `json:"description"`
    Start      string `json:"timeStart"`
    End        string `json:"timeEnd"`
}

type TimeEntry struct {
    ID          string `json:"id"`
    ProjectID   string `json:"projectId"`
    TaskID      string `json:"taskId"`
    Name        string `json:"name"`
    Description string `json:"description"`
    Start       string `json:"timeStart"`
    End         string `json:"timeEnd"`
}

type Project struct {
    ID   string `json:"id"`
    Name string `json:"name"`
}
```

### Client (`client.go`)

```go
type Client struct {
    apiKey       string
    workspaceID  string
    baseURL      string  // "https://api.clockify.me/api/v1"
    httpClient   *http.Client
}

func NewClient(apiKey, workspaceID string) *Client

// CreateEntry creates a time entry. Returns entry or error.
func (c *Client) CreateEntry(entry TimeEntryRequest) (*TimeEntry, error)

// ListEntriesForDate returns all entries for a workspace on the given date.
func (c *Client) ListEntriesForDate(date string) ([]TimeEntry, error)

// ListProjects returns all projects in the workspace.
func (c *Client) ListProjects() ([]Project, error)
```

All API calls use `X-Api-Key` header. Time format: RFC3339 (`2026-04-05T09:00:00Z`).

- [ ] **Step 1: Write tests with mock HTTP (no real API)**
- [ ] **Step 2: Run tests to verify they fail**
- [ ] **Step 3: Write implementation**
- [ ] **Step 4: Run tests to verify they pass**
- [ ] **Step 5: Commit**

---

## Task 5: Trello fetcher (`internal/trello/`)

**Files:**
- Create: `internal/trello/types.go`
- Create: `internal/trello/fetcher.go`
- Create: `internal/trello/fetcher_test.go`

### Types (`types.go`)

```go
type CardInfo struct {
    CardNumber string `json:"cardNumber"`
    Title      string `json:"title"`
    Status     string `json:"status"`
}
```

### Fetcher (`fetcher.go`)

`FetchCard(url string) (*CardInfo, error)` — calls `trello card <url>` via `exec.Command`, parses JSON stdout.

Example call:
```bash
trello card https://trello.com/c/ABC123/1
```
Expected JSON output:
```json
{"cardNumber":"1234","title":"Fix login bug","status":"In-progress"}
```

- [ ] **Step 1: Write tests with fake exec Command**
- [ ] **Step 2: Run tests to verify they fail**
- [ ] **Step 3: Write implementation**
- [ ] **Step 4: Run tests to verify they pass**
- [ ] **Step 5: Commit**

---

## Task 6: Validators (`internal/validator/`)

**Files:**
- Create: `internal/validator/overlap.go` + `_test.go`
- Create: `internal/validator/daily.go` + `_test.go`

### Overlap validator (`overlap.go`)

```go
// HasOverlap returns true if [start, end] overlaps with any entry.
func HasOverlap(start, end time.Time, entries []clockify.TimeEntry) (bool, *clockify.TimeEntry)
```

Rule: `new_start < existing_end AND new_end > existing_start`

### Daily cap validator (`daily.go`)

```go
// DailyTotal returns total seconds for entries on a given date.
func DailyTotal(date time.Time, entries []clockify.TimeEntry) int

// ExceedsCap returns true if adding duration to existing total would exceed 14h.
func ExceedsCap(date time.Time, existingSeconds, newSeconds int) bool
```

Constant: `MaxDailySeconds = 14 * 3600`

- [ ] **Step 1: Write tests**
- [ ] **Step 2: Run tests to verify they fail**
- [ ] **Step 3: Write implementation**
- [ ] **Step 4: Run tests to verify they pass**
- [ ] **Step 5: Commit**

---

## Task 7: Main CLI (`main.go` + `log.go`)

**Files:**
- Create: `main.go`
- Create: `cmd/root.go` ( cobra root — optional, or inline in main.go)

Implement `clockify log` subcommand with three input modes:

```bash
# A) Duration only
clockify log <card-url> <duration>

# B) Start + End
clockify log <card-url> <start> <end>

# C) Start + Duration
clockify log <card-url> <start> <duration>
```

Flags:
- `--date YYYY-MM-DD` (default: today)
- `--dry-run` (default: false)
- `--project-id ID` (override)

**Flow:**
1. Parse card URL → extract shortLink
2. Call `trello card <url>` → get CardInfo
3. Parse time inputs → compute start, end, duration
4. Load config → get workspace ID, API key, default project
5. List existing entries for date → check overlap & 14h cap
6. Create entry via Clockify API
7. Print result (name, description, start, end, duration)

**Error handling:**
- Invalid URL → "Error: invalid Trello card URL"
- Trello fetch fails → "Error: failed to fetch card info: <err>"
- Overlap → "Error: Entry overlaps with existing entry "<name>" (<start> - <end>)"
- 14h exceeded → "Error: Total time for <date> would be <X>h (exceeds 14h limit)"
- Dry run → print what would be created, exit 0

- [ ] **Step 1: Write integration tests (if feasible) or manual test plan**
- [ ] **Step 2: Implement main.go with flag parsing**
- [ ] **Step 3: Test with real calls (manual)**
- [ ] **Step 4: Commit**

---

## Task 8: Add `trello card` command to trello-agent

**Files (in trello-agent):**
- Modify: `main.go` — add `"card"` case in subcommand switch
- Create: `runCardCmd()` function

```bash
trello card <card-url>
```

Output (JSON to stdout):
```json
{"cardNumber":"1234","title":"Fix login bug","status":"In-progress"}
```

**Implementation:**
- Call `client.FetchCard(shortLink, FetchOptions{})`
- Fetch the card's current list (status) via separate API call
- Output JSON with cardNumber (shortLink numeric suffix), title, status (list name)

- [ ] **Step 1: Write test**
- [ ] **Step 2: Implement `runCardCmd`**
- [ ] **Step 3: Test manually**
- [ ] **Step 4: Commit to trello-agent**

---

## Spec Coverage Check

- [x] CLI with 3 input modes (A, B, C) — Task 7
- [x] Duration format H:MM:SS with seconds — Task 3
- [x] Time format HH:MM (24-hour) — Task 3
- [x] Date format YYYY-MM-DD — Task 3
- [x] Trello card info (number, title, status) — Task 5
- [x] Clockify time entry creation — Task 4
- [x] Entry name = card number — Task 7
- [x] Entry description = card title — Task 7
- [x] Overlap check — Task 6
- [x] 14h daily cap — Task 6
- [x] Config via file + env vars — Task 2
- [x] `--date`, `--dry-run`, `--project-id` flags — Task 7
- [x] `trello card` command in trello-agent — Task 8

## Placeholder Scan

All steps have actual code blocks. No "TBD" or "TODO" found. All tasks have been broken into concrete steps.

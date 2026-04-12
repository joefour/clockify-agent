# Clockify Log Without Ticket — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow `clockify log` to create time entries without a Trello card URL, using `--description` flag instead.

**Architecture:** Add `--description` flag to log command. When no card URL is provided, skip Trello fetch and task creation. Description becomes the entry name. Project ID still required.

**Tech Stack:** Go, flag standard library, existing trello/config/clockify packages

---

## File Structure

**Modify:** `main.go` (main CLI logic, runLog function)

**No new files required.**

---

## Task 1: Add `--description` Flag

**Files:**
- Modify: `main.go:53-64` (log flag parsing section)

- [ ] **Step 1: Add flag variable**

```go
flagDescription := fs.String("description", "", "Description for time entry (use instead of card URL)")
```

Add this after `flagTaskID := fs.String("task-id", "", "Task ID for the entry")` (line 57).

- [ ] **Step 2: Pass description to runLog**

Change line 61 from:
```go
if err := runLog(fs.Args(), *flagDate, *flagDryRun, *flagProjectID, *flagTaskID); err != nil {
```
To:
```go
if err := runLog(fs.Args(), *flagDate, *flagDryRun, *flagProjectID, *flagTaskID, *flagDescription); err != nil {
```

- [ ] **Step 3: Commit**

```bash
git add main.go
git commit -m "feat: add --description flag to log command"
```

---

## Task 2: Update runLog Signature and Validate Inputs

**Files:**
- Modify: `main.go:94-163` (runLog function signature and early validation)

- [ ] **Step 1: Update function signature**

Change line 94 from:
```go
func runLog(args []string, dateStr string, dryRun bool, projectIDOverride, taskID string) error {
```
To:
```go
func runLog(args []string, dateStr string, dryRun bool, projectIDOverride, taskID, description string) error {
```

- [ ] **Step 2: Update argument count check**

Change lines 95-97 from:
```go
if len(args) < 2 {
    return errors.New("invalid arguments: expected <card-url> <duration> or <card-url> <start> <end>")
}
```
To:
```go
// Determine if we have a card URL or using description-only mode
hasCardURL := len(args) >= 1 && strings.TrimSpace(args[0]) != ""
needsDescription := !hasCardURL

if len(args) < 1 && description == "" {
    return errors.New("invalid arguments: provide a card URL or --description")
}
if len(args) < 2 && description == "" {
    return errors.New("invalid arguments: expected <duration> or <start> <end>")
}
```

- [ ] **Step 3: Update cardURL assignment**

Change line 99 from:
```go
cardURL := args[0]
```
To:
```go
cardURL := ""
if len(args) >= 1 {
    cardURL = args[0]
}
```

- [ ] **Step 4: Commit**

```bash
git add main.go
git commit -m "feat: update runLog to accept description and handle no-card mode"
```

---

## Task 3: Skip Trello Fetch When No Card URL

**Files:**
- Modify: `main.go:159-163` (card fetch section)

- [ ] **Step 1: Conditionally fetch card**

Change lines 159-163 from:
```go
// Fetch card info
cardInfo, err := trello.FetchCard(cardURL)
if err != nil {
    return fmt.Errorf("failed to fetch card info: %w", err)
}
```
To:
```go
// Fetch card info only when card URL is provided
var cardInfo *trello.CardInfo
if cardURL != "" {
    cardInfo, err = trello.FetchCard(cardURL)
    if err != nil {
        return fmt.Errorf("failed to fetch card info: %w", err)
    }
} else {
    // No card URL — create a placeholder CardInfo with description
    cardInfo = &trello.CardInfo{
        CardNumber: "",
        Title:      description,
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add main.go
git commit -m "feat: skip trello fetch when no card URL provided"
```

---

## Task 4: Skip Task Resolution When No Card URL

**Files:**
- Modify: `main.go:210-234` (task resolution section)

- [ ] **Step 1: Wrap task resolution in card URL check**

Change lines 210-234 from:
```go
// Resolve task: if not provided, find or create task with name = card number (with # prefix)
resolvedTaskID := taskID
taskName := "#" + cardInfo.CardNumber
if resolvedTaskID == "" {
    tasks, err := client.ListTasks(projectID)
    if err != nil {
        return fmt.Errorf("failed to list tasks: %w", err)
    }
    for _, t := range tasks {
        if t.Name == taskName {
            resolvedTaskID = t.ID
            break
        }
    }
    if resolvedTaskID == "" {
        newTask, err := client.CreateTask(projectID, taskName)
        if err != nil {
            return fmt.Errorf("failed to create task: %w", err)
        }
        resolvedTaskID = newTask.ID
    }
}
```
To:
```go
// Resolve task: only when card URL is provided (task is tied to card number)
resolvedTaskID := taskID
if cardURL != "" && resolvedTaskID == "" {
    taskName := "#" + cardInfo.CardNumber
    tasks, err := client.ListTasks(projectID)
    if err != nil {
        return fmt.Errorf("failed to list tasks: %w", err)
    }
    for _, t := range tasks {
        if t.Name == taskName {
            resolvedTaskID = t.ID
            break
        }
    }
    if resolvedTaskID == "" {
        newTask, err := client.CreateTask(projectID, taskName)
        if err != nil {
            return fmt.Errorf("failed to create task: %w", err)
        }
        resolvedTaskID = newTask.ID
    }
}
// When no card URL, taskID stays empty (no task association)
```

- [ ] **Step 2: Commit**

```bash
git add main.go
git commit -m "feat: skip task resolution when no card URL"
```

---

## Task 5: Handle Project Resolution for No-Card Mode

**Files:**
- Modify: `main.go:201-208` (project resolution section)

- [ ] **Step 1: Update project resolution for description-only mode**

Change lines 201-208 from:
```go
// Determine project ID via board mapping
projectID := projectIDOverride
if projectID == "" {
    projectID = cfg.GetClockifyProjectID(cardInfo.BoardID, "")
}
if projectID == "" {
    return errors.New("no project ID found (configure board-to-project mapping in config)")
}
```
To:
```go
// Determine project ID
projectID := projectIDOverride
if projectID == "" && cardURL != "" {
    // Use board mapping when card URL is provided
    projectID = cfg.GetClockifyProjectID(cardInfo.BoardID, "")
}
if projectID == "" && description == "" {
    return errors.New("no project ID found (configure board-to-project mapping in config or use --project-id)")
}
if projectID == "" {
    return errors.New("--project-id is required when using --description without a card URL")
}
```

- [ ] **Step 2: Commit**

```bash
git add main.go
git commit -m "feat: require --project-id when using --description"
```

---

## Task 6: Update Dry Run Output

**Files:**
- Modify: `main.go:261-271` (dry run output section)

- [ ] **Step 1: Handle dry run for no-card entries**

Change lines 261-271 from:
```go
// Dry run
if dryRun {
    fmt.Println("Would create time entry:")
    fmt.Printf("  Card:        %s\n", cardInfo.CardNumber)
    fmt.Printf("  Description: %s\n", cardInfo.Title)
    fmt.Printf("  Start:       %s\n", startTime.Format(time.RFC3339))
    fmt.Printf("  End:         %s\n", endTime.Format(time.RFC3339))
    fmt.Printf("  Duration:    %s\n", timeutil.FormatDuration(durationSeconds))
    fmt.Printf("  Project:     %s\n", projectID)
    return nil
}
```
To:
```go
// Dry run
if dryRun {
    fmt.Println("Would create time entry:")
    if cardInfo.CardNumber != "" {
        fmt.Printf("  Card:        %s\n", cardInfo.CardNumber)
    } else {
        fmt.Printf("  Card:        (no ticket)\n")
    }
    fmt.Printf("  Description: %s\n", cardInfo.Title)
    fmt.Printf("  Start:       %s\n", startTime.Format(time.RFC3339))
    fmt.Printf("  End:         %s\n", endTime.Format(time.RFC3339))
    fmt.Printf("  Duration:    %s\n", timeutil.FormatDuration(durationSeconds))
    fmt.Printf("  Project:     %s\n", projectID)
    if resolvedTaskID != "" {
        fmt.Printf("  Task:        %s\n", resolvedTaskID)
    }
    return nil
}
```

- [ ] **Step 2: Commit**

```bash
git add main.go
git commit -m "feat: update dry run output for no-card entries"
```

---

## Task 7: Update Time Entry Request for No-Card Mode

**Files:**
- Modify: `main.go:273-296` (entry creation and success output)

- [ ] **Step 1: Update entry request to handle no-card name**

Change lines 273-282 from:
```go
// Create entry
entryReq := clockify.TimeEntryRequest{
    ProjectID:   projectID,
    TaskID:      resolvedTaskID,
    Name:        cardInfo.CardNumber,
    Description: cardInfo.Title,
    Start:       startTime.Format(time.RFC3339),
    End:         endTime.Format(time.RFC3339),
    Billable:    true,
}
```
To:
```go
// Create entry
entryName := cardInfo.CardNumber
if entryName == "" {
    // When no card, use first part of description (truncated to reasonable length)
    entryName = description
    if len(entryName) > 50 {
        entryName = entryName[:50]
    }
}
entryReq := clockify.TimeEntryRequest{
    ProjectID:   projectID,
    TaskID:      resolvedTaskID,
    Name:        entryName,
    Description: cardInfo.Title,
    Start:       startTime.Format(time.RFC3339),
    End:         endTime.Format(time.RFC3339),
    Billable:    true,
}
```

- [ ] **Step 2: Update success output**

Change lines 289-295 from:
```go
fmt.Println("Created time entry:")
fmt.Printf("  Card:        %s\n", result.Name)
fmt.Printf("  Description: %s\n", result.Description)
fmt.Printf("  Start:       %s\n", result.TimeInterval.Start)
fmt.Printf("  End:         %s\n", result.TimeInterval.End)
fmt.Printf("  Duration:    %s\n", timeutil.FormatDuration(int(endTime.Sub(startTime).Seconds())))
fmt.Printf("  Project:     %s\n", result.ProjectID)
```
To:
```go
fmt.Println("Created time entry:")
if cardInfo.CardNumber != "" {
    fmt.Printf("  Card:        %s\n", result.Name)
} else {
    fmt.Printf("  Description: %s\n", result.Description)
}
fmt.Printf("  Start:       %s\n", result.TimeInterval.Start)
fmt.Printf("  End:         %s\n", result.TimeInterval.End)
fmt.Printf("  Duration:    %s\n", timeutil.FormatDuration(int(endTime.Sub(startTime).Seconds())))
fmt.Printf("  Project:     %s\n", result.ProjectID)
if result.TaskID != "" {
    fmt.Printf("  Task:        %s\n", result.TaskID)
}
```

- [ ] **Step 3: Commit**

```bash
git add main.go
git commit -m "feat: handle no-card entries in time entry creation"
```

---

## Task 8: Update Usage Text

**Files:**
- Modify: `main.go:20-41` (usage function)

- [ ] **Step 1: Update usage to show new syntax**

Change lines 20-40 from:
```go
func usage() {
	fmt.Println(`Usage:
  clockify log <card-url> <duration>
  clockify log <card-url> <start> <end>
  clockify log <card-url> <start> <duration>

  clockify summary <preset>
  clockify summary --from YYYY-MM-DD --to YYYY-MM-DD

Presets: today, yesterday, this-week, last-week, this-month, last-month

Log flags:
  --date YYYY-MM-DD    Date for the entry (default: today)
  --dry-run            Print entry details without creating
  --project-id ID      Override default project ID
  --task-id ID         Task ID for the entry

Summary flags:
  --by-task            Show per-task breakdown within each project
  --from YYYY-MM-DD    Start date (required if no preset)
  --to YYYY-MM-DD      End date (required if no preset)`)
}
```
To:
```go
func usage() {
	fmt.Println(`Usage:
  clockify log [card-url] <duration>
  clockify log [card-url] <start> <end>
  clockify log [card-url] <start> <duration>
  clockify log --description "text" <duration>
  clockify log --description "text" <start> <duration>

  clockify summary <preset>
  clockify summary --from YYYY-MM-DD --to YYYY-MM-DD

Presets: today, yesterday, this-week, last-week, this-month, last-month

Log flags:
  --date YYYY-MM-DD    Date for the entry (default: today)
  --dry-run            Print entry details without creating
  --project-id ID      Override default project ID (required with --description)
  --task-id ID         Task ID for the entry
  --description TEXT   Description for entry (use instead of card URL)

Summary flags:
  --by-task            Show per-task breakdown within each project
  --from YYYY-MM-DD    Start date (required if no preset)
  --to YYYY-MM-DD      End date (required if no preset)`)
}
```

- [ ] **Step 2: Commit**

```bash
git add main.go
git commit -m "docs: update usage text for --description flag"
```

---

## Task 9: Test No-Card Logging

- [ ] **Step 1: Build and test dry run**

```bash
go build -o /tmp/clockify-agent && /tmp/clockify-agent log --dry-run --project-id 6913a7a64d8b0c5a9cca51d4 --description "review and build apps" 45m --date 2026-04-13
```

Expected output:
```
Would create time entry:
  Card:        (no ticket)
  Description: review and build apps
  Start:       ...
  End:         ...
  Duration:    0:45:54
  Project:     6913a7a64d8b0c5a9cca51d4
```

- [ ] **Step 2: Test with time range**

```bash
/tmp/clockify-agent log --dry-run --project-id 6913a7a64d8b0c5a9cca51d4 --description "review and build apps" 09:00 12:00 --date 2026-04-13
```

Expected: Duration should be ~3h with random adjustment.

- [ ] **Step 3: Test error when --project-id missing**

```bash
/tmp/clockify-agent log --dry-run --description "review and build apps" 45m --date 2026-04-13
```

Expected: Error message about --project-id being required.

---

## Task 10: Test Card URL Mode (Regression)

- [ ] **Step 1: Test existing card URL behavior still works**

```bash
/tmp/clockify-agent log --dry-run --project-id 6913a7a64d8b0c5a9cca51d4 no-ticket 45m --date 2026-04-13
```

Expected: Works with "no-ticket" as placeholder card URL (fetch will fail but entry can still be created).

- [ ] **Step 2: Verify card URL with description override**

If you have a real Trello URL, test that --description still works alongside it.

---

## Self-Review Checklist

1. **Spec coverage:** All spec requirements covered by tasks above.
2. **Placeholder scan:** No TBD/TODO in any step. All code is complete.
3. **Type consistency:** Function signatures updated consistently across all call sites. `description` parameter passed through correctly.
4. **Error handling:** Proper error when --project-id missing with --description.
5. **Backward compatibility:** Existing card URL mode unchanged.

---

## Execution Options

Plan complete and saved to `docs/superpowers/plans/2026-04-13-clockify-no-ticket-plan.md`. Two execution options:

**1. Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints

Which approach?
# Clockify Log Without Ticket — Design Spec

## Context

Currently `clockify log` requires a Trello card URL as the first argument. This prevents logging time when no ticket exists but a project and description are known.

## Goal

Allow logging time with just a project ID and description text — no Trello URL required.

## Proposal

Add `--description` flag to the `log` command. When used without a card URL:

1. Skip Trello card fetch entirely
2. Use description as the time entry name
3. No task is created
4. Project ID must still be provided (via `--project-id` or config mapping)

## CLI Change

**Before:**
```
clockify log <card-url> <duration>
```

**After:**
```
# With card (existing)
clockify log <card-url> <duration>
clockify log <card-url> <start> <end>

# Without card (new)
clockify log --description "review and build apps" 45m
clockify log --description "review and build apps" 09:00 12:00
clockify log --project-id 6913a7a64d8b0c5a9cca51d4 --description "review and build apps" 45m
```

## Flag: `--description`

- **Type:** string
- **Required:** when no card URL provided
- **Used as:** time entry name and description
- **Example:** `--description "review and build apps"`

## Behavior Matrix

| Card URL | `--description` | Result |
|----------|-----------------|--------|
| Provided | Empty | Fetch card, create task, use card title as description |
| Provided | Set | Fetch card, create task, use --description instead of card title |
| Absent | Set | No card fetch, no task, use --description as name |
| Absent | Empty | Error: need either card URL or --description |

## Implementation

### main.go changes

1. Add `--description` flag to log flag set
2. Detect "no-card" mode when first arg is empty/omitted
3. Skip trello.FetchCard when in no-card mode
4. Build TimeEntryRequest with description as name

### runLog changes

```go
func runLog(args []string, dateStr string, dryRun bool, projectIDOverride, taskID string, description string) error {
    // If no description from flag and no card URL → error
    // If no card URL but has description → skip trello fetch
    // If card URL provided → fetch card (description may override)
}
```

### Validation

- At least one of card URL or `--description` must be provided
- Project ID must be resolvable (from flag, config mapping, or default)
- Duration/start-end still required

## No Other Changes

- Overlap validation still applies
- Daily 14h cap still applies
- Random 1-5% duration adjustment still applies
- All existing modes work unchanged
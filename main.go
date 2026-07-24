package main

import (
	"errors"
	"flag"
	"fmt"
	"math/rand"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/loithai/clockify-agent/internal/clockify"
	"github.com/loithai/clockify-agent/internal/config"
	"github.com/loithai/clockify-agent/internal/summary"
	"github.com/loithai/clockify-agent/internal/timeutil"
	"github.com/loithai/clockify-agent/internal/trello"
	"github.com/loithai/clockify-agent/internal/validator"
)

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

func main() {
	if len(os.Args) < 2 {
		usage()
		os.Exit(1)
	}

	subcmd := os.Args[1]

	switch subcmd {
	case "log":
		fs := flag.NewFlagSet("log", flag.ExitOnError)
		flagDate := fs.String("date", "", "Date in YYYY-MM-DD format (default: today)")
		flagDryRun := fs.Bool("dry-run", false, "Print entry details without creating")
		flagProjectID := fs.String("project-id", "", "Override default project ID")
		flagTaskID := fs.String("task-id", "", "Task ID for the entry")
		flagDescription := fs.String("description", "", "Description for time entry (use instead of card URL)")
		fs.Usage = func() { usage() }
		fsArgs, err := extractFlags(os.Args[2:], fs)
		if err != nil {
			fmt.Fprintf(os.Stderr, "Error: %s\n", err.Error())
			os.Exit(1)
		}

		if err := runLog(fsArgs, *flagDate, *flagDryRun, *flagProjectID, *flagTaskID, *flagDescription); err != nil {
			fmt.Fprintf(os.Stderr, "Error: %s\n", err.Error())
			os.Exit(1)
		}

	case "summary":
		fs := flag.NewFlagSet("summary", flag.ExitOnError)
		flagByTask := fs.Bool("by-task", false, "Show per-task breakdown within each project")
		flagFrom := fs.String("from", "", "Start date (YYYY-MM-DD)")
		flagTo := fs.String("to", "", "End date (YYYY-MM-DD)")
		fs.Usage = func() { usage() }
		fsArgs, err := extractFlags(os.Args[2:], fs)
		if err != nil {
			fmt.Fprintf(os.Stderr, "Error: %s\n", err.Error())
			os.Exit(1)
		}

		preset := ""
		if len(fsArgs) > 0 {
			preset = fsArgs[0]
		}

		if err := runSummary(preset, *flagFrom, *flagTo, *flagByTask); err != nil {
			fmt.Fprintf(os.Stderr, "Error: %s\n", err.Error())
			os.Exit(1)
		}

	case "-h", "--help":
		usage()

	default:
		fmt.Fprintf(os.Stderr, "Error: unknown subcommand %q\n", subcmd)
		usage()
		os.Exit(1)
	}
}

// extractFlags walks args, separating flag tokens (anywhere in the list) from
// positional tokens, and feeds only the flag tokens to fs.Parse. This lets
// users write `clockify log <url> 2h --date 2026-06-27` (flags after args),
// which Go's standard flag library does not support on its own.
func extractFlags(args []string, fs *flag.FlagSet) ([]string, error) {
	flagArgs := make([]string, 0, len(args))
	positional := make([]string, 0, len(args))

	boolFlags := map[string]bool{
		"dry-run": true,
		"by-task": true,
	}

	stringFlags := map[string]bool{
		"date":        true,
		"project-id":  true,
		"task-id":     true,
		"description": true,
		"from":        true,
		"to":          true,
	}

	i := 0
	for i < len(args) {
		arg := args[i]

		if arg == "--" {
			positional = append(positional, args[i+1:]...)
			break
		}

		var name string
		var rest string
		var hadValue bool
		switch {
		case strings.HasPrefix(arg, "--"):
			name = strings.TrimPrefix(arg, "--")
		case strings.HasPrefix(arg, "-"):
			name = strings.TrimPrefix(arg, "-")
		default:
			positional = append(positional, arg)
			i++
			continue
		}

		if eq := strings.Index(name, "="); eq >= 0 {
			rest = name[eq+1:]
			name = name[:eq]
			hadValue = true
		}

		if !boolFlags[name] && !stringFlags[name] {
			positional = append(positional, arg)
			i++
			continue
		}

		if boolFlags[name] {
			flagArgs = append(flagArgs, "--"+name)
			if rest != "" {
				flagArgs = append(flagArgs, rest)
			} else if hadValue {
				// `-flag=` form, no extra token to consume
			} else {
				// `-flag` form: for bool, no value follows
			}
			i++
			continue
		}

		// string flag
		if rest != "" {
			flagArgs = append(flagArgs, "--"+name+"="+rest)
			i++
			continue
		}
		if i+1 >= len(args) {
			return nil, fmt.Errorf("flag --%s requires a value", name)
		}
		next := args[i+1]
		if strings.HasPrefix(next, "-") && len(next) > 1 && !looksLikeDuration(next) {
			return nil, fmt.Errorf("flag --%s requires a value", name)
		}
		flagArgs = append(flagArgs, "--"+name+"="+next)
		i += 2
	}

	if err := fs.Parse(flagArgs); err != nil {
		return nil, err
	}
	return positional, nil
}

// looksLikeDuration returns true for tokens like "08:00", "2h", "1h30m".
// Used to disambiguate `-date -08:00` (not valid) from a missing value.
func looksLikeDuration(s string) bool {
	if strings.Contains(s, ":") {
		return true
	}
	if strings.Contains(s, "h") || strings.Contains(s, "m") {
		return true
	}
	if _, err := strconv.Atoi(s); err == nil {
		return true
	}
	return false
}

func runLog(args []string, dateStr string, dryRun bool, projectIDOverride, taskID, description string) error {
	// Determine card URL first (only relevant when no description)
	cardURL := ""
	if description == "" && len(args) >= 1 {
		cardURL = args[0]
	}

	// Validate arguments
	if len(args) < 1 && description == "" {
		return errors.New("invalid arguments: provide a card URL or --description")
	}
	if description == "" && cardURL != "" && len(args) < 2 {
		return errors.New("invalid arguments: expected <duration> or <start> <end>")
	}

	// Detect mode by arg count
	var startStr, endStr, durationStr string
	var durationSeconds int
	var relativeStartSeconds int // seconds ago for relative start time (e.g., "4h ago" = 4*3600)
	var err error

	if cardURL != "" {
		// Normal mode: args[0] is cardURL
		if len(args) == 2 {
			// Mode A: cardURL + duration
			durationStr = args[1]
			durationSeconds, err = timeutil.ParseDuration(durationStr)
			if err != nil {
				return fmt.Errorf("invalid duration format: %w", err)
			}
		} else if len(args) == 3 {
			// Check if args[1] is a relative time like "4h ago"
			if relSecs, relErr := timeutil.ParseRelativeTime(args[1]); relErr == nil {
				// Relative start time + duration: "4h ago" + "30m"
				relativeStartSeconds = relSecs
				durationStr = args[2]
				durationSeconds, err = timeutil.ParseDuration(durationStr)
				if err != nil {
					return fmt.Errorf("invalid duration format: %w", err)
				}
			} else if _, err = timeutil.ParseTimeHHMM(args[1]); err == nil {
				if _, err = timeutil.ParseTimeHHMM(args[2]); err == nil {
					// Mode B: start + end
					startStr = args[1]
					endStr = args[2]
				} else {
					// Mode C: start + duration
					startStr = args[1]
					durationStr = args[2]
					durationSeconds, err = timeutil.ParseDuration(durationStr)
					if err != nil {
						return fmt.Errorf("invalid duration format: %w", err)
					}
				}
			} else {
				// Mode C: start + duration
				startStr = args[1]
				durationStr = args[2]
				durationSeconds, err = timeutil.ParseDuration(durationStr)
				if err != nil {
					return fmt.Errorf("invalid duration format: %w", err)
				}
			}
		} else {
			return errors.New("invalid arguments")
		}
	} else {
		// No-card mode: args[0] is duration or start time
		if len(args) == 1 {
			// Mode A: duration only
			durationStr = args[0]
			durationSeconds, err = timeutil.ParseDuration(durationStr)
			if err != nil {
				return fmt.Errorf("invalid duration format: %w", err)
			}
		} else if len(args) == 2 {
			if relSecs, relErr := timeutil.ParseRelativeTime(args[0]); relErr == nil {
				// Relative start time + duration: "4h ago" + "30m"
				relativeStartSeconds = relSecs
				durationStr = args[1]
				durationSeconds, err = timeutil.ParseDuration(durationStr)
				if err != nil {
					return fmt.Errorf("invalid duration format: %w", err)
				}
			} else if _, err = timeutil.ParseTimeHHMM(args[0]); err == nil {
				// args[0] is start time
				if _, err = timeutil.ParseTimeHHMM(args[1]); err == nil {
					// Mode B: start + end
					startStr = args[0]
					endStr = args[1]
				} else {
					// Mode C: start + duration
					startStr = args[0]
					durationStr = args[1]
					durationSeconds, err = timeutil.ParseDuration(durationStr)
					if err != nil {
						return fmt.Errorf("invalid duration format: %w", err)
					}
				}
			} else {
				return errors.New("invalid arguments")
			}
		} else {
			return errors.New("invalid arguments")
		}
	}

	// Add 1-5% random adjustment to duration to avoid exact round numbers
	if durationSeconds > 0 {
		// Random factor between 1% and 5%
		extraPercent := 1 + rand.Intn(5) // 1 to 5, so total is 1.01 to 1.05
		extraSeconds := (durationSeconds * extraPercent) / 100
		durationSeconds = durationSeconds + extraSeconds
	}

	// Parse date
	dateStrExplicit := dateStr != ""
	if dateStr == "" {
		dateStr = time.Now().Format("2006-01-02")
	}
	year, month, day, err := timeutil.ParseDate(dateStr)
	if err != nil {
		return fmt.Errorf("invalid date format: %w", err)
	}

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

	// Compute start and end times based on mode
	// All user input times are in VNT (UTC+7), convert to UTC for Clockify
	vntLoc := time.FixedZone("VNT", 7*3600)

	var startTime, endTime time.Time

	if relativeStartSeconds > 0 {
		// Relative start time mode: "4h ago" + "30m"
		// Calculate start time as now - relativeStartSeconds
		now := time.Now().In(vntLoc)
		if dateStrExplicit {
			now = time.Date(year, time.Month(month), day, 0, 0, 0, 0, vntLoc)
		}
		startTime = now.Add(-time.Duration(relativeStartSeconds) * time.Second)
		endTime = startTime.Add(time.Duration(durationSeconds) * time.Second)
	} else if durationStr != "" && startStr == "" {
		// Mode A: start = now - duration, end = now (in VNT)
		// Get current time in VNT
		now := time.Now().In(vntLoc)
		// If --date was explicitly provided, interpret "now" as 00:00 on that date
		// This ensures "4h ago" with --date 2026-04-16 means 20:00 on April 15
		if dateStrExplicit {
			now = time.Date(year, time.Month(month), day, 0, 0, 0, 0, vntLoc)
		}
		endTime = time.Date(year, time.Month(month), day, now.Hour(), now.Minute(), now.Second(), 0, vntLoc).UTC()
		startTime = endTime.Add(-time.Duration(durationSeconds) * time.Second)
	} else if endStr != "" {
		// Mode B: start + end times (both in VNT)
		startSecs, _ := timeutil.ParseTimeHHMM(startStr)
		endSecs, _ := timeutil.ParseTimeHHMM(endStr)
		startTime = time.Date(year, time.Month(month), day, startSecs/3600, (startSecs%3600)/60, 0, 0, vntLoc).UTC()
		endTime = time.Date(year, time.Month(month), day, endSecs/3600, (endSecs%3600)/60, 0, 0, vntLoc).UTC()
		if endTime.Before(startTime) {
			endTime = endTime.Add(24 * time.Hour)
		}
	} else {
		// Mode C: start + duration (start time in VNT)
		startSecs, _ := timeutil.ParseTimeHHMM(startStr)
		startTime = time.Date(year, time.Month(month), day, startSecs/3600, (startSecs%3600)/60, 0, 0, vntLoc).UTC()
		endTime = startTime.Add(time.Duration(durationSeconds) * time.Second)
	}

	// Recalculate duration from computed times
	durationSeconds = int(endTime.Sub(startTime).Seconds())

	// Load config
	cfg, err := config.Load()
	if err != nil {
		return fmt.Errorf("failed to load config: %w", err)
	}

	// Determine project ID
	projectID := projectIDOverride
	if projectID == "" && cardURL != "" {
		// Use board mapping when card URL is provided
		projectID = cfg.GetClockifyProjectID(cardInfo.BoardID, cardInfo.BoardName)
	}
	if projectID == "" && description == "" {
		return errors.New("no project ID found (configure board-to-project mapping in config or use --project-id)")
	}
	if projectID == "" {
		return errors.New("--project-id is required when using --description without a card URL")
	}

	// Create Clockify client
	client := clockify.NewClient(cfg.APIKey, cfg.WorkspaceID)

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

	// List existing entries for the date (for overlap validation)
	var entries []clockify.TimeEntry
	if cfg.UserID != "" {
		entries, err = client.ListEntriesForUser(cfg.UserID, dateStr, dateStr)
	} else {
		entries, err = client.GetDetailedReport(dateStr+"T00:00:00Z", dateStr+"T23:59:59Z")
	}
	if err != nil {
		fmt.Fprintf(os.Stderr, "Warning: Could not fetch existing entries for overlap check: %v\n", err)
		fmt.Fprintf(os.Stderr, "Proceeding without overlap validation.\n")
		entries = nil
	}

	// Check overlap (only if we have entries data)
	if entries != nil {
		overlap, conflictingEntry := validator.HasOverlap(startTime, endTime, entries)
		if overlap && conflictingEntry != nil {
			entryStart := parseTimeEntryTime(conflictingEntry.TimeInterval.Start)
			entryEnd := parseTimeEntryTime(conflictingEntry.TimeInterval.End)
			return fmt.Errorf("Entry overlaps with existing entry %s (%s - %s)", conflictingEntry.Name, entryStart.Format("15:04"), entryEnd.Format("15:04"))
		}

		// Check 14h cap
		existingTotal := validator.DailyTotal(startTime, entries)
		if validator.ExceedsCap(startTime, existingTotal, durationSeconds) {
			hours := float64(existingTotal+durationSeconds) / 3600.0
			return fmt.Errorf("Total time for %s would be %.1fh (exceeds 14h limit)", dateStr, hours)
		}
	}

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

	result, err := client.CreateEntry(entryReq)
	if err != nil {
		return fmt.Errorf("Clockify API error: %w", err)
	}

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
	fmt.Printf("  Link:       https://app.clockify.me/workspaces/%s/time-entries/%s\n", client.WorkspaceID(), result.ID)

	// Print project total for today
	fmt.Println()
	if err := printProjectTotalToday(client, result.ProjectID); err != nil {
		fmt.Fprintf(os.Stderr, "Warning: Could not fetch project total: %v\n", err)
	}

	return nil
}

func runSummary(preset, from, to string, byTask bool) error {
	start, end, label, err := summary.ResolveDateRange(preset, from, to)
	if err != nil {
		return err
	}

	cfg, err := config.Load()
	if err != nil {
		return fmt.Errorf("failed to load config: %w", err)
	}

	client := clockify.NewClient(cfg.APIKey, cfg.WorkspaceID)

	var entries []clockify.TimeEntry
	var listErr error

	if cfg.UserID != "" {
		entries, listErr = client.ListEntriesForUser(cfg.UserID, start, end)
	} else {
		// Fallback to detailed report if no user ID configured
		entries, listErr = client.GetDetailedReport(start, end)
	}
	if listErr != nil {
		return fmt.Errorf("Clockify API error: %w", listErr)
	}

	projects, err := client.ListProjects()
	if err != nil {
		return fmt.Errorf("Clockify API error: %w", err)
	}

	fmt.Printf("Summary: %s\n\n", label)

	var grandTotal int

	if byTask {
		breakdown := summary.ComputeTaskBreakdown(entries, projects)
		for _, pt := range breakdown {
			fmt.Printf("%-35s %s\n", pt.Name, timeutil.FormatDuration(pt.Duration))
			for _, t := range pt.Tasks {
				fmt.Printf("  %-33s %s\n", t.Name, timeutil.FormatDuration(t.Duration))
			}
			grandTotal += pt.Duration
			fmt.Println()
		}
	} else {
		totals := summary.ComputeProjectTotals(entries, projects)
		for _, pt := range totals {
			fmt.Printf("%-35s %s\n", pt.Name, timeutil.FormatDuration(pt.Duration))
			grandTotal += pt.Duration
		}
	}

	fmt.Println(strings.Repeat("\u2500", 40))
	fmt.Printf("%-35s %s\n", "Total", timeutil.FormatDuration(grandTotal))

	return nil
}

func printProjectTotalToday(client *clockify.Client, projectID string) error {
	now := time.Now()
	today := now.Format("2006-01-02")

	cfg, err := config.Load()
	if err != nil {
		return fmt.Errorf("failed to load config: %w", err)
	}

	var entries []clockify.TimeEntry
	if cfg.UserID != "" {
		entries, err = client.ListEntriesForUser(cfg.UserID, today, today)
	} else {
		entries, err = client.GetDetailedReport(today+"T00:00:00Z", today+"T23:59:59Z")
	}
	if err != nil {
		return err
	}

	projects, err := client.ListProjects()
	if err != nil {
		return err
	}

	totals := summary.ComputeProjectTotals(entries, projects)

	// Find the total for the specified project
	projectName := projectID
	for _, p := range projects {
		if p.ID == projectID {
			projectName = p.Name
			break
		}
	}

	for _, pt := range totals {
		if pt.Name == projectName || pt.Name == projectID {
			fmt.Printf("Project total today: %s (%s)\n", projectName, timeutil.FormatDuration(pt.Duration))
			return nil
		}
	}

	// Project not found in entries yet (might be the only entry today)
	fmt.Printf("Project total today: %s (0:00)\n", projectName)
	return nil
}

func parseTimeEntryTime(s string) time.Time {
	t, err := time.Parse(time.RFC3339, s)
	if err != nil {
		return time.Time{}
	}
	return t
}

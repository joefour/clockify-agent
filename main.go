package main

import (
	"errors"
	"flag"
	"fmt"
	"math/rand"
	"os"
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
		fs.Parse(os.Args[2:])

		if err := runLog(fs.Args(), *flagDate, *flagDryRun, *flagProjectID, *flagTaskID, *flagDescription); err != nil {
			fmt.Fprintf(os.Stderr, "Error: %s\n", err.Error())
			os.Exit(1)
		}

	case "summary":
		fs := flag.NewFlagSet("summary", flag.ExitOnError)
		flagByTask := fs.Bool("by-task", false, "Show per-task breakdown within each project")
		flagFrom := fs.String("from", "", "Start date (YYYY-MM-DD)")
		flagTo := fs.String("to", "", "End date (YYYY-MM-DD)")
		fs.Usage = func() { usage() }
		fs.Parse(os.Args[2:])

		preset := ""
		if fs.NArg() > 0 {
			preset = fs.Arg(0)
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

func runLog(args []string, dateStr string, dryRun bool, projectIDOverride, taskID, description string) error {
	// Determine if we have a card URL or using description-only mode
	if len(args) < 1 && description == "" {
		return errors.New("invalid arguments: provide a card URL or --description")
	}
	if len(args) < 2 && description == "" {
		return errors.New("invalid arguments: expected <duration> or <start> <end>")
	}

	cardURL := ""
	if len(args) >= 1 {
		cardURL = args[0]
	}

	// Detect mode by arg count
	var startStr, endStr, durationStr string
	var durationSeconds int
	var err error

	if len(args) == 2 {
		// Mode A: cardURL + duration
		durationStr = args[1]
		durationSeconds, err = timeutil.ParseDuration(durationStr)
		if err != nil {
			return fmt.Errorf("invalid duration format: %w", err)
		}
	} else if len(args) == 3 {
		// Mode B or C: need to determine by parsing 2nd arg
		if _, err = timeutil.ParseTimeHHMM(args[1]); err == nil {
			if _, err = timeutil.ParseTimeHHMM(args[2]); err == nil {
				// Mode B: start + end (both times)
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

	// Add 1-5% random adjustment to duration to avoid exact round numbers
	if durationSeconds > 0 {
		// Random factor between 1% and 5%
		extraPercent := 1 + rand.Intn(5) // 1 to 5, so total is 1.01 to 1.05
		extraSeconds := (durationSeconds * extraPercent) / 100
		durationSeconds = durationSeconds + extraSeconds
	}

	// Parse date
	if dateStr == "" {
		dateStr = time.Now().Format("2006-01-02")
	}
	year, month, day, err := timeutil.ParseDate(dateStr)
	if err != nil {
		return fmt.Errorf("invalid date format: %w", err)
	}

	// Fetch card info
	cardInfo, err := trello.FetchCard(cardURL)
	if err != nil {
		return fmt.Errorf("failed to fetch card info: %w", err)
	}

	// Compute start and end times based on mode
	// All user input times are in VNT (UTC+7), convert to UTC for Clockify
	vntLoc := time.FixedZone("VNT", 7*3600)

	var startTime, endTime time.Time

	if durationStr != "" && startStr == "" {
		// Mode A: start = now - duration, end = now (in VNT)
		now := time.Now()
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

	// Determine project ID via board mapping
	projectID := projectIDOverride
	if projectID == "" {
		projectID = cfg.GetClockifyProjectID(cardInfo.BoardID, "")
	}
	if projectID == "" {
		return errors.New("no project ID found (configure board-to-project mapping in config)")
	}

	// Create Clockify client
	client := clockify.NewClient(cfg.APIKey, cfg.WorkspaceID)

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

	// List existing entries for the date (for overlap validation)
	entries, err := client.GetDetailedReport(dateStr+"T00:00:00Z", dateStr+"T23:59:59Z")
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
		fmt.Printf("  Card:        %s\n", cardInfo.CardNumber)
		fmt.Printf("  Description: %s\n", cardInfo.Title)
		fmt.Printf("  Start:       %s\n", startTime.Format(time.RFC3339))
		fmt.Printf("  End:         %s\n", endTime.Format(time.RFC3339))
		fmt.Printf("  Duration:    %s\n", timeutil.FormatDuration(durationSeconds))
		fmt.Printf("  Project:     %s\n", projectID)
		return nil
	}

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

	result, err := client.CreateEntry(entryReq)
	if err != nil {
		return fmt.Errorf("Clockify API error: %w", err)
	}

	fmt.Println("Created time entry:")
	fmt.Printf("  Card:        %s\n", result.Name)
	fmt.Printf("  Description: %s\n", result.Description)
	fmt.Printf("  Start:       %s\n", result.TimeInterval.Start)
	fmt.Printf("  End:         %s\n", result.TimeInterval.End)
	fmt.Printf("  Duration:    %s\n", timeutil.FormatDuration(int(endTime.Sub(startTime).Seconds())))
	fmt.Printf("  Project:     %s\n", result.ProjectID)

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

func parseTimeEntryTime(s string) time.Time {
	t, err := time.Parse(time.RFC3339, s)
	if err != nil {
		return time.Time{}
	}
	return t
}

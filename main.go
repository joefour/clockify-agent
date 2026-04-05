package main

import (
	"errors"
	"flag"
	"fmt"
	"os"
	"time"

	"github.com/loithai/clockify-agent/internal/clockify"
	"github.com/loithai/clockify-agent/internal/config"
	"github.com/loithai/clockify-agent/internal/timeutil"
	"github.com/loithai/clockify-agent/internal/trello"
	"github.com/loithai/clockify-agent/internal/validator"
)

var (
	flagDate      = flag.String("date", "", "Date in YYYY-MM-DD format (default: today)")
	flagDryRun    = flag.Bool("dry-run", false, "Print entry details without creating")
	flagProjectID = flag.String("project-id", "", "Override default project ID")
	flagTaskID    = flag.String("task-id", "", "Task ID for the entry")
)

func usage() {
	fmt.Println(`clockify log <card-url> <duration>
clockify log <card-url> <start> <end>
clockify log <card-url> <start> <duration>

Flags:
  --date YYYY-MM-DD    Date for the entry (default: today)
  --dry-run            Print entry details without creating
  --project-id ID      Override default project ID
  --task-id ID         Task ID for the entry`)
}

func main() {
	flag.Usage = usage
	flag.Parse()

	if flag.NArg() < 1 {
		flag.Usage()
		os.Exit(1)
	}

	subcmd := flag.Arg(0)
	if subcmd != "log" {
		fmt.Fprintf(os.Stderr, "Error: unknown subcommand %q\n", subcmd)
		flag.Usage()
		os.Exit(1)
	}

	if err := runLog(); err != nil {
		fmt.Fprintf(os.Stderr, "Error: %s\n", err.Error())
		os.Exit(1)
	}
}

func runLog() error {
	args := flag.Args()[1:] // remove "log"

	if len(args) < 2 {
		return errors.New("invalid arguments: expected <card-url> <duration> or <card-url> <start> <end>")
	}

	cardURL := args[0]

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
		// If parses as HH:MM -> mode B (start time)
		// If parses as H:MM:SS -> mode C (duration)
		if _, err = timeutil.ParseTimeHHMM(args[1]); err == nil {
			// Mode B: start + end (both times)
			startStr = args[1]
			endStr = args[2]
			if _, err = timeutil.ParseTimeHHMM(endStr); err != nil {
				return fmt.Errorf("invalid end time format: %w", err)
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

	// Parse date
	dateStr := *flagDate
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
	var startTime, endTime time.Time

	if durationStr != "" && startStr == "" {
		// Mode A: start = now - duration, end = now
		// We need to compute start from end - duration
		now := time.Now()
		endTime = time.Date(year, time.Month(month), day, now.Hour(), now.Minute(), now.Second(), 0, time.UTC)
		startTime = endTime.Add(-time.Duration(durationSeconds) * time.Second)
	} else if endStr != "" {
		// Mode B: start + end times
		startSecs, _ := timeutil.ParseTimeHHMM(startStr)
		endSecs, _ := timeutil.ParseTimeHHMM(endStr)
		startTime = time.Date(year, time.Month(month), day, startSecs/3600, (startSecs%3600)/60, 0, 0, time.UTC)
		endTime = time.Date(year, time.Month(month), day, endSecs/3600, (endSecs%3600)/60, 0, 0, time.UTC)
		if endTime.Before(startTime) {
			endTime = endTime.Add(24 * time.Hour)
		}
	} else {
		// Mode C: start + duration
		startSecs, _ := timeutil.ParseTimeHHMM(startStr)
		startTime = time.Date(year, time.Month(month), day, startSecs/3600, (startSecs%3600)/60, 0, 0, time.UTC)
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
	projectID := *flagProjectID
	if projectID == "" {
		projectID = cfg.DefaultProjectID
	}
	if projectID == "" {
		return errors.New("no project ID provided (use --project-id or set default_project_id in config)")
	}

	// Create Clockify client
	client := clockify.NewClient(cfg.APIKey, cfg.WorkspaceID)

	// List existing entries for the date
	entries, err := client.ListEntriesForDate(dateStr)
	if err != nil {
		return fmt.Errorf("Clockify API error: %w", err)
	}

	// Check overlap
	overlap, conflictingEntry := validator.HasOverlap(startTime, endTime, entries)
	if overlap && conflictingEntry != nil {
		entryStart := parseTimeEntryTime(conflictingEntry.Start)
		entryEnd := parseTimeEntryTime(conflictingEntry.End)
		return fmt.Errorf("Entry overlaps with existing entry %q (%s - %s)", conflictingEntry.Name, entryStart.Format("15:04"), entryEnd.Format("15:04"))
	}

	// Check 14h cap
	existingTotal := validator.DailyTotal(startTime, entries)
	if validator.ExceedsCap(startTime, existingTotal, durationSeconds) {
		hours := float64(existingTotal+durationSeconds) / 3600.0
		return fmt.Errorf("Total time for %s would be %.1fh (exceeds 14h limit)", dateStr, hours)
	}

	// Dry run
	if *flagDryRun {
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
		TaskID:      *flagTaskID,
		Name:        cardInfo.CardNumber,
		Description: cardInfo.Title,
		Start:       startTime.Format(time.RFC3339),
		End:         endTime.Format(time.RFC3339),
	}

	result, err := client.CreateEntry(entryReq)
	if err != nil {
		return fmt.Errorf("Clockify API error: %w", err)
	}

	fmt.Println("Created time entry:")
	fmt.Printf("  Card:       %s\n", result.Name)
	fmt.Printf("  Description: %s\n", result.Description)
	fmt.Printf("  Start:      %s\n", result.Start)
	fmt.Printf("  End:        %s\n", result.End)
	fmt.Printf("  Duration:   %s\n", timeutil.FormatDuration(int(endTime.Sub(startTime).Seconds())))
	fmt.Printf("  Project:    %s\n", result.ProjectID)

	return nil
}

func parseTimeEntryTime(s string) time.Time {
	t, err := time.Parse(time.RFC3339, s)
	if err != nil {
		return time.Time{}
	}
	return t
}

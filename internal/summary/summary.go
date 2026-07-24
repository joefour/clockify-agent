package summary

import (
	"fmt"
	"sort"
	"time"

	"github.com/loithai/clockify-agent/internal/clockify"
)

// ProjectTotal holds total hours for a single project.
type ProjectTotal struct {
	Name     string
	Duration int // seconds
}

// TaskTotal holds total hours for a single task within a project.
type TaskTotal struct {
	Name     string
	Duration int // seconds
}

// ProjectTasks holds a project's total and its task breakdown.
type ProjectTasks struct {
	Name     string
	Duration int // seconds
	Tasks    []TaskTotal
}

// ResolveDateRange converts a preset name (or explicit from/to dates) into
// start/end date strings (YYYY-MM-DD) and a human-readable label.
// When preset is non-empty, from and to are ignored.
func ResolveDateRange(preset, from, to string) (start, end, label string, err error) {
	if preset != "" {
		return resolvePreset(preset)
	}
	if from == "" || to == "" {
		return "", "", "", fmt.Errorf("provide a preset (today, yesterday, this-week, last-week, this-month, last-month) or --from/--to dates")
	}
	return fmt.Sprintf("%sT00:00:00Z", from), fmt.Sprintf("%sT23:59:59Z", to), fmt.Sprintf("%s -> %s", from, to), nil
}

func resolvePreset(preset string) (start, end, label string, err error) {
	now := time.Now()
	today := now.Format("2006-01-02")

	switch preset {
	case "today":
		return today, today, fmt.Sprintf("today (%s)", today), nil

	case "yesterday":
		y := now.AddDate(0, 0, -1).Format("2006-01-02")
		return y, y, fmt.Sprintf("yesterday (%s)", y), nil

	case "this-week":
		monday := mondayOf(now)
		return monday.Format("2006-01-02"), today,
			fmt.Sprintf("this week (%s -> %s)", monday.Format("2006-01-02"), today), nil

	case "last-week":
		lastWeekMonday := mondayOf(now).AddDate(0, 0, -7)
		lastWeekSunday := lastWeekMonday.AddDate(0, 0, 6)
		return lastWeekMonday.Format("2006-01-02"), lastWeekSunday.Format("2006-01-02"),
			fmt.Sprintf("last week (%s -> %s)", lastWeekMonday.Format("2006-01-02"), lastWeekSunday.Format("2006-01-02")), nil

	case "this-month":
		firstOfMonth := time.Date(now.Year(), now.Month(), 1, 0, 0, 0, 0, time.UTC)
		return firstOfMonth.Format("2006-01-02"), today,
			fmt.Sprintf("this month (%s -> %s)", firstOfMonth.Format("2006-01-02"), today), nil

	case "last-month":
		firstOfLastMonth := time.Date(now.Year(), now.Month()-1, 1, 0, 0, 0, 0, time.UTC)
		lastOfLastMonth := time.Date(now.Year(), now.Month(), 0, 0, 0, 0, 0, time.UTC)
		return firstOfLastMonth.Format("2006-01-02"), lastOfLastMonth.Format("2006-01-02"),
			fmt.Sprintf("last month (%s -> %s)", firstOfLastMonth.Format("2006-01-02"), lastOfLastMonth.Format("2006-01-02")), nil

	default:
		return "", "", "", fmt.Errorf("unknown preset %q (use: today, yesterday, this-week, last-week, this-month, last-month)", preset)
	}
}

func mondayOf(t time.Time) time.Time {
	weekday := t.Weekday()
	// Sunday = 0, shift to 6; otherwise offset by (weekday - Monday)
	offset := int(weekday)
	if weekday == time.Sunday {
		offset = 6
	} else {
		offset = int(weekday - time.Monday)
	}
	return t.AddDate(0, 0, -offset)
}

// ComputeProjectTotals aggregates entry durations by project.
// Projects are sorted alphabetically by name.
func ComputeProjectTotals(entries []clockify.TimeEntry, projects []clockify.Project) []ProjectTotal {
	projectName := buildProjectNameMap(projects)

	totals := make(map[string]int) // projectID -> seconds
	for _, e := range entries {
		if e.TimeInterval.End == "" || e.ProjectID == "" {
			continue // skip running timers and entries without project
		}
		dur := entryDuration(e)
		if dur == 0 {
			continue
		}
		totals[e.ProjectID] += dur
	}

	result := make([]ProjectTotal, 0, len(totals))
	for pid, dur := range totals {
		name := projectName[pid]
		if name == "" {
			name = pid
		}
		result = append(result, ProjectTotal{Name: name, Duration: dur})
	}

	sort.Slice(result, func(i, j int) bool {
		return result[i].Name < result[j].Name
	})
	return result
}

// ComputeTaskBreakdown aggregates entry durations by project, then by task.
// Projects sorted alphabetically; tasks sorted alphabetically within each project.
func ComputeTaskBreakdown(entries []clockify.TimeEntry, projects []clockify.Project) []ProjectTasks {
	projectName := buildProjectNameMap(projects)

	type projKey struct {
		id   string
		name string
	}

	// project -> task name -> seconds
	projTasks := make(map[projKey]map[string]int)
	projTotal := make(map[projKey]int)

	for _, e := range entries {
		if e.TimeInterval.End == "" || e.ProjectID == "" {
			continue
		}
		dur := entryDuration(e)
		if dur == 0 {
			continue
		}
		pk := projKey{id: e.ProjectID, name: projectName[e.ProjectID]}
		if pk.name == "" {
			pk.name = e.ProjectID
		}
		projTotal[pk] += dur

		taskName := "(no task)"
		if e.Task != nil && e.Task.Name != "" {
			taskName = e.Task.Name
		} else if e.Name != "" {
			taskName = e.Name
		} else if e.Description != "" {
			taskName = e.Description
		}
		if projTasks[pk] == nil {
			projTasks[pk] = make(map[string]int)
		}
		projTasks[pk][taskName] += dur
	}

	result := make([]ProjectTasks, 0, len(projTotal))
	for pk, total := range projTotal {
		tasks := make([]TaskTotal, 0, len(projTasks[pk]))
		for tname, tdur := range projTasks[pk] {
			tasks = append(tasks, TaskTotal{Name: tname, Duration: tdur})
		}
		sort.Slice(tasks, func(i, j int) bool {
			return tasks[i].Name < tasks[j].Name
		})
		result = append(result, ProjectTasks{
			Name:     pk.name,
			Duration: total,
			Tasks:    tasks,
		})
	}

	sort.Slice(result, func(i, j int) bool {
		return result[i].Name < result[j].Name
	})
	return result
}

func buildProjectNameMap(projects []clockify.Project) map[string]string {
	m := make(map[string]string, len(projects))
	for _, p := range projects {
		m[p.ID] = p.Name
	}
	return m
}

func entryDuration(e clockify.TimeEntry) int {
	start, err := time.Parse(time.RFC3339, e.TimeInterval.Start)
	if err != nil {
		return 0
	}
	end, err := time.Parse(time.RFC3339, e.TimeInterval.End)
	if err != nil {
		return 0
	}
	dur := int(end.Sub(start).Seconds())
	// Skip entries with duration > 24h (these are summary/range entries, not real entries)
	if dur > 24*3600 {
		return 0
	}
	return dur
}

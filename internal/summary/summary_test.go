package summary

import (
	"testing"
	"time"

	"github.com/loithai/clockify-agent/internal/clockify"
)

func TestResolveDateRange_Today(t *testing.T) {
	start, end, label, err := ResolveDateRange("today", "", "")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	today := time.Now().Format("2006-01-02")
	if start != today {
		t.Errorf("expected start %s, got %s", today, start)
	}
	if end != today {
		t.Errorf("expected end %s, got %s", today, end)
	}
	if label == "" {
		t.Error("expected non-empty label")
	}
}

func TestResolveDateRange_CustomRange(t *testing.T) {
	start, end, label, err := ResolveDateRange("", "2026-04-01", "2026-04-09")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if start != "2026-04-01T00:00:00Z" {
		t.Errorf("expected start 2026-04-01T00:00:00Z, got %s", start)
	}
	if end != "2026-04-09T23:59:59Z" {
		t.Errorf("expected end 2026-04-09T23:59:59Z, got %s", end)
	}
	if label != "2026-04-01 -> 2026-04-09" {
		t.Errorf("unexpected label: %s", label)
	}
}

func TestResolveDateRange_NoPresetNoDates(t *testing.T) {
	_, _, _, err := ResolveDateRange("", "", "")
	if err == nil {
		t.Error("expected error when no preset or dates provided")
	}
}

func TestResolveDateRange_UnknownPreset(t *testing.T) {
	_, _, _, err := ResolveDateRange("tomorrow", "", "")
	if err == nil {
		t.Error("expected error for unknown preset")
	}
}

func TestComputeProjectTotals(t *testing.T) {
	entries := []clockify.TimeEntry{
		{
			ProjectID: "proj1",
			TimeInterval: clockify.TimeInterval{
				Start: "2026-04-09T09:00:00Z",
				End:   "2026-04-09T12:00:00Z",
			},
		},
		{
			ProjectID: "proj2",
			TimeInterval: clockify.TimeInterval{
				Start: "2026-04-09T13:00:00Z",
				End:   "2026-04-09T15:00:00Z",
			},
		},
		{
			ProjectID: "proj1",
			TimeInterval: clockify.TimeInterval{
				Start: "2026-04-09T16:00:00Z",
				End:   "2026-04-09T17:30:00Z",
			},
		},
	}

	projects := []clockify.Project{
		{ID: "proj1", Name: "Alpha"},
		{ID: "proj2", Name: "Beta"},
	}

	totals := ComputeProjectTotals(entries, projects)

	if len(totals) != 2 {
		t.Fatalf("expected 2 projects, got %d", len(totals))
	}

	// Sorted alphabetically: Alpha, Beta
	if totals[0].Name != "Alpha" {
		t.Errorf("expected first project Alpha, got %s", totals[0].Name)
	}
	// Alpha: 3h + 1.5h = 4.5h = 16200s
	if totals[0].Duration != 16200 {
		t.Errorf("expected Alpha duration 16200s, got %d", totals[0].Duration)
	}

	if totals[1].Name != "Beta" {
		t.Errorf("expected second project Beta, got %s", totals[1].Name)
	}
	// Beta: 2h = 7200s
	if totals[1].Duration != 7200 {
		t.Errorf("expected Beta duration 7200s, got %d", totals[1].Duration)
	}
}

func TestComputeProjectTotals_SkipsRunningTimers(t *testing.T) {
	entries := []clockify.TimeEntry{
		{
			ProjectID: "proj1",
			TimeInterval: clockify.TimeInterval{
				Start: "2026-04-09T09:00:00Z",
				End:   "", // running timer
			},
		},
		{
			ProjectID: "proj1",
			TimeInterval: clockify.TimeInterval{
				Start: "2026-04-09T10:00:00Z",
				End:   "2026-04-09T11:00:00Z",
			},
		},
	}

	totals := ComputeProjectTotals(entries, nil)
	if len(totals) != 1 {
		t.Fatalf("expected 1 project, got %d", len(totals))
	}
	if totals[0].Duration != 3600 {
		t.Errorf("expected 3600s, got %d", totals[0].Duration)
	}
}

func TestComputeTaskBreakdown(t *testing.T) {
	entries := []clockify.TimeEntry{
		{
			ProjectID: "proj1",
			Name:      "TASK-101",
			Task:      &clockify.TaskRef{ID: "t1", Name: "Feature X"},
			TimeInterval: clockify.TimeInterval{
				Start: "2026-04-09T09:00:00Z",
				End:   "2026-04-09T12:00:00Z",
			},
		},
		{
			ProjectID: "proj1",
			Name:      "TASK-102",
			Task:      &clockify.TaskRef{ID: "t2", Name: "Bug fix"},
			TimeInterval: clockify.TimeInterval{
				Start: "2026-04-09T13:00:00Z",
				End:   "2026-04-09T14:30:00Z",
			},
		},
		{
			ProjectID: "proj1",
			Name:      " unplanned",
			TimeInterval: clockify.TimeInterval{
				Start: "2026-04-09T15:00:00Z",
				End:   "2026-04-09T17:00:00Z",
			},
		},
	}

	projects := []clockify.Project{
		{ID: "proj1", Name: "Alpha"},
	}

	breakdown := ComputeTaskBreakdown(entries, projects)

	if len(breakdown) != 1 {
		t.Fatalf("expected 1 project, got %d", len(breakdown))
	}

	pt := breakdown[0]
	if pt.Name != "Alpha" {
		t.Errorf("expected Alpha, got %s", pt.Name)
	}
	// Total: 3h + 1.5h + 2h = 6.5h = 23400s
	if pt.Duration != 23400 {
		t.Errorf("expected total 23400s, got %d", pt.Duration)
	}
	if len(pt.Tasks) != 3 {
		t.Fatalf("expected 3 tasks, got %d", len(pt.Tasks))
	}

	// Tasks sorted alphabetically (space sorts before letters)
	if pt.Tasks[0].Name != " unplanned" {
		t.Errorf("expected first task ' unplanned', got %s", pt.Tasks[0].Name)
	}
	if pt.Tasks[1].Name != "Bug fix" {
		t.Errorf("expected second task 'Bug fix', got %s", pt.Tasks[1].Name)
	}
	if pt.Tasks[2].Name != "Feature X" {
		t.Errorf("expected third task 'Feature X', got %s", pt.Tasks[2].Name)
	}
}

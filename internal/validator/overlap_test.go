package validator

import (
	"testing"
	"time"

	"github.com/loithai/clockify-agent/internal/clockify"
)

func TestHasOverlap_NoOverlap_DifferentDays(t *testing.T) {
	// Entry on April 5, new entry on April 6 - no overlap
	entries := []clockify.TimeEntry{
		{ID: "1", Start: "2026-04-05T09:00:00Z", End: "2026-04-05T10:00:00Z"},
	}

	start := time.Date(2026, 4, 6, 9, 0, 0, 0, time.UTC)
	end := time.Date(2026, 4, 6, 10, 0, 0, 0, time.UTC)

	hasOverlap, conflicting := HasOverlap(start, end, entries)

	if hasOverlap {
		t.Errorf("Expected no overlap, but got overlap with entry %s", conflicting.ID)
	}
	if conflicting != nil {
		t.Errorf("Expected nil conflicting entry, but got %s", conflicting.ID)
	}
}

func TestHasOverlap_NoOverlap_EntriesDontTouch(t *testing.T) {
	// Existing entry: 09:00-10:00, new entry: 10:00-11:00 - they touch at endpoints
	entries := []clockify.TimeEntry{
		{ID: "1", Start: "2026-04-05T09:00:00Z", End: "2026-04-05T10:00:00Z"},
	}

	start := time.Date(2026, 4, 5, 10, 0, 0, 0, time.UTC)
	end := time.Date(2026, 4, 5, 11, 0, 0, 0, time.UTC)

	hasOverlap, conflicting := HasOverlap(start, end, entries)

	if hasOverlap {
		t.Errorf("Expected no overlap (entries touch at endpoints), but got overlap with entry %s", conflicting.ID)
	}
	if conflicting != nil {
		t.Errorf("Expected nil conflicting entry, but got %s", conflicting.ID)
	}
}

func TestHasOverlap_NoOverlap_BoundaryCase(t *testing.T) {
	// Existing entry: 09:00-10:00, new entry: 10:01-11:00 - no overlap (strict inequality)
	entries := []clockify.TimeEntry{
		{ID: "1", Start: "2026-04-05T09:00:00Z", End: "2026-04-05T10:00:00Z"},
	}

	start := time.Date(2026, 4, 5, 10, 1, 0, 0, time.UTC)
	end := time.Date(2026, 4, 5, 11, 0, 0, 0, time.UTC)

	hasOverlap, conflicting := HasOverlap(start, end, entries)

	if hasOverlap {
		t.Errorf("Expected no overlap, but got overlap with entry %s", conflicting.ID)
	}
	if conflicting != nil {
		t.Errorf("Expected nil conflicting entry, but got %s", conflicting.ID)
	}
}

func TestHasOverlap_Overlap_PartialOverlap(t *testing.T) {
	// Existing: 09:00-10:00, new: 09:30-10:30 - partial overlap
	entries := []clockify.TimeEntry{
		{ID: "1", Start: "2026-04-05T09:00:00Z", End: "2026-04-05T10:00:00Z"},
	}

	start := time.Date(2026, 4, 5, 9, 30, 0, 0, time.UTC)
	end := time.Date(2026, 4, 5, 10, 30, 0, 0, time.UTC)

	hasOverlap, conflicting := HasOverlap(start, end, entries)

	if !hasOverlap {
		t.Errorf("Expected overlap, but got no overlap")
	}
	if conflicting == nil {
		t.Errorf("Expected conflicting entry, but got nil")
	}
	if conflicting != nil && conflicting.ID != "1" {
		t.Errorf("Expected conflicting entry ID '1', but got '%s'", conflicting.ID)
	}
}

func TestHasOverlap_Overlap_NewInsideExisting(t *testing.T) {
	// Existing: 09:00-12:00, new: 10:00-11:00 - new is completely inside existing
	entries := []clockify.TimeEntry{
		{ID: "1", Start: "2026-04-05T09:00:00Z", End: "2026-04-05T12:00:00Z"},
	}

	start := time.Date(2026, 4, 5, 10, 0, 0, 0, time.UTC)
	end := time.Date(2026, 4, 5, 11, 0, 0, 0, time.UTC)

	hasOverlap, conflicting := HasOverlap(start, end, entries)

	if !hasOverlap {
		t.Errorf("Expected overlap, but got no overlap")
	}
	if conflicting == nil {
		t.Errorf("Expected conflicting entry, but got nil")
	}
	if conflicting != nil && conflicting.ID != "1" {
		t.Errorf("Expected conflicting entry ID '1', but got '%s'", conflicting.ID)
	}
}

func TestHasOverlap_Overlap_ExistingInsideNew(t *testing.T) {
	// Existing: 10:00-11:00, new: 09:00-12:00 - existing is completely inside new
	entries := []clockify.TimeEntry{
		{ID: "1", Start: "2026-04-05T10:00:00Z", End: "2026-04-05T11:00:00Z"},
	}

	start := time.Date(2026, 4, 5, 9, 0, 0, 0, time.UTC)
	end := time.Date(2026, 4, 5, 12, 0, 0, 0, time.UTC)

	hasOverlap, conflicting := HasOverlap(start, end, entries)

	if !hasOverlap {
		t.Errorf("Expected overlap, but got no overlap")
	}
	if conflicting == nil {
		t.Errorf("Expected conflicting entry, but got nil")
	}
	if conflicting != nil && conflicting.ID != "1" {
		t.Errorf("Expected conflicting entry ID '1', but got '%s'", conflicting.ID)
	}
}

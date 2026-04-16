package validator

import (
	"testing"
	"time"

	"github.com/loithai/clockify-agent/internal/clockify"
)

func TestDailyTotal_ZeroWhenNoEntries(t *testing.T) {
	entries := []clockify.TimeEntry{}
	date := time.Date(2026, 4, 5, 0, 0, 0, 0, time.UTC)

	total := DailyTotal(date, entries)

	if total != 0 {
		t.Errorf("Expected 0, but got %d", total)
	}
}

func TestDailyTotal_SumOfDurations(t *testing.T) {
	// Entry 1: 09:00-11:00 = 2 hours = 7200 seconds
	// Entry 2: 13:00-15:30 = 2.5 hours = 9000 seconds
	// Total = 16200 seconds
	entries := []clockify.TimeEntry{
		{ID: "1", TimeInterval: clockify.TimeInterval{Start: "2026-04-05T09:00:00Z", End: "2026-04-05T11:00:00Z"}},
		{ID: "2", TimeInterval: clockify.TimeInterval{Start: "2026-04-05T13:00:00Z", End: "2026-04-05T15:30:00Z"}},
	}
	date := time.Date(2026, 4, 5, 0, 0, 0, 0, time.UTC)

	total := DailyTotal(date, entries)

	expected := 2*3600 + 2*3600 + 30*60 // 2h + 2.5h = 16200
	if total != expected {
		t.Errorf("Expected %d, but got %d", expected, total)
	}
}

func TestExceedsCap_FalseWhenUnder14h(t *testing.T) {
	// 13 hours = 46800 seconds - under 14h limit
	existingSeconds := 13 * 3600
	newSeconds := 0
	date := time.Date(2026, 4, 5, 0, 0, 0, 0, time.UTC)

	result := ExceedsCap(date, existingSeconds, newSeconds)

	if result {
		t.Errorf("Expected false (under 14h), but got true")
	}
}

func TestExceedsCap_Exactly14h_ReturnsFalse(t *testing.T) {
	// Exactly 14 hours should return false (at limit is OK)
	existingSeconds := 14 * 3600
	newSeconds := 0
	date := time.Date(2026, 4, 5, 0, 0, 0, 0, time.UTC)

	result := ExceedsCap(date, existingSeconds, newSeconds)

	if result {
		t.Errorf("Expected false (at 14h limit), but got true")
	}
}

func TestExceedsCap_TrueWhenOver14h(t *testing.T) {
	// 14 hours + 1 second - should return true
	existingSeconds := 14*3600 + 1
	newSeconds := 0
	date := time.Date(2026, 4, 5, 0, 0, 0, 0, time.UTC)

	result := ExceedsCap(date, existingSeconds, newSeconds)

	if !result {
		t.Errorf("Expected true (over 14h), but got false")
	}
}

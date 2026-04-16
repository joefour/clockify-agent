package validator

import (
	"time"

	"github.com/loithai/clockify-agent/internal/clockify"
)

// DailyTotal returns total seconds for entries on the given date.
func DailyTotal(date time.Time, entries []clockify.TimeEntry) int {
	total := 0
	for _, entry := range entries {
		entryStart := parseTimeEntryTime(entry.TimeInterval.Start)
		entryEnd := parseTimeEntryTime(entry.TimeInterval.End)

		// Only count entries on the given date
		if entryStart.Year() == date.Year() &&
			entryStart.Month() == date.Month() &&
			entryStart.Day() == date.Day() {
			duration := entryEnd.Sub(entryStart)
			total += int(duration.Seconds())
		}
	}
	return total
}

// ExceedsCap returns true if adding newSeconds to existing entries on date would exceed 14h.
func ExceedsCap(date time.Time, existingSeconds, newSeconds int) bool {
	return existingSeconds+newSeconds > MaxDailySeconds
}

package validator

import (
	"time"

	"github.com/loithai/clockify-agent/internal/clockify"
)

// HasOverlap returns true if [start, end] overlaps with any entry.
// Returns (true, &conflictingEntry) if overlap found, (false, nil) otherwise.
// Rule: new_start < existing_end AND new_end > existing_start
// (Using strict inequality — entries that touch at endpoints don't overlap)
func HasOverlap(start, end time.Time, entries []clockify.TimeEntry) (bool, *clockify.TimeEntry) {
	for i := range entries {
		entry := &entries[i]
		entryStart := parseTimeEntryTime(entry.TimeInterval.Start)
		entryEnd := parseTimeEntryTime(entry.TimeInterval.End)

		// Check overlap: new_start < existing_end AND new_end > existing_start
		if start.Before(entryEnd) && end.After(entryStart) {
			return true, entry
		}
	}
	return false, nil
}

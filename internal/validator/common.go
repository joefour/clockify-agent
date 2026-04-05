package validator

import (
	"time"
)

const MaxDailySeconds = 14 * 3600 // 14 hours

// parseTimeEntryTime parses a time string from Clockify API (RFC3339 format) to time.Time
func parseTimeEntryTime(s string) time.Time {
	t, err := time.Parse(time.RFC3339, s)
	if err != nil {
		return time.Time{}
	}
	return t
}

package timeutil

import (
	"errors"
	"fmt"
	"strconv"
	"strings"
)

// ParseDuration parses duration strings. Returns total seconds.
// Supported formats:
//   - "H:MM:SS" or "H:MM" (e.g., "2:12:05" → 7725, "2:12" → 7320)
//   - "32m" (32 minutes), "1h" (1 hour), "1h30m" (1h30m)
func ParseDuration(s string) (int, error) {
	// Try "XhYm" / "Xh" / "Ym" format first
	if strings.Contains(s, "h") || strings.Contains(s, "m") {
		return parseHumanDuration(s)
	}

	parts := strings.Split(s, ":")
	if len(parts) < 2 || len(parts) > 3 {
		return 0, errors.New("invalid duration format")
	}

	var hours, minutes, seconds int

	if len(parts) == 3 {
		// H:MM:SS format
		if _, err := fmt.Sscanf(parts[0], "%d", &hours); err != nil {
			return 0, errors.New("invalid hours")
		}
		if _, err := fmt.Sscanf(parts[1], "%d", &minutes); err != nil {
			return 0, errors.New("invalid minutes")
		}
		if _, err := fmt.Sscanf(parts[2], "%d", &seconds); err != nil {
			return 0, errors.New("invalid seconds")
		}
	} else {
		// H:MM format
		if _, err := fmt.Sscanf(parts[0], "%d", &hours); err != nil {
			return 0, errors.New("invalid hours")
		}
		if _, err := fmt.Sscanf(parts[1], "%d", &minutes); err != nil {
			return 0, errors.New("invalid minutes")
		}
		seconds = 0
	}

	if hours < 0 || hours >= 24 || minutes < 0 || minutes >= 60 || seconds < 0 || seconds >= 60 {
		return 0, errors.New("invalid time component")
	}

	return hours*3600 + minutes*60 + seconds, nil
}

// ParseTimeHHMM parses "HH:MM" 24-hour format. Returns seconds from midnight.
// "09:00" → 32400 (9*3600)
// "14:30" → 52200 (14*3600 + 30*60)
func ParseTimeHHMM(s string) (int, error) {
	if len(s) != 5 || s[2] != ':' {
		return 0, errors.New("invalid time format")
	}

	var hours, minutes int
	if _, err := fmt.Sscanf(s[:2], "%d", &hours); err != nil {
		return 0, errors.New("invalid hours")
	}
	if _, err := fmt.Sscanf(s[3:], "%d", &minutes); err != nil {
		return 0, errors.New("invalid minutes")
	}

	if hours < 0 || hours >= 24 || minutes < 0 || minutes >= 60 {
		return 0, errors.New("invalid time component")
	}

	return hours*3600 + minutes*60, nil
}

// ParseDate parses "YYYY-MM-DD" format. Returns year, month, day.
// "2026-04-05" → (2026, 4, 5)
func ParseDate(s string) (year, month, day int, err error) {
	if len(s) != 10 || s[4] != '-' || s[7] != '-' {
		return 0, 0, 0, errors.New("invalid date format")
	}

	if _, parseErr := fmt.Sscanf(s[:4], "%d", &year); parseErr != nil {
		return 0, 0, 0, errors.New("invalid year")
	}
	if _, parseErr := fmt.Sscanf(s[5:7], "%d", &month); parseErr != nil {
		return 0, 0, 0, errors.New("invalid month")
	}
	if _, parseErr := fmt.Sscanf(s[8:], "%d", &day); parseErr != nil {
		return 0, 0, 0, errors.New("invalid day")
	}

	if year < 0 || month < 1 || month > 12 || day < 1 || day > 31 {
		return 0, 0, 0, errors.New("invalid date component")
	}

	return year, month, day, nil
}

// parseHumanDuration parses "1h30m", "1h", "30m" style durations.
func parseHumanDuration(s string) (int, error) {
	total := 0
	remaining := s

	for len(remaining) > 0 {
		idx := strings.IndexFunc(remaining, func(r rune) bool { return r < '0' || r > '9' })
		if idx <= 0 {
			return 0, errors.New("invalid duration format")
		}
		val, err := strconv.Atoi(remaining[:idx])
		if err != nil {
			return 0, errors.New("invalid duration number")
		}
		unit := remaining[idx]
		remaining = remaining[idx+1:]

		switch unit {
		case 'h':
			total += val * 3600
		case 'm':
			total += val * 60
		case 's':
			total += val
		default:
			return 0, fmt.Errorf("unknown duration unit '%c'", unit)
		}
	}

	if total == 0 {
		return 0, errors.New("empty duration")
	}
	return total, nil
}

// ParseRelativeTime parses relative time strings like "4h ago", "30m ago", "1h30m ago".
// Returns the number of seconds the time is in the past.
// "4h ago" → 4*3600 = 14400
// "30m ago" → 30*60 = 1800
func ParseRelativeTime(s string) (int, error) {
	s = strings.TrimSpace(s)
	if !strings.HasSuffix(s, " ago") {
		return 0, errors.New("invalid relative time format")
	}
	durationStr := strings.TrimSuffix(s, " ago")
	return ParseDuration(durationStr)
}

// FormatDuration converts total seconds back to "H:MM:SS" format.
// 7925 → "2:12:05"
func FormatDuration(seconds int) string {
	if seconds < 0 {
		return "0:00"
	}

	hours := seconds / 3600
	minutes := (seconds % 3600) / 60
	secs := seconds % 60

	if hours > 0 {
		// Always include seconds when hours > 0
		return fmt.Sprintf("%d:%02d:%02d", hours, minutes, secs)
	}
	// When hours is 0, only include seconds if non-zero
	if secs > 0 {
		return fmt.Sprintf("0:%02d:%02d", minutes, secs)
	}
	return fmt.Sprintf("0:%02d", minutes)
}

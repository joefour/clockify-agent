package timeutil

import (
	"testing"
)

func TestParseDuration(t *testing.T) {
	tests := []struct {
		name     string
		input    string
		expected int
		wantErr  bool
	}{
		{"H:MM:SS format", "2:12:05", 7925, false},
		{"H:MM format (no seconds)", "2:12", 7920, false},
		{"Zero minutes and seconds", "1:00:00", 3600, false},
		{"Single digit hour", "0:05", 300, false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := ParseDuration(tt.input)
			if (err != nil) != tt.wantErr {
				t.Errorf("ParseDuration(%q) error = %v, wantErr %v", tt.input, err, tt.wantErr)
				return
			}
			if got != tt.expected {
				t.Errorf("ParseDuration(%q) = %v, want %v", tt.input, got, tt.expected)
			}
		})
	}
}

func TestParseDurationInvalid(t *testing.T) {
	invalidInputs := []string{
		"invalid",
		"2:12:05:00",
		"",
		"25:00:00", // invalid hour
		"1:60:00",  // invalid minutes
		"1:00:60",  // invalid seconds
	}

	for _, input := range invalidInputs {
		t.Run(input, func(t *testing.T) {
			_, err := ParseDuration(input)
			if err == nil {
				t.Errorf("ParseDuration(%q) expected error, got nil", input)
			}
		})
	}
}

func TestParseTimeHHMM(t *testing.T) {
	tests := []struct {
		name     string
		input    string
		expected int
		wantErr  bool
	}{
		{"Morning time", "09:00", 32400, false},
		{"Afternoon time", "14:30", 52200, false},
		{"Midnight", "00:00", 0, false},
		{"End of day", "23:59", 86340, false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := ParseTimeHHMM(tt.input)
			if (err != nil) != tt.wantErr {
				t.Errorf("ParseTimeHHMM(%q) error = %v, wantErr %v", tt.input, err, tt.wantErr)
				return
			}
			if got != tt.expected {
				t.Errorf("ParseTimeHHMM(%q) = %v, want %v", tt.input, got, tt.expected)
			}
		})
	}
}

func TestParseTimeHHMMInvalid(t *testing.T) {
	invalidInputs := []string{
		"invalid",
		"9:00",      // missing leading zero
		"09:0",      // missing trailing digit
		"24:00",     // invalid hour
		"12:60",     // invalid minutes
		"12:00:00",  // extra component
		"",
	}

	for _, input := range invalidInputs {
		t.Run(input, func(t *testing.T) {
			_, err := ParseTimeHHMM(input)
			if err == nil {
				t.Errorf("ParseTimeHHMM(%q) expected error, got nil", input)
			}
		})
	}
}

func TestParseDate(t *testing.T) {
	tests := []struct {
		name        string
		input       string
		wantYear    int
		wantMonth   int
		wantDay     int
		wantErr     bool
	}{
		{"Valid date", "2026-04-05", 2026, 4, 5, false},
		{"Another valid date", "2024-12-31", 2024, 12, 31, false},
		{"January 1st", "2000-01-01", 2000, 1, 1, false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			year, month, day, err := ParseDate(tt.input)
			if (err != nil) != tt.wantErr {
				t.Errorf("ParseDate(%q) error = %v, wantErr %v", tt.input, err, tt.wantErr)
				return
			}
			if year != tt.wantYear || month != tt.wantMonth || day != tt.wantDay {
				t.Errorf("ParseDate(%q) = (%v, %v, %v), want (%v, %v, %v)",
					tt.input, year, month, day, tt.wantYear, tt.wantMonth, tt.wantDay)
			}
		})
	}
}

func TestParseDateInvalid(t *testing.T) {
	invalidInputs := []string{
		"invalid",
		"2026/04/05",   // wrong separator
		"04-05-2026",   // wrong order
		"2026-4-5",     // missing leading zeros
		"26-04-05",     // 2-digit year
		"2026-13-05",   // invalid month
		"2026-04-32",   // invalid day
		"2026-00-05",   // zero month
		"2026-04-00",   // zero day
		"",
	}

	for _, input := range invalidInputs {
		t.Run(input, func(t *testing.T) {
			_, _, _, err := ParseDate(input)
			if err == nil {
				t.Errorf("ParseDate(%q) expected error, got nil", input)
			}
		})
	}
}

func TestFormatDuration(t *testing.T) {
	tests := []struct {
		name     string
		seconds  int
		expected string
	}{
		{"2:12:05", 7925, "2:12:05"},
		{"0:05", 300, "0:05"},
		{"1:00:00", 3600, "1:00:00"},
		{"Zero", 0, "0:00"},
		{"Round minutes", 3660, "1:01:00"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := FormatDuration(tt.seconds)
			if got != tt.expected {
				t.Errorf("FormatDuration(%v) = %v, want %v", tt.seconds, got, tt.expected)
			}
		})
	}
}

func TestParseDurationFormatDurationRoundtrip(t *testing.T) {
	inputs := []string{"2:12:05", "0:05", "1:00:00", "10:30:45"}

	for _, input := range inputs {
		t.Run(input, func(t *testing.T) {
			seconds, err := ParseDuration(input)
			if err != nil {
				t.Errorf("ParseDuration(%q) unexpected error: %v", input, err)
				return
			}
			got := FormatDuration(seconds)
			if got != input {
				t.Errorf("Roundtrip ParseDuration(%q) -> FormatDuration(%d) = %q, want %q",
					input, seconds, got, input)
			}
		})
	}
}

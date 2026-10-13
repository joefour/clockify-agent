//! Duration, time-of-day and date parsing for the log command, and the H:MM:SS formatter.

use anyhow::{Context, Result, anyhow, bail};
use chrono::NaiveDate;

/// Parses a duration into seconds: "H:MM:SS" or "H:MM" (hours below 24), or
/// "1h30m" / "1h" / "30m" / "45s" style.
pub fn parse_duration(s: &str) -> Result<i64> {
    if s.contains('h') || s.contains('m') {
        return parse_human_duration(s);
    }
    let parts: Vec<&str> = s.split(':').collect();
    let (hours, minutes, seconds) = match parts.as_slice() {
        [h, m, sec] => (num(h, "hours")?, num(m, "minutes")?, num(sec, "seconds")?),
        [h, m] => (num(h, "hours")?, num(m, "minutes")?, 0),
        _ => bail!("invalid duration format"),
    };
    if hours >= 24 || minutes >= 60 || seconds >= 60 {
        bail!("invalid time component");
    }
    Ok(hours * 3600 + minutes * 60 + seconds)
}

/// Parses 24-hour "HH:MM" into seconds from midnight ("09:00" is 32400).
pub fn parse_time_hhmm(s: &str) -> Result<i64> {
    if !s.is_ascii() || s.len() != 5 || s.as_bytes()[2] != b':' {
        bail!("invalid time format");
    }
    let (hours, minutes) = (num(&s[..2], "hours")?, num(&s[3..], "minutes")?);
    if hours >= 24 || minutes >= 60 {
        bail!("invalid time component");
    }
    Ok(hours * 3600 + minutes * 60)
}

/// Parses "YYYY-MM-DD". A date the calendar lacks, such as 2026-02-31, is an error.
pub fn parse_date(s: &str) -> Result<NaiveDate> {
    if !s.is_ascii() || s.len() != 10 || s.as_bytes()[4] != b'-' || s.as_bytes()[7] != b'-' {
        bail!("invalid date format");
    }
    let year = num(&s[..4], "year")?;
    let month = num(&s[5..7], "month")?;
    let day = num(&s[8..], "day")?;
    NaiveDate::from_ymd_opt(year as i32, month as u32, day as u32).context("invalid date component")
}

/// Parses "4h ago" / "30m ago" / "1h30m ago" into seconds in the past.
pub fn parse_relative_time(s: &str) -> Result<i64> {
    match s.trim().strip_suffix(" ago") {
        Some(d) => parse_duration(d),
        None => bail!("invalid relative time format"),
    }
}

/// Formats seconds as "H:MM:SS", or "0:MM" / "0:MM:SS" under an hour. Negative is "0:00".
pub fn format_duration(seconds: i64) -> String {
    if seconds < 0 {
        return "0:00".into();
    }
    let (h, m, s) = (seconds / 3600, seconds % 3600 / 60, seconds % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else if s > 0 {
        format!("0:{m:02}:{s:02}")
    } else {
        format!("0:{m:02}")
    }
}

/// Parses "1h30m", "1h", "30m", "45s". The total is capped at i32::MAX seconds so
/// later chrono arithmetic on it can't overflow.
fn parse_human_duration(s: &str) -> Result<i64> {
    let mut total: i64 = 0;
    let mut rest = s;
    while !rest.is_empty() {
        let idx = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(0);
        if idx == 0 {
            bail!("invalid duration format");
        }
        let val: i64 = rest[..idx].parse().map_err(|_| anyhow!("invalid duration number"))?;
        let unit = rest[idx..].chars().next().unwrap_or_default();
        rest = &rest[idx + unit.len_utf8()..];
        let scale = match unit {
            'h' => 3600,
            'm' => 60,
            's' => 1,
            _ => bail!("unknown duration unit '{unit}'"),
        };
        total = val
            .checked_mul(scale)
            .and_then(|v| v.checked_add(total))
            .filter(|t| *t <= i64::from(i32::MAX))
            .context("invalid duration number")?;
    }
    if total == 0 {
        bail!("empty duration");
    }
    Ok(total)
}

/// Reads an unsigned decimal field (ASCII digits only; Go's Sscanf read "5x" as 5).
/// `what` names the field in the error: "invalid hours".
fn num(s: &str, what: &str) -> Result<i64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        bail!("invalid {what}");
    }
    s.parse().map_err(|_| anyhow!("invalid {what}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn parse_duration_valid() {
        for (input, want) in [
            ("2:12:05", 7925),
            ("2:12", 7920),
            ("1:00:00", 3600),
            ("0:05", 300),
            ("2h", 7200),
            ("30m", 1800),
            ("1h30m", 5400),
            ("1h30m15s", 5415),
        ] {
            assert_eq!(parse_duration(input).unwrap(), want, "{input}");
        }
    }

    #[test]
    fn parse_duration_invalid() {
        for input in [
            "invalid",
            "2:12:05:00",
            "",
            "25:00:00",
            "1:60:00",
            "1:00:60",
            "2x",
            "1.5h",
            "1h30",
            "0m",
            "5x:00",
            "99999999999h",
            "99999999999999999999:00",
            "2hé",
            "2éh",
            "é",
        ] {
            assert!(parse_duration(input).is_err(), "{input}");
        }
    }

    #[test]
    fn parse_duration_error_text() {
        assert_eq!(parse_duration("2x").unwrap_err().to_string(), "invalid duration format");
        assert_eq!(parse_duration("2éh").unwrap_err().to_string(), "unknown duration unit 'é'");
        assert_eq!(parse_duration("0m").unwrap_err().to_string(), "empty duration");
        assert_eq!(parse_duration("25:00").unwrap_err().to_string(), "invalid time component");
    }

    #[test]
    fn parse_time_hhmm_valid() {
        for (input, want) in [("09:00", 32400), ("14:30", 52200), ("00:00", 0), ("23:59", 86340)] {
            assert_eq!(parse_time_hhmm(input).unwrap(), want, "{input}");
        }
    }

    #[test]
    fn parse_time_hhmm_invalid() {
        for input in ["invalid", "9:00", "09:0", "24:00", "12:60", "12:00:00", "", "é:00", "09:é", "+9:00"] {
            assert!(parse_time_hhmm(input).is_err(), "{input}");
        }
    }

    #[test]
    fn parse_date_valid() {
        for (input, (y, m, d)) in [
            ("2026-04-05", (2026, 4, 5)),
            ("2024-12-31", (2024, 12, 31)),
            ("2000-01-01", (2000, 1, 1)),
        ] {
            assert_eq!(parse_date(input).unwrap(), NaiveDate::from_ymd_opt(y, m, d).unwrap(), "{input}");
        }
    }

    #[test]
    fn parse_date_invalid() {
        for input in [
            "invalid",
            "2026/04/05",
            "04-05-2026",
            "2026-4-5",
            "26-04-05",
            "2026-13-05",
            "2026-04-32",
            "2026-00-05",
            "2026-04-00",
            "",
            "2026-02-31",
            "2026-0a-05",
            "2026-é-05",
        ] {
            assert!(parse_date(input).is_err(), "{input}");
        }
    }

    #[test]
    fn parse_relative_time_valid() {
        for (input, want) in [("4h ago", 14400), ("30m ago", 1800), ("1h30m ago", 5400), ("  2h ago  ", 7200)] {
            assert_eq!(parse_relative_time(input).unwrap(), want, "{input}");
        }
    }

    #[test]
    fn parse_relative_time_invalid() {
        for input in ["4h", "ago", "4h  ago", "x ago"] {
            assert!(parse_relative_time(input).is_err(), "{input}");
        }
        assert_eq!(parse_relative_time("4h").unwrap_err().to_string(), "invalid relative time format");
    }

    #[test]
    fn format_duration_cases() {
        for (secs, want) in [
            (7925, "2:12:05"),
            (300, "0:05"),
            (3600, "1:00:00"),
            (0, "0:00"),
            (3660, "1:01:00"),
            (65, "0:01:05"),
            (-5, "0:00"),
        ] {
            assert_eq!(format_duration(secs), want, "{secs}");
        }
    }

    #[test]
    fn parse_then_format_round_trips() {
        for input in ["2:12:05", "0:05", "1:00:00", "10:30:45"] {
            assert_eq!(format_duration(parse_duration(input).unwrap()), input);
        }
    }
}

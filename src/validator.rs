//! Checks run before creating an entry: the 14h daily cap and overlap with existing entries.

use chrono::{DateTime, Utc};

use crate::clockify::TimeEntry;

pub const MAX_DAILY_SECONDS: i64 = 14 * 3600;

/// Total seconds of the entries that start on `date`'s UTC calendar day. Entries
/// without a parseable start and end (a running timer) are skipped.
pub fn daily_total(date: DateTime<Utc>, entries: &[TimeEntry]) -> i64 {
    entries
        .iter()
        .filter_map(TimeEntry::interval)
        .filter(|(start, _)| start.date_naive() == date.date_naive())
        .map(|(start, end)| (end - start).num_seconds())
        .sum()
}

/// True when existing plus new seconds go over 14h; exactly 14h is allowed.
pub fn exceeds_cap(existing_seconds: i64, new_seconds: i64) -> bool {
    existing_seconds + new_seconds > MAX_DAILY_SECONDS
}

/// The first entry that overlaps [start, end]. Entries that only touch at an endpoint
/// don't overlap; entries without a parseable start and end are skipped.
pub fn has_overlap<'a>(start: DateTime<Utc>, end: DateTime<Utc>, entries: &'a [TimeEntry]) -> Option<&'a TimeEntry> {
    entries
        .iter()
        .find(|e| e.interval().is_some_and(|(s, en)| start < en && end > s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clockify::TimeInterval;

    fn entry(id: &str, start: &str, end: &str) -> TimeEntry {
        TimeEntry {
            id: id.into(),
            time_interval: TimeInterval { start: start.into(), end: end.into() },
            ..Default::default()
        }
    }

    fn utc(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    #[test]
    fn daily_total_zero_when_no_entries() {
        assert_eq!(daily_total(utc("2026-04-05T00:00:00Z"), &[]), 0);
    }

    #[test]
    fn daily_total_sums_entries_on_the_day() {
        let entries = [
            entry("1", "2026-04-05T09:00:00Z", "2026-04-05T11:00:00Z"),
            entry("2", "2026-04-05T13:00:00Z", "2026-04-05T15:30:00Z"),
            entry("3", "2026-04-06T09:00:00Z", "2026-04-06T10:00:00Z"),
        ];
        assert_eq!(daily_total(utc("2026-04-05T08:00:00Z"), &entries), 2 * 3600 + 2 * 3600 + 30 * 60);
    }

    #[test]
    fn daily_total_skips_running_timer() {
        // Go counted the empty end as year 1, which drove the total far below zero (fix 7).
        let entries = [
            entry("1", "2026-04-05T09:00:00Z", "2026-04-05T11:00:00Z"),
            entry("2", "2026-04-05T12:00:00Z", ""),
        ];
        assert_eq!(daily_total(utc("2026-04-05T08:00:00Z"), &entries), 7200);
    }

    #[test]
    fn exceeds_cap_cases() {
        assert!(!exceeds_cap(13 * 3600, 0));
        assert!(!exceeds_cap(14 * 3600, 0));
        assert!(exceeds_cap(14 * 3600 + 1, 0));
        assert!(exceeds_cap(13 * 3600, 2 * 3600));
    }

    #[test]
    fn no_overlap_on_different_days() {
        let entries = [entry("1", "2026-04-05T09:00:00Z", "2026-04-05T10:00:00Z")];
        assert!(has_overlap(utc("2026-04-06T09:00:00Z"), utc("2026-04-06T10:00:00Z"), &entries).is_none());
    }

    #[test]
    fn touching_entries_do_not_overlap() {
        let entries = [entry("1", "2026-04-05T09:00:00Z", "2026-04-05T10:00:00Z")];
        assert!(has_overlap(utc("2026-04-05T10:00:00Z"), utc("2026-04-05T11:00:00Z"), &entries).is_none());
        assert!(has_overlap(utc("2026-04-05T10:01:00Z"), utc("2026-04-05T11:00:00Z"), &entries).is_none());
    }

    #[test]
    fn partial_inner_and_outer_overlaps() {
        let one = [entry("1", "2026-04-05T09:00:00Z", "2026-04-05T10:00:00Z")];
        assert_eq!(has_overlap(utc("2026-04-05T09:30:00Z"), utc("2026-04-05T10:30:00Z"), &one).unwrap().id, "1");
        let wide = [entry("1", "2026-04-05T09:00:00Z", "2026-04-05T12:00:00Z")];
        assert_eq!(has_overlap(utc("2026-04-05T10:00:00Z"), utc("2026-04-05T11:00:00Z"), &wide).unwrap().id, "1");
        let narrow = [entry("1", "2026-04-05T10:00:00Z", "2026-04-05T11:00:00Z")];
        assert_eq!(has_overlap(utc("2026-04-05T09:00:00Z"), utc("2026-04-05T12:00:00Z"), &narrow).unwrap().id, "1");
    }

    #[test]
    fn overlap_skips_running_timer() {
        let entries = [entry("1", "2026-04-05T09:00:00Z", "")];
        assert!(has_overlap(utc("2026-04-05T10:00:00Z"), utc("2026-04-05T11:00:00Z"), &entries).is_none());
    }
}

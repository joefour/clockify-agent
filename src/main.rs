#![allow(dead_code)] // Task 8 removes this once the CLI uses every module.

mod clockify;
mod config;
mod summary;
mod timeutil;
mod trello;
mod validator;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveTime, TimeDelta, Timelike, Utc};

use crate::clockify::Client;

/// How the log command's positional arguments set the entry's time. Times of day are
/// seconds from midnight in VNT; durations are seconds.
#[derive(Debug, PartialEq)]
enum Mode {
    /// A: ends now (or at 00:00 on an explicit --date) and lasts this long.
    Duration(i64),
    /// B: start and end times of day.
    StartEnd(i64, i64),
    /// C: start time of day and duration.
    StartDuration(i64, i64),
    /// Starts this many seconds before now, and lasts the duration.
    Relative(i64, i64),
}

impl Mode {
    /// Adds `percent` to the duration so entries don't land on exact round numbers.
    /// Mode B has no duration and stays as it is.
    fn with_extra(self, percent: i64) -> Mode {
        let add = |d: i64| d + d * percent / 100;
        match self {
            Mode::Duration(d) => Mode::Duration(add(d)),
            Mode::StartDuration(s, d) => Mode::StartDuration(s, add(d)),
            Mode::Relative(ago, d) => Mode::Relative(ago, add(d)),
            b @ Mode::StartEnd(..) => b,
        }
    }
}

#[derive(Debug, PartialEq)]
struct LogArgs {
    card_url: Option<String>,
    mode: Mode,
}

/// Reads the log command's positionals: a card URL first (only without --description),
/// then <duration>, <start> <end>, <start> <duration> or "<N> ago" <duration>.
fn parse_log_args(args: &[String], description: &str) -> Result<LogArgs> {
    if args.is_empty() && description.is_empty() {
        bail!("invalid arguments: provide a card URL or --description");
    }
    let card_url = description.is_empty().then(|| args[0].clone());
    if card_url.is_some() && args.len() < 2 {
        bail!("invalid arguments: expected <duration> or <start> <end>");
    }
    let times = if card_url.is_some() { &args[1..] } else { args };
    let mode = match times {
        [d] => Mode::Duration(duration(d)?),
        [first, second] => {
            if let Ok(ago) = timeutil::parse_relative_time(first) {
                Mode::Relative(ago, duration(second)?)
            } else if let Ok(start) = timeutil::parse_time_hhmm(first) {
                match timeutil::parse_time_hhmm(second) {
                    Ok(end) => Mode::StartEnd(start, end),
                    Err(_) => Mode::StartDuration(start, duration(second)?),
                }
            } else {
                // Go's card mode logged at 00:00 here; no-card mode already refused (fix 6).
                bail!("invalid arguments");
            }
        }
        _ => bail!("invalid arguments"),
    };
    Ok(LogArgs { card_url, mode })
}

fn duration(s: &str) -> Result<i64> {
    timeutil::parse_duration(s).context("invalid duration format")
}

/// 1 to 5, from the clock's nanoseconds.
fn random_percent() -> i64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    1 + i64::from(nanos % 5)
}

/// Vietnam time (UTC+7): the zone of every time the user types.
fn vnt() -> FixedOffset {
    FixedOffset::east_opt(7 * 3600).expect("valid offset")
}

/// Start and end of the entry. `date` is the entry's day (today unless --date was given)
/// and `now` the current time; on an explicit --date, "now" means 00:00 VNT that day.
fn entry_times(mode: &Mode, date: NaiveDate, explicit_date: bool, now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    let vnt = vnt();
    let midnight = date
        .and_time(NaiveTime::MIN)
        .and_local_timezone(vnt)
        .single()
        .expect("a fixed offset maps every local time once")
        .with_timezone(&Utc);
    let at = |secs: i64| midnight + TimeDelta::seconds(secs);
    let anchor = if explicit_date { midnight } else { now };
    match *mode {
        Mode::Relative(ago, d) => {
            let start = anchor - TimeDelta::seconds(ago);
            (start, start + TimeDelta::seconds(d))
        }
        Mode::Duration(d) => {
            // Ends on `date` at the anchor's VNT clock time. With --date that is 00:00,
            // so the entry falls on the day before (a follow-up fix in the spec).
            let end = at(i64::from(anchor.with_timezone(&vnt).num_seconds_from_midnight()));
            (end - TimeDelta::seconds(d), end)
        }
        Mode::StartEnd(s, e) => {
            let (start, end) = (at(s), at(e));
            (start, if end < start { end + TimeDelta::days(1) } else { end })
        }
        Mode::StartDuration(s, d) => (at(s), at(s) + TimeDelta::seconds(d)),
    }
}

/// The id of the project's task named `task_name`, created when missing. A dry run
/// never creates it and returns None instead.
fn resolve_task(client: &Client, project_id: &str, task_name: &str, dry_run: bool) -> Result<Option<String>> {
    let tasks = client.list_tasks(project_id).context("failed to list tasks")?;
    if let Some(t) = tasks.into_iter().find(|t| t.name == task_name) {
        return Ok(Some(t.id));
    }
    if dry_run {
        return Ok(None);
    }
    let task = client.create_task(project_id, task_name).context("failed to create task")?;
    Ok(Some(task.id))
}

/// The entry's name: the card number, else the description cut to 50 characters.
fn entry_name(card_number: &str, description: &str) -> String {
    if card_number.is_empty() {
        description.chars().take(50).collect()
    } else {
        card_number.to_string()
    }
}

fn main() {}

#[cfg(test)]
mod tests {
    use super::*;

    const CARD: &str = "https://trello.com/c/abc/12-x";

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn card(mode: Mode) -> LogArgs {
        LogArgs { card_url: Some(CARD.into()), mode }
    }

    fn no_card(mode: Mode) -> LogArgs {
        LogArgs { card_url: None, mode }
    }

    #[test]
    fn card_modes() {
        assert_eq!(parse_log_args(&args(&[CARD, "2h"]), "").unwrap(), card(Mode::Duration(7200)));
        assert_eq!(parse_log_args(&args(&[CARD, "09:00", "11:30"]), "").unwrap(), card(Mode::StartEnd(32400, 41400)));
        assert_eq!(parse_log_args(&args(&[CARD, "09:00", "2h30m"]), "").unwrap(), card(Mode::StartDuration(32400, 9000)));
        assert_eq!(parse_log_args(&args(&[CARD, "09:00", "2:30"]), "").unwrap(), card(Mode::StartDuration(32400, 9000)));
        assert_eq!(parse_log_args(&args(&[CARD, "4h ago", "30m"]), "").unwrap(), card(Mode::Relative(14400, 1800)));
    }

    #[test]
    fn no_card_modes() {
        assert_eq!(parse_log_args(&args(&["2h"]), "standup").unwrap(), no_card(Mode::Duration(7200)));
        assert_eq!(parse_log_args(&args(&["09:00", "11:30"]), "standup").unwrap(), no_card(Mode::StartEnd(32400, 41400)));
        assert_eq!(parse_log_args(&args(&["09:00", "45m"]), "standup").unwrap(), no_card(Mode::StartDuration(32400, 2700)));
        assert_eq!(parse_log_args(&args(&["1h ago", "1h"]), "standup").unwrap(), no_card(Mode::Relative(3600, 3600)));
    }

    #[test]
    fn argument_errors_keep_go_text() {
        let err = |a: &[&str], d: &str| format!("{:#}", parse_log_args(&args(a), d).unwrap_err());
        assert_eq!(err(&[], ""), "invalid arguments: provide a card URL or --description");
        assert_eq!(err(&["2h"], ""), "invalid arguments: expected <duration> or <start> <end>");
        assert_eq!(err(&[CARD, "2x"], ""), "invalid duration format: invalid duration format");
        assert_eq!(err(&[CARD, "09:00", "2x"], ""), "invalid duration format: invalid duration format");
        assert_eq!(err(&[CARD, "a", "b", "c"], ""), "invalid arguments");
        assert_eq!(err(&[], "standup"), "invalid arguments");
        assert_eq!(err(&["2x"], "standup"), "invalid duration format: invalid duration format");
        assert_eq!(err(&["a", "b"], "standup"), "invalid arguments");
        assert_eq!(err(&["a", "b", "c"], "standup"), "invalid arguments");
    }

    #[test]
    fn card_start_without_leading_zero_is_refused() {
        // Go logged this at 00:00 (fix 6).
        assert_eq!(format!("{:#}", parse_log_args(&args(&[CARD, "9:00", "2h"]), "").unwrap_err()), "invalid arguments");
    }

    #[test]
    fn extra_percent_applies_to_durations_only() {
        assert_eq!(Mode::Duration(7200).with_extra(5), Mode::Duration(7560));
        assert_eq!(Mode::StartDuration(32400, 3600).with_extra(1), Mode::StartDuration(32400, 3636));
        assert_eq!(Mode::Relative(14400, 1800).with_extra(3), Mode::Relative(14400, 1854));
        assert_eq!(Mode::StartEnd(32400, 41400).with_extra(5), Mode::StartEnd(32400, 41400));
        assert!((1..=5).contains(&random_percent()));
    }

    fn utc(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 4, 5).unwrap()
    }

    /// 10:00 VNT on the 5th.
    fn now() -> DateTime<Utc> {
        utc("2026-04-05T03:00:00Z")
    }

    #[test]
    fn start_end_in_vnt() {
        assert_eq!(
            entry_times(&Mode::StartEnd(32400, 41400), day(), false, now()),
            (utc("2026-04-05T02:00:00Z"), utc("2026-04-05T04:30:00Z"))
        );
    }

    #[test]
    fn end_before_start_rolls_to_next_day() {
        // 22:00 to 01:00 VNT
        assert_eq!(
            entry_times(&Mode::StartEnd(79200, 3600), day(), false, now()),
            (utc("2026-04-05T15:00:00Z"), utc("2026-04-05T18:00:00Z"))
        );
    }

    #[test]
    fn start_and_duration() {
        assert_eq!(
            entry_times(&Mode::StartDuration(32400, 9000), day(), false, now()),
            (utc("2026-04-05T02:00:00Z"), utc("2026-04-05T04:30:00Z"))
        );
    }

    #[test]
    fn duration_ends_now() {
        assert_eq!(
            entry_times(&Mode::Duration(7200), day(), false, now()),
            (utc("2026-04-05T01:00:00Z"), utc("2026-04-05T03:00:00Z"))
        );
    }

    #[test]
    fn duration_with_explicit_date_ends_at_midnight() {
        // Pins the Go behavior the port keeps: the entry lands on the day before.
        // The Mode A --date follow-up in the spec changes this test.
        assert_eq!(
            entry_times(&Mode::Duration(7200), day(), true, now()),
            (utc("2026-04-04T15:00:00Z"), utc("2026-04-04T17:00:00Z"))
        );
    }

    #[test]
    fn relative_start() {
        assert_eq!(
            entry_times(&Mode::Relative(14400, 1800), day(), false, now()),
            (utc("2026-04-04T23:00:00Z"), utc("2026-04-04T23:30:00Z"))
        );
        // With --date, 4h before 00:00 VNT on the 5th is 20:00 VNT on the 4th.
        assert_eq!(
            entry_times(&Mode::Relative(14400, 1800), day(), true, now()),
            (utc("2026-04-04T13:00:00Z"), utc("2026-04-04T13:30:00Z"))
        );
    }

    fn mock_client(server: &mockito::Server) -> Client {
        let mut c = Client::new("k", "ws");
        c.base_url = server.url();
        c
    }

    #[test]
    fn resolve_task_finds_existing() {
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/workspaces/ws/projects/p1/tasks?page-size=1000")
            .with_body(r##"[{"id":"t1","name":"#12"}]"##)
            .create();
        let create = server.mock("POST", "/workspaces/ws/projects/p1/tasks").expect(0).create();
        assert_eq!(resolve_task(&mock_client(&server), "p1", "#12", false).unwrap(), Some("t1".into()));
        create.assert();
    }

    #[test]
    fn resolve_task_creates_missing() {
        let mut server = mockito::Server::new();
        server.mock("GET", "/workspaces/ws/projects/p1/tasks?page-size=1000").with_body("[]").create();
        let create = server
            .mock("POST", "/workspaces/ws/projects/p1/tasks")
            .match_body(mockito::Matcher::Json(serde_json::json!({"name": "#12"})))
            .with_status(201)
            .with_body(r##"{"id":"t9","name":"#12"}"##)
            .create();
        assert_eq!(resolve_task(&mock_client(&server), "p1", "#12", false).unwrap(), Some("t9".into()));
        create.assert();
    }

    #[test]
    fn dry_run_never_creates_a_task() {
        // Go's --dry-run created the task here (fix 5).
        let mut server = mockito::Server::new();
        server.mock("GET", "/workspaces/ws/projects/p1/tasks?page-size=1000").with_body("[]").create();
        let create = server.mock("POST", "/workspaces/ws/projects/p1/tasks").expect(0).create();
        assert_eq!(resolve_task(&mock_client(&server), "p1", "#12", true).unwrap(), None);
        create.assert();
    }

    #[test]
    fn resolve_task_list_error_keeps_go_text() {
        let mut server = mockito::Server::new();
        server.mock("GET", "/workspaces/ws/projects/p1/tasks?page-size=1000").with_status(500).create();
        let err = resolve_task(&mock_client(&server), "p1", "#12", false).unwrap_err();
        assert_eq!(format!("{err:#}"), "failed to list tasks: API error: status 500");
    }

    #[test]
    fn entry_name_cuts_descriptions_by_character() {
        assert_eq!(entry_name("320", "anything"), "320");
        let vietnamese = "Sửa lỗi đăng nhập ".repeat(5);
        let name = entry_name("", &vietnamese);
        assert_eq!(name.chars().count(), 50);
        assert!(vietnamese.starts_with(&name));
    }
}

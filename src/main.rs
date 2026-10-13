mod clockify;
mod config;
mod summary;
mod timeutil;
mod trello;
mod validator;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, FixedOffset, Local, NaiveDate, NaiveTime, SecondsFormat, TimeDelta, Timelike, Utc};
use clap::{Parser, Subcommand};

use crate::clockify::{Client, TimeEntry, TimeEntryRequest};
use crate::config::Config;

#[derive(Parser)]
#[command(name = "clockify", about = "Log Clockify time entries and view summaries")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Log a time entry against a Trello card, or a description with --project-id
    #[command(override_usage = "clockify log [card-url] <duration>
       clockify log [card-url] <start> <end>
       clockify log [card-url] <start> <duration>
       clockify log [card-url] \"<N> ago\" <duration>
       clockify log --description \"text\" <duration>
       clockify log --description \"text\" <start> <duration>")]
    Log {
        /// [card-url] then <duration>, <start> <end>, <start> <duration>, or "<N> ago" <duration>
        #[arg(value_name = "ARGS")]
        args: Vec<String>,
        /// Date for the entry (default: today)
        #[arg(long, value_name = "YYYY-MM-DD")]
        date: Option<String>,
        /// Print entry details without creating
        #[arg(long)]
        dry_run: bool,
        /// Override default project ID (required with --description)
        #[arg(long, value_name = "ID")]
        project_id: Option<String>,
        /// Task ID for the entry
        #[arg(long, value_name = "ID")]
        task_id: Option<String>,
        /// Description for entry (use instead of card URL)
        #[arg(long, value_name = "TEXT")]
        description: Option<String>,
    },
    /// Show logged time per project for a preset or a date range
    #[command(override_usage = "clockify summary <preset>
       clockify summary --from YYYY-MM-DD --to YYYY-MM-DD")]
    Summary {
        /// today, yesterday, this-week, last-week, this-month, last-month
        preset: Option<String>,
        /// Show per-task breakdown within each project
        #[arg(long)]
        by_task: bool,
        /// Start date (required if no preset)
        #[arg(long, value_name = "YYYY-MM-DD")]
        from: Option<String>,
        /// End date (required if no preset)
        #[arg(long, value_name = "YYYY-MM-DD")]
        to: Option<String>,
    },
}

/// How the log command's positional arguments set the entry's time. Times of day are
/// seconds from midnight in VNT; durations are seconds.
#[derive(Debug, PartialEq)]
enum Mode {
    /// A: ends at the current time of day (on --date when given) and lasts this long.
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

/// 1 to 5, from std's randomly keyed hasher (clock nanos tick in µs on macOS, so % 5 was always 0).
fn random_percent() -> i64 {
    use std::hash::BuildHasher;
    1 + (std::collections::hash_map::RandomState::new().hash_one(()) % 5) as i64
}

/// Vietnam time (UTC+7): the zone of every time the user types.
fn vnt() -> FixedOffset {
    FixedOffset::east_opt(7 * 3600).expect("valid offset")
}

/// Start and end of the entry. `date` is the entry's day (today unless --date was given)
/// and `now` the current time. For a relative start on an explicit --date, "now" means
/// 00:00 VNT that day.
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
            // Ends on `date` at now's VNT clock time. Go used 00:00 on an explicit --date,
            // which put the entry on the day before.
            let end = at(i64::from(now.with_timezone(&vnt).num_seconds_from_midnight()));
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

fn main() {
    let result = match Cli::parse().command {
        Cmd::Log { args, date, dry_run, project_id, task_id, description } => run_log(
            &args,
            date.as_deref().unwrap_or(""),
            dry_run,
            project_id.as_deref().unwrap_or(""),
            task_id.as_deref().unwrap_or(""),
            description.as_deref().unwrap_or(""),
        ),
        Cmd::Summary { preset, by_task, from, to } => run_summary(
            preset.as_deref().unwrap_or(""),
            from.as_deref().unwrap_or(""),
            to.as_deref().unwrap_or(""),
            by_task,
        ),
    };
    if let Err(e) = result {
        eprintln!("Error: {e:#}");
        std::process::exit(1);
    }
}

fn run_log(
    args: &[String],
    date: &str,
    dry_run: bool,
    project_id_override: &str,
    task_id: &str,
    description: &str,
) -> Result<()> {
    let parsed = parse_log_args(args, description)?;
    let mode = parsed.mode.with_extra(random_percent());

    let explicit_date = !date.is_empty();
    let date_str = if explicit_date { date.to_string() } else { Local::now().format("%Y-%m-%d").to_string() };
    let day = timeutil::parse_date(&date_str).context("invalid date format")?;

    let cfg = config::load();
    let card = match &parsed.card_url {
        Some(url) => trello::fetch_card(url, &cfg.trello_api_key, &cfg.trello_token).context("failed to fetch card info")?,
        None => trello::CardInfo { title: description.to_string(), ..Default::default() },
    };

    let now = Utc::now().with_nanosecond(0).unwrap_or_else(Utc::now);
    let (start, end) = entry_times(&mode, day, explicit_date, now);
    let duration = (end - start).num_seconds();

    let mut project_id = project_id_override.to_string();
    if project_id.is_empty() && parsed.card_url.is_some() {
        project_id = cfg.get_clockify_project_id(&card.board_id, &card.board_name);
    }
    if project_id.is_empty() && description.is_empty() {
        bail!("no project ID found (configure board-to-project mapping in config or use --project-id)");
    }
    if project_id.is_empty() {
        bail!("--project-id is required when using --description without a card URL");
    }

    let client = Client::new(&cfg.api_key, &cfg.workspace_id);

    // A task is tied to a card number, so only card entries look one up.
    let task_name = format!("#{}", card.card_number);
    let task = if !task_id.is_empty() {
        Some(task_id.to_string())
    } else if parsed.card_url.is_some() {
        resolve_task(&client, &project_id, &task_name, dry_run)?
    } else {
        None
    };

    match client.entries_between(&cfg.user_id, &date_str, &date_str) {
        Ok(entries) => {
            if let Some(c) = validator::has_overlap(start, end, &entries) {
                let (cs, ce) = c.interval().expect("has_overlap only returns entries with an interval");
                bail!(
                    "Entry overlaps with existing entry {} ({} - {})",
                    conflicting_entry_label(c),
                    cs.format("%H:%M"),
                    ce.format("%H:%M")
                );
            }
            let existing = validator::daily_total(start, &entries);
            if validator::exceeds_cap(existing, duration) {
                bail!(
                    "Total time for {date_str} would be {:.1}h (exceeds 14h limit)",
                    (existing + duration) as f64 / 3600.0
                );
            }
        }
        Err(e) => {
            eprintln!("Warning: Could not fetch existing entries for overlap check: {e:#}");
            eprintln!("Proceeding without overlap validation.");
        }
    }

    let rfc3339 = |t: DateTime<Utc>| t.to_rfc3339_opts(SecondsFormat::Secs, true);

    if dry_run {
        println!("Would create time entry:");
        if card.card_number.is_empty() {
            println!("  Card:        (no ticket)");
        } else {
            println!("  Card:        {}", card.card_number);
        }
        println!("  Description: {}", card.title);
        println!("  Start:       {}", rfc3339(start));
        println!("  End:         {}", rfc3339(end));
        println!("  Duration:    {}", timeutil::format_duration(duration));
        println!("  Project:     {project_id}");
        match &task {
            Some(id) => println!("  Task:        {id}"),
            None if parsed.card_url.is_some() => println!("  Task:        (would create {task_name})"),
            None => {}
        }
        return Ok(());
    }

    let request = TimeEntryRequest {
        project_id: project_id.clone(),
        task_id: task.unwrap_or_default(),
        name: entry_name(&card.card_number, description),
        description: card.title.clone(),
        start: rfc3339(start),
        end: rfc3339(end),
        billable: true,
    };
    let result = client.create_entry(&request).context("Clockify API error")?;

    println!("Created time entry:");
    if card.card_number.is_empty() {
        println!("  Description: {}", result.description);
    } else {
        println!("  Card:        {}", card.card_number);
    }
    println!("  Start:       {}", result.time_interval.start);
    println!("  End:         {}", result.time_interval.end);
    println!("  Duration:    {}", timeutil::format_duration(duration));
    println!("  Project:     {}", result.project_id);
    if !result.task_id.is_empty() {
        println!("  Task:        {}", result.task_id);
    }
    println!(
        "  Link:       https://app.clockify.me/workspaces/{}/time-entries/{}",
        client.workspace_id(),
        result.id
    );

    println!();
    if let Err(e) = print_project_total_today(&client, &cfg, &result.project_id) {
        eprintln!("Warning: Could not fetch project total: {e:#}");
    }
    Ok(())
}

fn run_summary(preset: &str, from: &str, to: &str, by_task: bool) -> Result<()> {
    let (start, end, label) = summary::resolve_date_range(preset, from, to)?;
    let cfg = config::load();
    let client = Client::new(&cfg.api_key, &cfg.workspace_id);
    let entries = client.entries_between(&cfg.user_id, &start, &end).context("Clockify API error")?;
    let projects = client.list_projects().context("Clockify API error")?;

    println!("Summary: {label}\n");
    let mut grand_total = 0;
    if by_task {
        for p in summary::compute_task_breakdown(&entries, &projects) {
            println!("{:<35} {}", p.name, timeutil::format_duration(p.duration));
            for t in &p.tasks {
                println!("  {:<33} {}", t.name, timeutil::format_duration(t.duration));
            }
            grand_total += p.duration;
            println!();
        }
    } else {
        for p in summary::compute_project_totals(&entries, &projects) {
            println!("{:<35} {}", p.name, timeutil::format_duration(p.duration));
            grand_total += p.duration;
        }
    }
    println!("{}", "\u{2500}".repeat(40));
    println!("{:<35} {}", "Total", timeutil::format_duration(grand_total));
    Ok(())
}

/// Prints today's total for the project the new entry went to.
fn print_project_total_today(client: &Client, cfg: &Config, project_id: &str) -> Result<()> {
    let today = Local::now().format("%Y-%m-%d").to_string();
    let entries = client.entries_between(&cfg.user_id, &today, &today)?;
    let projects = client.list_projects()?;
    let totals = summary::compute_project_totals(&entries, &projects);
    let name = projects.iter().find(|p| p.id == project_id).map_or(project_id, |p| p.name.as_str());
    let total = totals.iter().find(|t| t.name == name || t.name == project_id).map_or(0, |t| t.duration);
    println!("Project total today: {name} ({})", timeutil::format_duration(total));
    Ok(())
}

/// Names an entry in error messages: its task, else its description. Clockify's
/// time-entries API has no "name" field.
fn conflicting_entry_label(e: &TimeEntry) -> &str {
    match &e.task {
        Some(t) if !t.name.is_empty() => t.name.as_str(),
        _ if !e.description.is_empty() => e.description.as_str(),
        _ => "(untitled)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use clap::{CommandFactory, Parser};

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn flags_after_positionals() {
        let cli = Cli::try_parse_from(["clockify", "log", CARD, "4h ago", "30m", "--date", "2026-06-27", "--dry-run"]).unwrap();
        let Cmd::Log { args, date, dry_run, .. } = cli.command else { panic!("expected log") };
        assert_eq!(args, vec![CARD, "4h ago", "30m"]);
        assert_eq!(date.as_deref(), Some("2026-06-27"));
        assert!(dry_run);
    }

    #[test]
    fn description_flag_before_times() {
        let cli = Cli::try_parse_from(["clockify", "log", "--description", "fix login", "--project-id", "p1", "2h"]).unwrap();
        let Cmd::Log { args, description, project_id, .. } = cli.command else { panic!("expected log") };
        assert_eq!(args, vec!["2h"]);
        assert_eq!(description.as_deref(), Some("fix login"));
        assert_eq!(project_id.as_deref(), Some("p1"));
    }

    #[test]
    fn summary_flags() {
        let cli = Cli::try_parse_from(["clockify", "summary", "--from", "2026-04-01", "--to", "2026-04-09", "--by-task"]).unwrap();
        let Cmd::Summary { preset, by_task, from, to } = cli.command else { panic!("expected summary") };
        assert_eq!(preset, None);
        assert!(by_task);
        assert_eq!((from.as_deref(), to.as_deref()), (Some("2026-04-01"), Some("2026-04-09")));
    }

    #[test]
    fn unknown_and_single_dash_flags_are_rejected() {
        assert!(Cli::try_parse_from(["clockify", "log", CARD, "2h", "--foo"]).is_err());
        // Go accepted -date (fix 4).
        assert!(Cli::try_parse_from(["clockify", "log", CARD, "2h", "-date", "2026-06-27"]).is_err());
    }

    #[test]
    fn conflicting_entry_label_falls_back() {
        let mut e = clockify::TimeEntry { description: "Standup".into(), ..Default::default() };
        assert_eq!(conflicting_entry_label(&e), "Standup");
        e.task = Some(clockify::TaskRef { name: "#12".into() });
        assert_eq!(conflicting_entry_label(&e), "#12");
        assert_eq!(conflicting_entry_label(&clockify::TimeEntry::default()), "(untitled)");
    }

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

    #[test]
    fn random_percent_varies() {
        // macOS clocks tick in microseconds, so clock nanos % 5 was always 0 (always 1%).
        let seen: std::collections::BTreeSet<i64> = (0..200).map(|_| random_percent()).collect();
        assert!(seen.len() > 1, "{seen:?}");
        assert!(seen.iter().all(|p| (1..=5).contains(p)), "{seen:?}");
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
    fn duration_with_explicit_date_ends_at_current_time_on_that_date() {
        // Go ended the entry at 00:00 of --date, so it landed on the day before.
        // Now: 10:00 VNT on the 5th; --date 2026-04-02 gives 08:00-10:00 VNT on the 2nd.
        let date = NaiveDate::from_ymd_opt(2026, 4, 2).unwrap();
        assert_eq!(
            entry_times(&Mode::Duration(7200), date, true, now()),
            (utc("2026-04-02T01:00:00Z"), utc("2026-04-02T03:00:00Z"))
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

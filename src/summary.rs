//! Date ranges for the summary presets, and per-project / per-task totals.

use std::collections::{BTreeMap, HashMap};

use anyhow::{Result, bail};
use chrono::{Datelike, Local, NaiveDate, TimeDelta};

use crate::clockify::{Project, TimeEntry};

#[derive(Debug, PartialEq)]
pub struct ProjectTotal {
    pub name: String,
    pub duration: i64,
}

#[derive(Debug, PartialEq)]
pub struct TaskTotal {
    pub name: String,
    pub duration: i64,
}

#[derive(Debug, PartialEq)]
pub struct ProjectTasks {
    pub name: String,
    pub duration: i64,
    pub tasks: Vec<TaskTotal>,
}

const PRESETS: &str = "today, yesterday, this-week, last-week, this-month, last-month";

/// Turns a preset, or explicit from/to dates, into start and end dates (YYYY-MM-DD)
/// and a label. A non-empty preset wins over from/to.
pub fn resolve_date_range(preset: &str, from: &str, to: &str) -> Result<(String, String, String)> {
    resolve_date_range_on(preset, from, to, Local::now().date_naive())
}

fn resolve_date_range_on(preset: &str, from: &str, to: &str, today: NaiveDate) -> Result<(String, String, String)> {
    if preset.is_empty() {
        if from.is_empty() || to.is_empty() {
            bail!("provide a preset ({PRESETS}) or --from/--to dates");
        }
        return Ok((from.into(), to.into(), format!("{from} -> {to}")));
    }
    let day = TimeDelta::days(1);
    let monday = today - TimeDelta::days(i64::from(today.weekday().num_days_from_monday()));
    let first_of_month = today.with_day(1).expect("every month has a day 1");
    let span = |name: &str, start: NaiveDate, end: NaiveDate| (start, end, format!("{name} ({start} -> {end})"));
    let (start, end, label) = match preset {
        "today" => (today, today, format!("today ({today})")),
        "yesterday" => {
            let y = today - day;
            (y, y, format!("yesterday ({y})"))
        }
        "this-week" => span("this week", monday, today),
        "last-week" => span("last week", monday - TimeDelta::days(7), monday - day),
        "this-month" => span("this month", first_of_month, today),
        "last-month" => {
            let last = first_of_month - day;
            span("last month", last.with_day(1).expect("every month has a day 1"), last)
        }
        _ => bail!("unknown preset {preset:?} (use: {PRESETS})"),
    };
    Ok((start.to_string(), end.to_string(), label))
}

/// Seconds per project, sorted by project name.
pub fn compute_project_totals(entries: &[TimeEntry], projects: &[Project]) -> Vec<ProjectTotal> {
    let names = project_names(projects);
    let mut totals: BTreeMap<&str, i64> = BTreeMap::new();
    for (e, dur) in counted(entries) {
        *totals.entry(e.project_id.as_str()).or_default() += dur;
    }
    let mut result: Vec<ProjectTotal> = totals
        .into_iter()
        .map(|(id, duration)| ProjectTotal { name: name_of(&names, id), duration })
        .collect();
    result.sort_by(|a, b| a.name.cmp(&b.name));
    result
}

/// Seconds per project and, within each, per task; both sorted by name. A task is named
/// by its Clockify task, else the entry name, else the description, else "(no task)".
pub fn compute_task_breakdown(entries: &[TimeEntry], projects: &[Project]) -> Vec<ProjectTasks> {
    let names = project_names(projects);
    let mut by_project: BTreeMap<&str, BTreeMap<&str, i64>> = BTreeMap::new();
    for (e, dur) in counted(entries) {
        let task = [e.task.as_ref().map_or("", |t| t.name.as_str()), e.name.as_str(), e.description.as_str()]
            .into_iter()
            .find(|n| !n.is_empty())
            .unwrap_or("(no task)");
        *by_project.entry(e.project_id.as_str()).or_default().entry(task).or_default() += dur;
    }
    let mut result: Vec<ProjectTasks> = by_project
        .into_iter()
        .map(|(id, tasks)| ProjectTasks {
            name: name_of(&names, id),
            duration: tasks.values().sum(),
            tasks: tasks.into_iter().map(|(name, duration)| TaskTotal { name: name.into(), duration }).collect(),
        })
        .collect();
    result.sort_by(|a, b| a.name.cmp(&b.name));
    result
}

/// Entries that count toward totals, with their seconds: a project, a parseable start
/// and end (not a running timer), and at most 24h (longer rows are ranges, not entries).
fn counted(entries: &[TimeEntry]) -> impl Iterator<Item = (&TimeEntry, i64)> {
    entries.iter().filter(|e| !e.project_id.is_empty()).filter_map(|e| {
        let (start, end) = e.interval()?;
        let dur = (end - start).num_seconds();
        (dur != 0 && dur <= 24 * 3600).then_some((e, dur))
    })
}

fn project_names(projects: &[Project]) -> HashMap<&str, &str> {
    projects.iter().map(|p| (p.id.as_str(), p.name.as_str())).collect()
}

/// The project's name, or its id when the name is unknown or empty.
fn name_of(names: &HashMap<&str, &str>, id: &str) -> String {
    names.get(id).filter(|n| !n.is_empty()).copied().unwrap_or(id).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clockify::{TaskRef, TimeInterval};

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    fn entry(project: &str, start: &str, end: &str) -> TimeEntry {
        TimeEntry {
            project_id: project.into(),
            time_interval: TimeInterval { start: start.into(), end: end.into() },
            ..Default::default()
        }
    }

    fn project(id: &str, name: &str) -> Project {
        Project { id: id.into(), name: name.into() }
    }

    fn range(preset: &str, today: &str) -> (String, String, String) {
        resolve_date_range_on(preset, "", "", d(today)).unwrap()
    }

    fn r(start: &str, end: &str, label: &str) -> (String, String, String) {
        (start.to_string(), end.to_string(), label.to_string())
    }

    #[test]
    fn today_preset_uses_local_date() {
        let (start, end, label) = resolve_date_range("today", "", "").unwrap();
        let today = Local::now().date_naive().to_string();
        assert_eq!((start.as_str(), end.as_str()), (today.as_str(), today.as_str()));
        assert_eq!(label, format!("today ({today})"));
    }

    #[test]
    fn presets_on_a_wednesday() {
        let wed = "2026-10-14";
        assert_eq!(range("yesterday", wed), r("2026-10-13", "2026-10-13", "yesterday (2026-10-13)"));
        assert_eq!(range("this-week", wed), r("2026-10-12", wed, "this week (2026-10-12 -> 2026-10-14)"));
        assert_eq!(range("last-week", wed), r("2026-10-05", "2026-10-11", "last week (2026-10-05 -> 2026-10-11)"));
        assert_eq!(range("this-month", wed), r("2026-10-01", wed, "this month (2026-10-01 -> 2026-10-14)"));
        assert_eq!(range("last-month", wed), r("2026-09-01", "2026-09-30", "last month (2026-09-01 -> 2026-09-30)"));
    }

    #[test]
    fn week_edges_and_january() {
        // Sunday belongs to the week that started the Monday before.
        assert_eq!(range("this-week", "2026-10-18").0, "2026-10-12");
        // On a Monday the range is one day but still prints as a range, as in Go.
        assert_eq!(range("this-week", "2026-10-12").2, "this week (2026-10-12 -> 2026-10-12)");
        assert_eq!(range("last-month", "2026-01-15"), r("2025-12-01", "2025-12-31", "last month (2025-12-01 -> 2025-12-31)"));
    }

    #[test]
    fn custom_range_returns_bare_dates() {
        // Go returned "2026-04-01T00:00:00Z" here, and the user endpoint appended the time again (fix 1).
        assert_eq!(
            resolve_date_range("", "2026-04-01", "2026-04-09").unwrap(),
            r("2026-04-01", "2026-04-09", "2026-04-01 -> 2026-04-09")
        );
    }

    #[test]
    fn preset_wins_over_dates() {
        assert_eq!(resolve_date_range_on("today", "2026-04-01", "2026-04-09", d("2026-10-14")).unwrap().0, "2026-10-14");
    }

    #[test]
    fn range_errors() {
        assert_eq!(
            resolve_date_range("", "", "").unwrap_err().to_string(),
            "provide a preset (today, yesterday, this-week, last-week, this-month, last-month) or --from/--to dates"
        );
        assert_eq!(
            resolve_date_range("", "2026-04-01", "").unwrap_err().to_string(),
            "provide a preset (today, yesterday, this-week, last-week, this-month, last-month) or --from/--to dates"
        );
        assert_eq!(
            resolve_date_range("tomorrow", "", "").unwrap_err().to_string(),
            "unknown preset \"tomorrow\" (use: today, yesterday, this-week, last-week, this-month, last-month)"
        );
    }

    #[test]
    fn project_totals_sorted_by_name() {
        let entries = [
            entry("proj1", "2026-04-09T09:00:00Z", "2026-04-09T12:00:00Z"),
            entry("proj2", "2026-04-09T13:00:00Z", "2026-04-09T15:00:00Z"),
            entry("proj1", "2026-04-09T16:00:00Z", "2026-04-09T17:30:00Z"),
        ];
        let projects = [project("proj2", "Beta"), project("proj1", "Alpha")];
        assert_eq!(
            compute_project_totals(&entries, &projects),
            vec![
                ProjectTotal { name: "Alpha".into(), duration: 16200 },
                ProjectTotal { name: "Beta".into(), duration: 7200 },
            ]
        );
    }

    #[test]
    fn project_totals_skip_running_timers_missing_projects_and_long_rows() {
        let entries = [
            entry("proj1", "2026-04-09T09:00:00Z", ""),
            entry("proj1", "2026-04-09T10:00:00Z", "2026-04-09T11:00:00Z"),
            entry("", "2026-04-09T12:00:00Z", "2026-04-09T13:00:00Z"),
            entry("proj1", "2026-04-01T00:00:00Z", "2026-04-09T00:00:00Z"),
        ];
        // No project list: the id stands in for the name.
        assert_eq!(compute_project_totals(&entries, &[]), vec![ProjectTotal { name: "proj1".into(), duration: 3600 }]);
    }

    #[test]
    fn task_breakdown_names_and_order() {
        let mut feature = entry("proj1", "2026-04-09T09:00:00Z", "2026-04-09T12:00:00Z");
        feature.name = "TASK-101".into();
        feature.task = Some(TaskRef { name: "Feature X".into() });
        let mut bug = entry("proj1", "2026-04-09T13:00:00Z", "2026-04-09T14:30:00Z");
        bug.task = Some(TaskRef { name: "Bug fix".into() });
        let mut unplanned = entry("proj1", "2026-04-09T15:00:00Z", "2026-04-09T17:00:00Z");
        unplanned.name = " unplanned".into();
        let mut described = entry("proj1", "2026-04-09T17:00:00Z", "2026-04-09T17:30:00Z");
        described.description = "Standup".into();
        let bare = entry("proj1", "2026-04-09T18:00:00Z", "2026-04-09T18:15:00Z");

        let breakdown = compute_task_breakdown(&[feature, bug, unplanned, described, bare], &[project("proj1", "Alpha")]);

        assert_eq!(breakdown.len(), 1);
        assert_eq!(breakdown[0].name, "Alpha");
        assert_eq!(breakdown[0].duration, 3 * 3600 + 5400 + 2 * 3600 + 1800 + 900);
        let names: Vec<&str> = breakdown[0].tasks.iter().map(|t| t.name.as_str()).collect();
        // Byte order: space, "(", then capitals.
        assert_eq!(names, vec![" unplanned", "(no task)", "Bug fix", "Feature X", "Standup"]);
        assert_eq!(breakdown[0].tasks[3].duration, 3 * 3600);
    }
}

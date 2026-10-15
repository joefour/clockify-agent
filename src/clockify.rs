//! Clockify REST client: time entries, projects, tasks and the detailed report.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::json;

pub(crate) const BASE_URL: &str = "https://api.clockify.me/api/v1";

/// The agent every HTTP call uses: 10 s timeout, and 4xx/5xx returned as responses
/// so callers can report the status.
pub fn http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .http_status_as_error(false)
        .build()
        .into()
}

/// Reads JSON `null` as the type's default. Clockify sends null for unset fields such
/// as `taskId`, and for a running timer's `end`.
pub fn null_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeEntryRequest {
    pub project_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub task_id: String,
    pub name: String,
    pub description: String,
    pub start: String,
    pub end: String,
    pub billable: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TimeInterval {
    #[serde(default, deserialize_with = "null_default")]
    pub start: String,
    #[serde(default, deserialize_with = "null_default")]
    pub end: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TaskRef {
    #[serde(default, deserialize_with = "null_default")]
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Task {
    #[serde(default, deserialize_with = "null_default")]
    pub id: String,
    #[serde(default, deserialize_with = "null_default")]
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeEntry {
    #[serde(default, deserialize_with = "null_default")]
    pub id: String,
    #[serde(default, deserialize_with = "null_default")]
    pub project_id: String,
    #[serde(default, deserialize_with = "null_default")]
    pub task_id: String,
    #[serde(default, deserialize_with = "null_default")]
    pub name: String,
    #[serde(default, deserialize_with = "null_default")]
    pub description: String,
    #[serde(default, deserialize_with = "null_default")]
    pub time_interval: TimeInterval,
    #[serde(default)]
    pub task: Option<TaskRef>,
}

impl TimeEntry {
    /// Start and end as UTC instants; None when either is missing or malformed,
    /// as for a running timer.
    pub fn interval(&self) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        let parse = |s: &str| DateTime::parse_from_rfc3339(s).ok().map(|t| t.with_timezone(&Utc));
        Some((parse(&self.time_interval.start)?, parse(&self.time_interval.end)?))
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Project {
    #[serde(default, deserialize_with = "null_default")]
    pub id: String,
    #[serde(default, deserialize_with = "null_default")]
    pub name: String,
}

#[derive(Deserialize)]
struct DetailedReport {
    #[serde(default, deserialize_with = "null_default")]
    timeentries: Vec<TimeEntry>,
}

pub struct Client {
    api_key: String,
    workspace_id: String,
    pub(crate) base_url: String,
    agent: ureq::Agent,
}

impl Client {
    pub fn new(api_key: &str, workspace_id: &str) -> Self {
        Client {
            api_key: api_key.into(),
            workspace_id: workspace_id.into(),
            base_url: BASE_URL.into(),
            agent: http_agent(),
        }
    }

    pub fn workspace_id(&self) -> &str {
        &self.workspace_id
    }

    pub fn create_entry(&self, entry: &TimeEntryRequest) -> Result<TimeEntry> {
        self.post(&format!("/workspaces/{}/time-entries", self.workspace_id), entry)
    }

    /// The user's entries from `start` 00:00:00Z to `end` 23:59:59Z (dates as YYYY-MM-DD).
    pub fn list_entries_for_user(&self, user_id: &str, start: &str, end: &str) -> Result<Vec<TimeEntry>> {
        // shortcut: one page of 1000 entries (the API default is 50); page through if a range ever holds more
        self.get(&format!(
            "/workspaces/{}/user/{user_id}/time-entries?start={start}T00:00:00Z&end={end}T23:59:59Z&page-size=1000",
            self.workspace_id
        ))
    }

    /// Entries from the detailed report API; `start` and `end` are full RFC 3339 times.
    /// The endpoint needs special permissions and may return 403.
    pub fn get_detailed_report(&self, start: &str, end: &str) -> Result<Vec<TimeEntry>> {
        let body = json!({
            "dateRangeStart": start,
            "dateRangeEnd": end,
            "detailedFilter": {"groups": ["PROJECT"]},
            "users": {"contains": "ALL"},
            "projects": {"contains": "ALL"},
        });
        let report: DetailedReport =
            self.post(&format!("/report/workspaces/{}/reports/detailed", self.workspace_id), &body)?;
        Ok(report.timeentries)
    }

    /// Entries for the dates `start` to `end` (YYYY-MM-DD, inclusive): the user's own
    /// when `user_id` is set, the detailed report otherwise.
    pub fn entries_between(&self, user_id: &str, start: &str, end: &str) -> Result<Vec<TimeEntry>> {
        if user_id.is_empty() {
            self.get_detailed_report(&format!("{start}T00:00:00Z"), &format!("{end}T23:59:59Z"))
        } else {
            self.list_entries_for_user(user_id, start, end)
        }
    }

    pub fn list_projects(&self) -> Result<Vec<Project>> {
        // page-size: the API default is 50 projects
        self.get(&format!("/workspaces/{}/projects?page-size=1000", self.workspace_id))
    }

    pub fn list_tasks(&self, project_id: &str) -> Result<Vec<Task>> {
        // page-size: API default is 50, so older tasks were missed and re-created (400 duplicate)
        self.get(&format!("/workspaces/{}/projects/{project_id}/tasks?page-size=1000", self.workspace_id))
    }

    pub fn create_task(&self, project_id: &str, name: &str) -> Result<Task> {
        self.post(
            &format!("/workspaces/{}/projects/{project_id}/tasks", self.workspace_id),
            &json!({"name": name}),
        )
    }

    fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        decode(self.agent.get(format!("{}{path}", self.base_url)).header("X-Api-Key", &self.api_key).call())
    }

    fn post<T: DeserializeOwned>(&self, path: &str, body: &impl Serialize) -> Result<T> {
        decode(
            self.agent
                .post(format!("{}{path}", self.base_url))
                .header("X-Api-Key", &self.api_key)
                .send_json(body),
        )
    }
}

/// The decoded body of a ureq result, with the Go client's error texts.
fn decode<T: DeserializeOwned>(resp: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Result<T> {
    let mut resp = resp.context("failed to send request")?;
    let status = resp.status().as_u16();
    if status >= 400 {
        bail!("API error: status {status}");
    }
    resp.body_mut().read_json().context("failed to decode response")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Matcher;

    fn client(server: &mockito::Server) -> Client {
        let mut c = Client::new("test-api-key", "ws123");
        c.base_url = server.url();
        c
    }

    fn sample_request() -> TimeEntryRequest {
        TimeEntryRequest {
            project_id: "project456".into(),
            task_id: String::new(),
            name: "Test Entry".into(),
            description: "Test Description".into(),
            start: "2026-04-05T09:00:00Z".into(),
            end: "2026-04-05T10:00:00Z".into(),
            billable: true,
        }
    }

    #[test]
    fn create_entry_success() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/workspaces/ws123/time-entries")
            .match_header("x-api-key", "test-api-key")
            .match_header("content-type", Matcher::Regex("application/json".into()))
            // Exact body: taskId must be left out when empty.
            .match_body(Matcher::Json(json!({
                "projectId": "project456",
                "name": "Test Entry",
                "description": "Test Description",
                "start": "2026-04-05T09:00:00Z",
                "end": "2026-04-05T10:00:00Z",
                "billable": true
            })))
            .with_status(201)
            .with_body(r#"{"id":"entry123","projectId":"project456","taskId":null,"description":"Test Description","timeInterval":{"start":"2026-04-05T09:00:00Z","end":"2026-04-05T10:00:00Z"}}"#)
            .create();
        let result = client(&server).create_entry(&sample_request()).unwrap();
        mock.assert();
        assert_eq!(result.id, "entry123");
        assert_eq!(result.project_id, "project456");
        assert_eq!(result.task_id, "");
    }

    #[test]
    fn create_entry_api_error() {
        let mut server = mockito::Server::new();
        server
            .mock("POST", "/workspaces/ws123/time-entries")
            .with_status(400)
            .with_body(r#"{"message":"Invalid request"}"#)
            .create();
        let err = client(&server).create_entry(&sample_request()).unwrap_err();
        assert_eq!(err.to_string(), "API error: status 400");
    }

    #[test]
    fn unreachable_server_is_a_send_error() {
        let mut c = Client::new("k", "ws123");
        c.base_url = "http://127.0.0.1:1".into();
        let err = c.list_projects().unwrap_err();
        assert!(format!("{err:#}").starts_with("failed to send request"), "{err:#}");
    }

    #[test]
    fn list_entries_for_user_sends_day_bounds_and_page_size() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/workspaces/ws123/user/u1/time-entries")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("start".into(), "2026-04-05T00:00:00Z".into()),
                Matcher::UrlEncoded("end".into(), "2026-04-06T23:59:59Z".into()),
                Matcher::UrlEncoded("page-size".into(), "1000".into()),
            ]))
            .with_body(
                r#"[{"id":"entry1","projectId":"project1","taskId":null,"timeInterval":{"start":"2026-04-05T09:00:00Z","end":"2026-04-05T10:00:00Z"}},
                    {"id":"entry2","projectId":null,"description":null,"timeInterval":{"start":"2026-04-05T11:00:00Z","end":null}}]"#,
            )
            .create();
        let entries = client(&server).list_entries_for_user("u1", "2026-04-05", "2026-04-06").unwrap();
        mock.assert();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "entry1");
        assert_eq!(entries[0].task_id, "");
        assert_eq!(entries[1].project_id, "");
        assert_eq!(entries[1].description, "");
        assert_eq!(entries[1].time_interval.end, "");
    }

    #[test]
    fn entries_between_with_user_uses_user_endpoint() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/workspaces/ws123/user/u1/time-entries")
            .match_query(Matcher::UrlEncoded("start".into(), "2026-04-01T00:00:00Z".into()))
            .with_body("[]")
            .create();
        assert!(client(&server).entries_between("u1", "2026-04-01", "2026-04-09").unwrap().is_empty());
        mock.assert();
    }

    #[test]
    fn entries_between_without_user_uses_detailed_report() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/report/workspaces/ws123/reports/detailed")
            .match_body(Matcher::PartialJson(json!({
                "dateRangeStart": "2026-04-01T00:00:00Z",
                "dateRangeEnd": "2026-04-09T23:59:59Z",
                "detailedFilter": {"groups": ["PROJECT"]},
                "users": {"contains": "ALL"},
                "projects": {"contains": "ALL"}
            })))
            .with_body(r#"{"timeentries":[{"id":"e1","projectId":"p1","timeInterval":{"start":"2026-04-01T09:00:00Z","end":"2026-04-01T10:00:00Z"}}]}"#)
            .create();
        let entries = client(&server).entries_between("", "2026-04-01", "2026-04-09").unwrap();
        mock.assert();
        assert_eq!(entries[0].id, "e1");
    }

    #[test]
    fn list_projects_decodes() {
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/workspaces/ws123/projects")
            .match_query(Matcher::UrlEncoded("page-size".into(), "1000".into()))
            .with_body(r#"[{"id":"proj1","name":"Project 1"},{"id":"proj2","name":"Project 2"}]"#)
            .create();
        let projects = client(&server).list_projects().unwrap();
        assert_eq!(projects.len(), 2);
        assert_eq!((projects[1].id.as_str(), projects[1].name.as_str()), ("proj2", "Project 2"));
    }

    #[test]
    fn list_tasks_and_create_task() {
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/workspaces/ws123/projects/p1/tasks?page-size=1000")
            .with_body(r##"[{"id":"t1","name":"#12","projectId":"p1"}]"##)
            .create();
        let create = server
            .mock("POST", "/workspaces/ws123/projects/p1/tasks")
            .match_body(Matcher::Json(json!({"name": "#13"})))
            .with_status(201)
            .with_body(r##"{"id":"t2","name":"#13"}"##)
            .create();
        let c = client(&server);
        let tasks = c.list_tasks("p1").unwrap();
        assert_eq!((tasks[0].id.as_str(), tasks[0].name.as_str()), ("t1", "#12"));
        assert_eq!(c.create_task("p1", "#13").unwrap().id, "t2");
        create.assert();
    }

    #[test]
    fn bad_json_is_a_decode_error() {
        let mut server = mockito::Server::new();
        server.mock("GET", "/workspaces/ws123/projects").match_query(Matcher::Any).with_body("not json").create();
        let err = client(&server).list_projects().unwrap_err();
        assert!(format!("{err:#}").starts_with("failed to decode response"), "{err:#}");
    }

    #[test]
    fn interval_parses_to_utc_and_skips_running_timers() {
        let mut e = TimeEntry {
            time_interval: TimeInterval {
                start: "2026-04-05T16:00:00+07:00".into(),
                end: "2026-04-05T10:00:00Z".into(),
            },
            ..Default::default()
        };
        let (start, end) = e.interval().unwrap();
        assert_eq!(start, "2026-04-05T09:00:00Z".parse::<DateTime<Utc>>().unwrap());
        assert_eq!(end, "2026-04-05T10:00:00Z".parse::<DateTime<Utc>>().unwrap());
        e.time_interval.end = String::new();
        assert!(e.interval().is_none());
    }
}

//! `clockify serve`: the dashboard. Two loopback listeners share one router; a guard in
//! front of each checks Host, Origin and, on the tailnet one, the Tailscale login.

use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::extract::{Query, Request, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::response::sse::{self, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::Stream;
use serde::Deserialize;
use serde_json::json;
use tokio::net::TcpListener;
use tokio::signal::unix::{SignalKind, signal};

use crate::ask::{Ask, StartError};
use crate::clockify::{self, Client, Project};
use crate::config::{self, Config};
use crate::trello::{self, CardHit};
use crate::{LogOutcome, LogRequest, SummaryOutcome, log_entry, now_secs, random_percent, summarize};

/// The Mac's own listener: the tray's Dashboard button opens it.
pub const LOCAL_PORT: u16 = 8767;
/// The listener `tailscale serve --https=8447` forwards to.
pub const TAILNET_PORT: u16 = 8768;
const PAGE: &str = include_str!("web/index.html");

/// Who may use one listener.
#[derive(Clone, Debug)]
pub enum Gate {
    /// 127.0.0.1:8767: any browser on this Mac.
    Local,
    /// 127.0.0.1:8768 behind `tailscale serve`: only `owner`, through `host` (dns-name:8447).
    Tailnet { owner: String, host: String },
}

/// Why a request is refused, or None to let it through. Host stops DNS rebinding, a
/// same-origin JSON POST stops other sites driving Ask Claude, and the login header
/// (set by `tailscale serve`, which drops any the client sent) keeps other tailnet users out.
pub fn refusal(gate: &Gate, method: &Method, headers: &HeaderMap) -> Option<&'static str> {
    let h = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or("");
    let host = h("host");
    let origin = match gate {
        Gate::Local => {
            if host != format!("127.0.0.1:{LOCAL_PORT}") && host != format!("localhost:{LOCAL_PORT}") {
                return Some("wrong host");
            }
            format!("http://{host}")
        }
        Gate::Tailnet { owner, host: tailnet_host } => {
            if owner.is_empty() || h("tailscale-user-login") != owner {
                return Some("not the owner");
            }
            // tailscale serve may pass the client's Host or the backend's.
            if host != tailnet_host && host != format!("127.0.0.1:{TAILNET_PORT}") {
                return Some("wrong host");
            }
            format!("https://{tailnet_host}")
        }
    };
    if *method != Method::GET && *method != Method::HEAD {
        if h("origin") != origin {
            return Some("wrong origin");
        }
        if !h("content-type").starts_with("application/json") {
            return Some("not JSON");
        }
    }
    None
}

async fn guard(State(gate): State<Arc<Gate>>, req: Request, next: Next) -> Response {
    match refusal(&gate, req.method(), req.headers()) {
        Some(why) => {
            eprintln!("refused {} {}: {why}", req.method(), req.uri());
            (StatusCode::FORBIDDEN, Json(json!({ "error": why }))).into_response()
        }
        None => next.run(req).await,
    }
}

/// `app` behind the guard for one listener.
pub fn gated(app: Router, gate: Gate) -> Router {
    app.layer(middleware::from_fn_with_state(Arc::new(gate), guard))
}

/// What every handler shares. The bases are the real APIs, except in tests.
pub struct AppState {
    pub cfg: Config,
    pub clockify_base: String,
    pub trello_base: String,
    pub ask: Arc<Ask>,
}

impl AppState {
    fn client(&self) -> Client {
        let mut c = Client::new(&self.cfg.api_key, &self.cfg.workspace_id);
        c.base_url = self.clockify_base.clone();
        c
    }
}

type Shared = State<Arc<AppState>>;

/// An error the page shows under the card it came from.
struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError(StatusCode::BAD_REQUEST, format!("{e:#}"))
    }
}

/// Runs the CLI's blocking ureq code off the async threads.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(ApiError::from)
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(|| async { Html(PAGE) }))
        .route("/api/totals", get(totals))
        .route("/api/search", get(search))
        .route("/api/projects", get(projects))
        .route("/api/log", post(log))
        .route("/api/ask", post(ask_start))
        .route("/api/ask/events", get(ask_events))
        .route("/api/ask/stop", post(ask_stop))
        .with_state(state)
}

#[derive(Deserialize)]
struct TotalsQuery {
    preset: String,
}

async fn totals(State(s): Shared, Query(q): Query<TotalsQuery>) -> Result<Json<SummaryOutcome>, ApiError> {
    if !["today", "this-week", "this-month"].contains(&q.preset.as_str()) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "preset must be today, this-week or this-month".into()));
    }
    Ok(Json(blocking(move || summarize(&s.cfg, &s.client(), &q.preset, "", "")).await?))
}

#[derive(Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: String,
}

/// Under 2 characters searches nothing, so the first keystroke costs no Trello call.
async fn search(State(s): Shared, Query(q): Query<SearchQuery>) -> Result<Json<Vec<CardHit>>, ApiError> {
    let q = q.q.trim().to_string();
    if q.chars().count() < 2 {
        return Ok(Json(Vec::new()));
    }
    Ok(Json(
        blocking(move || trello::search_cards(&s.trello_base, &q, &s.cfg.trello_api_key, &s.cfg.trello_token)).await?,
    ))
}

async fn projects(State(s): Shared) -> Result<Json<Vec<Project>>, ApiError> {
    Ok(Json(blocking(move || s.client().list_projects().context("Clockify API error")).await?))
}

/// The page's log form: a card URL, or a description with a project, plus the CLI's
/// positional time arguments.
#[derive(Deserialize)]
struct LogBody {
    #[serde(default)]
    card_url: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    project_id: String,
    args: Vec<String>,
    #[serde(default)]
    date: String,
    #[serde(default)]
    dry_run: bool,
}

async fn log(State(s): Shared, Json(b): Json<LogBody>) -> Result<Json<LogOutcome>, ApiError> {
    let mut args = b.args;
    if !b.card_url.is_empty() {
        args.insert(0, b.card_url);
    }
    let req = LogRequest {
        args,
        date: b.date,
        dry_run: b.dry_run,
        project_id: b.project_id,
        task_id: String::new(),
        description: b.description,
    };
    let outcome = blocking(move || {
        let fetch = |url: &str| trello::fetch_card(url, &s.cfg.trello_api_key, &s.cfg.trello_token);
        log_entry(&s.cfg, &s.client(), &req, fetch, now_secs(), random_percent())
    })
    .await?;
    eprintln!("logged{}: {} {} ({}s)", if outcome.dry_run { " (dry run)" } else { "" }, outcome.description, outcome.start, outcome.duration);
    Ok(Json(outcome))
}

#[derive(Deserialize)]
struct AskBody {
    prompt: String,
}

async fn ask_start(State(s): Shared, Json(b): Json<AskBody>) -> Result<StatusCode, ApiError> {
    if b.prompt.trim().is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "type a prompt first".into()));
    }
    match s.ask.start(b.prompt) {
        Ok(()) => Ok(StatusCode::ACCEPTED),
        Err(StartError::Busy) => Err(ApiError(StatusCode::CONFLICT, "a run is already going".into())),
        Err(StartError::Spawn(e)) => Err(ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("could not start claude: {e}"))),
    }
}

async fn ask_events(State(s): Shared) -> Sse<impl Stream<Item = Result<sse::Event, Infallible>>> {
    let events = futures_util::stream::unfold(s.ask.follow(), |mut rx| async move {
        let e = rx.recv().await?;
        Some((Ok(sse::Event::default().json_data(&e).expect("events serialize")), rx))
    });
    Sse::new(events).keep_alive(sse::KeepAlive::default())
}

async fn ask_stop(State(s): Shared) -> StatusCode {
    s.ask.stop();
    StatusCode::ACCEPTED
}

/// Serves until SIGTERM or Ctrl-C. Ask Claude runs start in `repo`.
pub fn run(repo: PathBuf, owner: String, tailnet_host: String) -> Result<()> {
    tokio::runtime::Runtime::new().context("could not start the async runtime")?.block_on(serve(repo, owner, tailnet_host))
}

async fn serve(repo: PathBuf, owner: String, tailnet_host: String) -> Result<()> {
    let local = TcpListener::bind(("127.0.0.1", LOCAL_PORT))
        .await
        .with_context(|| format!("could not listen on 127.0.0.1:{LOCAL_PORT}"))?;
    let tailnet = TcpListener::bind(("127.0.0.1", TAILNET_PORT))
        .await
        .with_context(|| format!("could not listen on 127.0.0.1:{TAILNET_PORT}"))?;
    if owner.is_empty() {
        eprintln!("no --owner: the tailnet listener refuses every request");
    }
    eprintln!("dashboard on http://127.0.0.1:{LOCAL_PORT}; tailnet https://{tailnet_host} for {owner}");
    let state = Arc::new(AppState {
        cfg: config::load(),
        clockify_base: clockify::BASE_URL.into(),
        trello_base: trello::TRELLO_API.into(),
        ask: Arc::new(Ask::new("claude", repo)),
    });
    let ask = state.ask.clone();
    let local = axum::serve(local, gated(router(state.clone()), Gate::Local));
    let tailnet = axum::serve(tailnet, gated(router(state), Gate::Tailnet { owner, host: tailnet_host }));
    let mut term = signal(SignalKind::terminate()).context("could not watch SIGTERM")?;
    let served = tokio::select! {
        r = async { tokio::try_join!(async { local.await }, async { tailnet.await }) } => r.map(|_| ()),
        _ = term.recv() => Ok(()),
        _ = tokio::signal::ctrl_c() => Ok(()),
    };
    // A run would outlive the server: claude has its own process group and approvals off.
    ask.shutdown().await;
    served?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{HeaderName, HeaderValue};
    use tower::ServiceExt;

    const ME: &str = "me@example.com";

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(HeaderName::from_bytes(k.as_bytes()).unwrap(), HeaderValue::from_str(v).unwrap());
        }
        h
    }

    fn tailnet(owner: &str) -> Gate {
        Gate::Tailnet { owner: owner.into(), host: "mac.tail.ts.net:8447".into() }
    }

    #[test]
    fn local_needs_its_own_host() {
        assert_eq!(refusal(&Gate::Local, &Method::GET, &headers(&[("host", "127.0.0.1:8767")])), None);
        assert_eq!(refusal(&Gate::Local, &Method::GET, &headers(&[("host", "localhost:8767")])), None);
        assert_eq!(refusal(&Gate::Local, &Method::GET, &headers(&[("host", "evil.example:8767")])), Some("wrong host"));
        assert_eq!(refusal(&Gate::Local, &Method::GET, &headers(&[])), Some("wrong host"));
    }

    #[test]
    fn posts_need_the_same_origin_and_json() {
        let host = ("host", "127.0.0.1:8767");
        let origin = ("origin", "http://127.0.0.1:8767");
        let json = ("content-type", "application/json");
        assert_eq!(refusal(&Gate::Local, &Method::POST, &headers(&[host, origin, json])), None);
        assert_eq!(refusal(&Gate::Local, &Method::POST, &headers(&[host, ("origin", "https://evil.example"), json])), Some("wrong origin"));
        assert_eq!(refusal(&Gate::Local, &Method::POST, &headers(&[host, json])), Some("wrong origin"));
        assert_eq!(refusal(&Gate::Local, &Method::POST, &headers(&[host, origin, ("content-type", "text/plain")])), Some("not JSON"));
    }

    #[test]
    fn tailnet_needs_the_owner() {
        let host = ("host", "mac.tail.ts.net:8447");
        assert_eq!(refusal(&tailnet(ME), &Method::GET, &headers(&[host, ("tailscale-user-login", ME)])), None);
        assert_eq!(
            refusal(&tailnet(ME), &Method::GET, &headers(&[host, ("tailscale-user-login", "other@example.com")])),
            Some("not the owner")
        );
        assert_eq!(refusal(&tailnet(ME), &Method::GET, &headers(&[host])), Some("not the owner"), "tagged devices send no login");
        assert_eq!(refusal(&tailnet(""), &Method::GET, &headers(&[host, ("tailscale-user-login", "")])), Some("not the owner"));
    }

    #[test]
    fn tailnet_host_is_the_tailnet_name_or_the_backend() {
        let me = ("tailscale-user-login", ME);
        assert_eq!(refusal(&tailnet(ME), &Method::GET, &headers(&[("host", "127.0.0.1:8768"), me])), None);
        assert_eq!(refusal(&tailnet(ME), &Method::GET, &headers(&[("host", "evil.example:8768"), me])), Some("wrong host"));
        let host = ("host", "127.0.0.1:8768");
        let json = ("content-type", "application/json");
        assert_eq!(refusal(&tailnet(ME), &Method::POST, &headers(&[host, me, ("origin", "https://mac.tail.ts.net:8447"), json])), None);
        assert_eq!(refusal(&tailnet(ME), &Method::POST, &headers(&[host, me, ("origin", "http://127.0.0.1:8768"), json])), Some("wrong origin"));
    }

    #[tokio::test]
    async fn the_gate_answers_403_before_any_route() {
        let app = gated(router(state("http://127.0.0.1:1", "http://127.0.0.1:1")), Gate::Local);
        let bad = Request::get("/").header("host", "evil.example:8767").body(Body::empty()).unwrap();
        assert_eq!(app.clone().oneshot(bad).await.unwrap().status(), StatusCode::FORBIDDEN);
        let ok = Request::get("/").header("host", "127.0.0.1:8767").body(Body::empty()).unwrap();
        assert_eq!(app.oneshot(ok).await.unwrap().status(), StatusCode::OK);
    }

    use crate::config::Config;

    fn state(clockify: &str, trello: &str) -> Arc<AppState> {
        Arc::new(AppState {
            cfg: Config {
                api_key: "k".into(),
                workspace_id: "ws123".into(),
                user_id: "u1".into(),
                trello_api_key: "tk".into(),
                trello_token: "tt".into(),
                ..Default::default()
            },
            clockify_base: clockify.into(),
            trello_base: trello.into(),
            ask: Arc::new(Ask::new("claude", PathBuf::from("/tmp"))),
        })
    }

    fn local_get(uri: &str) -> Request<Body> {
        Request::get(uri).header("host", "127.0.0.1:8767").body(Body::empty()).unwrap()
    }

    fn local_post(uri: &str, body: serde_json::Value) -> Request<Body> {
        Request::post(uri)
            .header("host", "127.0.0.1:8767")
            .header("origin", "http://127.0.0.1:8767")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    async fn call(app: Router, req: Request<Body>) -> (StatusCode, serde_json::Value) {
        let resp = app.oneshot(req).await.unwrap();
        let status = resp.status();
        let body = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        (status, serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null))
    }

    #[tokio::test]
    async fn totals_for_a_preset() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/workspaces/ws123/user/u1/time-entries")
            .match_query(mockito::Matcher::Any)
            .with_body(r#"[{"projectId":"p1","description":"Standup","timeInterval":{"start":"2026-04-05T02:00:00Z","end":"2026-04-05T02:30:00Z"}}]"#)
            .create_async()
            .await;
        server.mock("GET", "/workspaces/ws123/projects").with_body(r#"[{"id":"p1","name":"Internal"}]"#).create_async().await;
        let app = gated(router(state(&server.url(), "http://127.0.0.1:1")), Gate::Local);

        let (status, body) = call(app.clone(), local_get("/api/totals?preset=today")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!((body["total"].as_i64(), body["projects"][0]["name"].as_str()), (Some(1800), Some("Internal")));

        let (status, body) = call(app, local_get("/api/totals?preset=yesterday")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "preset must be today, this-week or this-month");
    }

    #[tokio::test]
    async fn log_answers_the_outcome_or_the_error() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/workspaces/ws123/user/u1/time-entries")
            .match_query(mockito::Matcher::Any)
            .with_body("[]")
            .create_async()
            .await;
        let app = gated(router(state(&server.url(), "http://127.0.0.1:1")), Gate::Local);

        let dry = json!({"description": "fix login", "project_id": "p1", "args": ["09:00", "1h"], "date": "2026-04-05", "dry_run": true});
        let (status, body) = call(app.clone(), local_post("/api/log", dry)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["dry_run"], true);
        assert_eq!(body["description"], "fix login");
        assert_eq!(body["start"], "2026-04-05T02:00:00Z");

        let bad = json!({"description": "fix login", "project_id": "p1", "args": ["soon"]});
        let (status, body) = call(app, local_post("/api/log", bad)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["error"].as_str().unwrap().starts_with("invalid duration format"), "{body}");
    }

    #[tokio::test]
    async fn search_skips_short_queries_and_maps_hits() {
        let mut trello = mockito::Server::new_async().await;
        let search = trello
            .mock("GET", "/search")
            .match_query(mockito::Matcher::UrlEncoded("query".into(), "login".into()))
            .with_body(r#"{"cards":[{"id":"c1","idShort":482,"name":"Login fails","shortUrl":"https://trello.com/c/AbC123","closed":false,"board":{"name":"RMS"}}]}"#)
            .expect(1)
            .create_async()
            .await;
        let app = gated(router(state("http://127.0.0.1:1", &trello.url())), Gate::Local);

        let (_, body) = call(app.clone(), local_get("/api/search?q=l")).await;
        assert_eq!(body, json!([]));
        let (status, body) = call(app, local_get("/api/search?q=%20login%20")).await;

        search.assert_async().await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, json!([{"id": "c1", "number": 482, "name": "Login fails", "url": "https://trello.com/c/AbC123", "board": "RMS"}]));
    }

    #[tokio::test]
    async fn the_page_has_the_three_cards() {
        let app = gated(router(state("http://127.0.0.1:1", "http://127.0.0.1:1")), Gate::Local);
        let resp = app.oneshot(local_get("/")).await.unwrap();
        let html = String::from_utf8(axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap().to_vec()).unwrap();
        for id in [r#"id="log""#, r#"id="totals""#, r#"id="ask""#, "backdrop-filter: blur(40px) saturate(1.4)"] {
            assert!(html.contains(id), "page lacks {id}");
        }
        assert!(!html.contains("innerHTML"), "server text goes in with textContent only");
    }
}

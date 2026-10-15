//! `clockify serve`: the dashboard. Two loopback listeners share one router; a guard in
//! front of each checks Host, Origin and, on the tailnet one, the Tailscale login.

use std::sync::Arc;

use anyhow::{Context, Result};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;
use tokio::net::TcpListener;

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

pub fn router() -> Router {
    Router::new().route("/", get(|| async { Html(PAGE) }))
}

/// Serves until the process is stopped.
pub fn run(owner: String, tailnet_host: String) -> Result<()> {
    tokio::runtime::Runtime::new().context("could not start the async runtime")?.block_on(serve(owner, tailnet_host))
}

async fn serve(owner: String, tailnet_host: String) -> Result<()> {
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
    let local = axum::serve(local, gated(router(), Gate::Local));
    let tailnet = axum::serve(tailnet, gated(router(), Gate::Tailnet { owner, host: tailnet_host }));
    tokio::try_join!(async { local.await }, async { tailnet.await })?;
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
        let app = gated(router(), Gate::Local);
        let bad = Request::get("/").header("host", "evil.example:8767").body(Body::empty()).unwrap();
        assert_eq!(app.clone().oneshot(bad).await.unwrap().status(), StatusCode::FORBIDDEN);
        let ok = Request::get("/").header("host", "127.0.0.1:8767").body(Body::empty()).unwrap();
        assert_eq!(app.oneshot(ok).await.unwrap().status(), StatusCode::OK);
    }
}

//! Trello card lookup for the log command: `trello-agent meta` for the title, the Trello
//! REST API for the board, and `trello card` as the fallback.

use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::clockify::{http_agent, null_default};

const TRELLO_API: &str = "https://api.trello.com/1";

#[derive(Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardInfo {
    #[serde(default, deserialize_with = "null_default")]
    pub card_number: String,
    #[serde(default, deserialize_with = "null_default")]
    pub title: String,
    #[serde(default, deserialize_with = "null_default")]
    pub board_id: String,
    #[serde(default, deserialize_with = "null_default")]
    pub board_name: String,
}

/// Looks up a card by URL. The board id and name come from the Trello API when
/// `api_key` and `token` are set; failures there leave them empty.
pub fn fetch_card(url: &str, api_key: &str, token: &str) -> Result<CardInfo> {
    let (short_id, card_number) = extract_ids(url);
    if let Ok(out) = Command::new("trello-agent").args(["meta", url]).output()
        && out.status.success()
    {
        let text = String::from_utf8_lossy(&out.stdout);
        let first = text.lines().next().unwrap_or("");
        let title = first.strip_prefix("# ").unwrap_or(first).to_string();
        return Ok(fetch_card_with_api(TRELLO_API, &short_id, card_number, title, api_key, token));
    }
    fetch_card_basic(url, &card_number)
}

/// The short id and card number from a card URL. Long URLs
/// (https://trello.com/c/QCxVCjm3/320-some-title) carry the number before the slug's
/// first dash; short ones (https://trello.com/c/QCxVCjm3) use the short id as the number.
fn extract_ids(url: &str) -> (String, String) {
    let Some((_, after)) = url.split_once("/c/") else {
        return (String::new(), String::new());
    };
    let mut path = after.split('/');
    let short_id = path.next().unwrap_or("").to_string();
    let number = path.next().map_or("", |slug| slug.split('-').next().unwrap_or(""));
    let card_number = if number.is_empty() { short_id.clone() } else { number.to_string() };
    (short_id, card_number)
}

/// Adds the board id (and the title, when there is none yet) and the board name from
/// the Trello REST API at `base`. Any failure leaves those fields as they are.
fn fetch_card_with_api(base: &str, short_id: &str, card_number: String, title: String, api_key: &str, token: &str) -> CardInfo {
    let mut info = CardInfo { card_number, title, ..Default::default() };
    if api_key.is_empty() || token.is_empty() || short_id.is_empty() {
        return info;
    }

    #[derive(Deserialize)]
    struct Named {
        #[serde(default, deserialize_with = "null_default")]
        name: String,
        #[serde(default, rename = "idBoard", deserialize_with = "null_default")]
        id_board: String,
    }

    let agent = http_agent();
    // Credentials go in the query string, as Trello requires; they no longer reach argv (fix 2).
    let get = |path: String| -> Option<Named> {
        agent
            .get(format!("{base}/{path}?key={api_key}&token={token}"))
            .call()
            .ok()?
            .body_mut()
            .read_json()
            .ok()
    };
    if let Some(card) = get(format!("cards/{short_id}")) {
        info.board_id = card.id_board;
        if info.title.is_empty() {
            info.title = card.name;
        }
    }
    if !info.board_id.is_empty()
        && let Some(board) = get(format!("boards/{}", info.board_id))
    {
        info.board_name = board.name;
    }
    info
}

/// Falls back to `trello card <url>`, which prints the card as JSON.
fn fetch_card_basic(url: &str, card_number: &str) -> Result<CardInfo> {
    let out = Command::new("trello").args(["card", url]).output().context("could not run trello")?;
    if !out.status.success() {
        bail!("trello card failed: {}", out.status);
    }
    let mut info: CardInfo = serde_json::from_slice(&out.stdout).context("trello card printed invalid JSON")?;
    if !card_number.is_empty() {
        info.card_number = card_number.to_string();
    }
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Matcher;

    fn ids(url: &str) -> (String, String) {
        extract_ids(url)
    }

    fn pair(a: &str, b: &str) -> (String, String) {
        (a.to_string(), b.to_string())
    }

    #[test]
    fn card_number_from_long_url() {
        assert_eq!(ids("https://trello.com/c/QCxVCjm3/320-use-caseid-to-validate"), pair("QCxVCjm3", "320"));
    }

    #[test]
    fn short_url_uses_short_id() {
        assert_eq!(ids("https://trello.com/c/QCxVCjm3"), pair("QCxVCjm3", "QCxVCjm3"));
        assert_eq!(ids("https://trello.com/c/abc/"), pair("abc", "abc"));
        assert_eq!(ids("https://trello.com/c/abc/-title"), pair("abc", "abc"));
    }

    #[test]
    fn non_card_url_gives_nothing() {
        assert_eq!(ids("https://example.com/x"), pair("", ""));
    }

    #[test]
    fn card_info_from_trello_card_json() {
        let info: CardInfo =
            serde_json::from_str(r#"{"cardNumber":"1234","title":"Fix login bug","status":"In-progress","boardId":null}"#).unwrap();
        assert_eq!(info, CardInfo { card_number: "1234".into(), title: "Fix login bug".into(), ..Default::default() });
        assert!(serde_json::from_str::<CardInfo>("not valid json").is_err());
    }

    #[test]
    fn api_adds_board_id_title_and_board_name() {
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/cards/abc")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("key".into(), "k".into()),
                Matcher::UrlEncoded("token".into(), "t".into()),
            ]))
            .with_body(r#"{"name":"From API","idBoard":"b1"}"#)
            .create();
        server.mock("GET", "/boards/b1").match_query(Matcher::Any).with_body(r#"{"name":"RMS Board"}"#).create();
        let info = fetch_card_with_api(&server.url(), "abc", "320".into(), String::new(), "k", "t");
        assert_eq!(
            info,
            CardInfo {
                card_number: "320".into(),
                title: "From API".into(),
                board_id: "b1".into(),
                board_name: "RMS Board".into()
            }
        );
    }

    #[test]
    fn api_keeps_meta_title_and_ignores_failures() {
        let mut server = mockito::Server::new();
        server.mock("GET", "/cards/abc").match_query(Matcher::Any).with_status(401).with_body("invalid key").create();
        let info = fetch_card_with_api(&server.url(), "abc", "320".into(), "Meta title".into(), "k", "t");
        assert_eq!(info, CardInfo { card_number: "320".into(), title: "Meta title".into(), ..Default::default() });
    }

    #[test]
    fn api_skipped_without_credentials() {
        let info = fetch_card_with_api("http://127.0.0.1:1", "abc", "320".into(), "T".into(), "", "t");
        assert_eq!(info, CardInfo { card_number: "320".into(), title: "T".into(), ..Default::default() });
    }
}

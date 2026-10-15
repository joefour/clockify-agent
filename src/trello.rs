//! Trello card lookup for the log command: `trello-agent meta` for the title, the Trello
//! REST API for the board, and `trello card` as the fallback.

use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::clockify::{http_agent, null_default};

pub(crate) const TRELLO_API: &str = "https://api.trello.com/1";

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

/// One card from a search, as the dashboard lists it.
#[derive(Debug, PartialEq, Serialize)]
pub struct CardHit {
    pub id: String,
    /// The card number Clockify tasks are named after ("#482").
    pub number: i64,
    pub name: String,
    pub url: String,
    pub board: String,
}

/// Up to 10 open cards matching `query`, from every board the token can see, through
/// the Trello REST API at `base`.
pub fn search_cards(base: &str, query: &str, api_key: &str, token: &str) -> Result<Vec<CardHit>> {
    if api_key.is_empty() || token.is_empty() {
        bail!("Trello key and token are not configured");
    }

    #[derive(Deserialize)]
    struct Found {
        #[serde(default)]
        cards: Vec<Card>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Card {
        id: String,
        id_short: i64,
        #[serde(default, deserialize_with = "null_default")]
        name: String,
        #[serde(default, deserialize_with = "null_default")]
        short_url: String,
        #[serde(default)]
        closed: bool,
        #[serde(default)]
        board: Option<Board>,
    }
    #[derive(Deserialize)]
    struct Board {
        #[serde(default, deserialize_with = "null_default")]
        name: String,
    }

    let mut resp = http_agent()
        .get(format!("{base}/search"))
        .query("query", query)
        .query("modelTypes", "cards")
        .query("partial", "true")
        .query("cards_limit", "10")
        .query("card_fields", "name,idShort,shortUrl,closed")
        .query("card_board", "true")
        .query("board_fields", "name")
        .query("key", api_key)
        .query("token", token)
        .call()
        .context("Trello search failed")?;
    // The status only: Trello's error body can echo the request.
    if !resp.status().is_success() {
        bail!("Trello search failed: {}", resp.status());
    }
    let found: Found = resp.body_mut().read_json().context("Trello search returned invalid JSON")?;
    Ok(found
        .cards
        .into_iter()
        .filter(|c| !c.closed)
        .map(|c| CardHit {
            id: c.id,
            number: c.id_short,
            name: c.name,
            url: c.short_url,
            board: c.board.map(|b| b.name).unwrap_or_default(),
        })
        .collect())
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

    #[test]
    fn search_cards_lists_open_cards() {
        let mut server = mockito::Server::new();
        let search = server
            .mock("GET", "/search")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("query".into(), "login & sso".into()),
                Matcher::UrlEncoded("modelTypes".into(), "cards".into()),
                Matcher::UrlEncoded("partial".into(), "true".into()),
                Matcher::UrlEncoded("cards_limit".into(), "10".into()),
                Matcher::UrlEncoded("card_board".into(), "true".into()),
                Matcher::UrlEncoded("key".into(), "k".into()),
                Matcher::UrlEncoded("token".into(), "t".into()),
            ]))
            .with_body(
                r#"{"cards":[
                    {"id":"c1","idShort":482,"name":"Login fails after SSO","shortUrl":"https://trello.com/c/AbC123","closed":false,"board":{"id":"b1","name":"RMS"}},
                    {"id":"c2","idShort":455,"name":"Old login copy","shortUrl":"https://trello.com/c/Old1","closed":true,"board":{"id":"b1","name":"RMS"}}
                ],"boards":[]}"#,
            )
            .create();

        let hits = search_cards(&server.url(), "login & sso", "k", "t").unwrap();

        search.assert();
        assert_eq!(
            hits,
            vec![CardHit {
                id: "c1".into(),
                number: 482,
                name: "Login fails after SSO".into(),
                url: "https://trello.com/c/AbC123".into(),
                board: "RMS".into()
            }]
        );
    }

    #[test]
    fn search_cards_needs_credentials() {
        let err = search_cards("http://127.0.0.1:1", "login", "", "t").unwrap_err();
        assert_eq!(err.to_string(), "Trello key and token are not configured");
    }

    #[test]
    fn search_cards_reports_trello_errors() {
        let mut server = mockito::Server::new();
        server.mock("GET", "/search").match_query(Matcher::Any).with_status(401).with_body("invalid token").create();
        let err = search_cards(&server.url(), "login", "k", "t").unwrap_err();
        assert_eq!(err.to_string(), "Trello search failed: 401 Unauthorized");
    }
}

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
    pub board_id: String,
    /// Whether the config maps this card's board to a Clockify project. The server sets it.
    pub mapped: bool,
}

/// A card as Trello's REST API returns it, with its board when asked for.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiCard {
    id: String,
    id_short: i64,
    #[serde(default, deserialize_with = "null_default")]
    name: String,
    #[serde(default, deserialize_with = "null_default")]
    url: String,
    #[serde(default)]
    closed: bool,
    #[serde(default)]
    board: Option<ApiBoard>,
}

#[derive(Default, Deserialize)]
struct ApiBoard {
    #[serde(default, deserialize_with = "null_default")]
    id: String,
    #[serde(default, deserialize_with = "null_default")]
    name: String,
}

impl From<ApiCard> for CardHit {
    fn from(c: ApiCard) -> Self {
        let board = c.board.unwrap_or_default();
        CardHit { id: c.id, number: c.id_short, name: c.name, url: c.url, board: board.name, board_id: board.id, mapped: false }
    }
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
        cards: Vec<ApiCard>,
    }

    let mut resp = http_agent()
        .get(format!("{base}/search"))
        .query("query", query)
        .query("modelTypes", "cards")
        .query("partial", "true")
        .query("cards_limit", "10")
        .query("card_fields", "name,idShort,url,closed")
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
    Ok(found.cards.into_iter().filter(|c| !c.closed).map(CardHit::from).collect())
}

/// The card a pasted link points at, through the Trello REST API at `base`. Only
/// trello.com card links pass, so a link can't steer the request to another path.
/// Archived cards come back too: pasting a link is a deliberate choice.
pub fn fetch_card_hit(base: &str, url: &str, api_key: &str, token: &str) -> Result<CardHit> {
    if api_key.is_empty() || token.is_empty() {
        bail!("Trello key and token are not configured");
    }
    let short_id = card_link_short_id(url).context("Not a Trello card link")?;
    let mut resp = http_agent()
        .get(format!("{base}/cards/{short_id}"))
        .query("fields", "name,idShort,url")
        .query("board", "true")
        .query("board_fields", "name")
        .query("key", api_key)
        .query("token", token)
        .call()
        .context("Trello card lookup failed")?;
    // The status only: Trello's error body can echo the request.
    if !resp.status().is_success() {
        bail!("Trello card lookup failed: {}", resp.status());
    }
    let card: ApiCard = resp.body_mut().read_json().context("Trello returned invalid card JSON")?;
    Ok(card.into())
}

/// The short id of a trello.com card link (https or http, with or without www), or
/// None. Short ids are letters and digits, so nothing else can reach the request path.
fn card_link_short_id(url: &str) -> Option<&str> {
    let url = url.trim();
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    let short_id = rest.strip_prefix("trello.com/c/")?.split(['/', '?', '#']).next()?;
    (!short_id.is_empty() && short_id.chars().all(|c| c.is_ascii_alphanumeric())).then_some(short_id)
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
                Matcher::UrlEncoded("card_fields".into(), "name,idShort,url,closed".into()),
                Matcher::UrlEncoded("card_board".into(), "true".into()),
                Matcher::UrlEncoded("key".into(), "k".into()),
                Matcher::UrlEncoded("token".into(), "t".into()),
            ]))
            .with_body(
                r#"{"cards":[
                    {"id":"c1","idShort":482,"name":"Login fails after SSO","url":"https://trello.com/c/AbC123/482-login-fails-after-sso","closed":false,"board":{"id":"b1","name":"RMS"}},
                    {"id":"c2","idShort":455,"name":"Old login copy","url":"https://trello.com/c/Old1/455-old-login-copy","closed":true,"board":{"id":"b1","name":"RMS"}}
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
                url: "https://trello.com/c/AbC123/482-login-fails-after-sso".into(),
                board: "RMS".into(),
                board_id: "b1".into(),
                mapped: false,
            }]
        );
        assert_eq!(extract_ids(&hits[0].url).1, hits[0].number.to_string());
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

    fn card_mock(server: &mut mockito::Server, short_id: &str) -> mockito::Mock {
        server
            .mock("GET", format!("/cards/{short_id}").as_str())
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("fields".into(), "name,idShort,url".into()),
                Matcher::UrlEncoded("board".into(), "true".into()),
                Matcher::UrlEncoded("board_fields".into(), "name".into()),
                Matcher::UrlEncoded("key".into(), "k".into()),
                Matcher::UrlEncoded("token".into(), "t".into()),
            ]))
            .with_body(r#"{"id":"c1","idShort":482,"name":"Login fails after SSO","url":"https://trello.com/c/AbC123/482-login-fails-after-sso","board":{"id":"b1","name":"RMS"}}"#)
    }

    fn login_hit() -> CardHit {
        CardHit {
            id: "c1".into(),
            number: 482,
            name: "Login fails after SSO".into(),
            url: "https://trello.com/c/AbC123/482-login-fails-after-sso".into(),
            board: "RMS".into(),
            board_id: "b1".into(),
            mapped: false,
        }
    }

    #[test]
    fn card_hit_from_short_and_long_links() {
        let mut server = mockito::Server::new();
        let card = card_mock(&mut server, "AbC123").expect(5).create();
        for link in [
            "https://trello.com/c/AbC123",
            "https://trello.com/c/AbC123/482-login-fails-after-sso",
            "http://www.trello.com/c/AbC123/",
            " https://trello.com/c/AbC123?filter=member ",
            "https://trello.com/c/AbC123#comment-1",
        ] {
            assert_eq!(fetch_card_hit(&server.url(), link, "k", "t").unwrap(), login_hit(), "{link}");
        }
        card.assert();
    }

    #[test]
    fn card_hit_refuses_other_links_before_any_request() {
        // Port 1 refuses connections: a request would fail with a different error.
        for link in [
            "https://evil.example/c/AbC123",
            "https://trello.com.evil.example/c/AbC123",
            "https://evil.example/trello.com/c/AbC123",
            "https://trello.com/b/AbC123/rms",
            "https://trello.com/c/",
            "https://trello.com/c/..%2F..%2Fmembers",
            "https://trello.com/c/Ab-C1",
            "trello.com/c/AbC123",
            "",
        ] {
            let err = fetch_card_hit("http://127.0.0.1:1", link, "k", "t").unwrap_err();
            assert_eq!(err.to_string(), "Not a Trello card link", "{link}");
        }
    }

    #[test]
    fn card_hit_needs_credentials() {
        let err = fetch_card_hit("http://127.0.0.1:1", "https://trello.com/c/AbC123", "k", "").unwrap_err();
        assert_eq!(err.to_string(), "Trello key and token are not configured");
    }

    #[test]
    fn card_hit_reports_trello_errors() {
        let mut server = mockito::Server::new();
        server.mock("GET", "/cards/Gone1").match_query(Matcher::Any).with_status(404).with_body("The requested resource was not found.").create();
        let err = fetch_card_hit(&server.url(), "https://trello.com/c/Gone1", "k", "t").unwrap_err();
        assert_eq!(err.to_string(), "Trello card lookup failed: 404 Not Found");
    }
}

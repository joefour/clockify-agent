//! Ask Claude from the dashboard: one headless `claude -p` run at a time with the tray's
//! argv, its stream-json turned into events that every viewer replays from the start.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, watch};

const MODEL: &str = "claude-sonnet-5-5";
/// The tray's scope prompt (`scope` in tray/main.swift), word for word.
const SCOPE: &str = "You are a Clockify time-tracking assistant. Use the installed clockify binary on PATH, as the clockify skill describes. Never build, run or edit this repository's source code.";
/// Event text kept per run, as in the tray; the oldest events go first.
const CAP: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum AskEvent {
    /// No run since the server started.
    Idle,
    /// The first event of every replay: the prompt, and when the run started (Unix ms).
    Start { prompt: String, started: u64 },
    Text { text: String },
    Bash { command: String },
    Result { text: String, is_error: bool },
    /// The run ended: its exit code (None when a signal stopped it) and its last stderr lines.
    Exit { code: Option<i32>, stderr: Vec<String> },
}

impl AskEvent {
    fn size(&self) -> usize {
        match self {
            AskEvent::Text { text } | AskEvent::Result { text, .. } => text.len(),
            AskEvent::Bash { command } => command.len(),
            AskEvent::Exit { stderr, .. } => stderr.iter().map(String::len).sum(),
            AskEvent::Idle | AskEvent::Start { .. } => 0,
        }
    }
}

/// argv after `claude` for one headless run in `repo`, the same as the tray's claudeArgs.
pub fn claude_args(prompt: &str, repo: &Path) -> Vec<String> {
    let mcp = repo.join(".mcp.json").to_string_lossy().into_owned();
    [
        "-p", "--model", MODEL,
        "--permission-mode", "bypassPermissions",
        "--setting-sources", "project,local",
        "--strict-mcp-config", "--mcp-config", &mcp,
        "--tools", "Bash,Read,Skill",
        "--effort", "low",
        "--no-session-persistence",
        "--append-system-prompt", SCOPE,
        "--output-format", "stream-json", "--verbose",
        "--", prompt,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// The events in one stream-json line; none for lines the reply ignores (init, tool
/// results, rate limits) and for malformed ones. A port of the tray's parseEvents.
pub fn parse_events(line: &str) -> Vec<AskEvent> {
    let Ok(v) = serde_json::from_str::<Value>(line) else { return Vec::new() };
    match v["type"].as_str() {
        Some("assistant") => v["message"]["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| match c["type"].as_str() {
                Some("text") => c["text"].as_str().map(|t| AskEvent::Text { text: t.into() }),
                Some("tool_use") if c["name"] == "Bash" => {
                    c["input"]["command"].as_str().map(|cmd| AskEvent::Bash { command: cmd.into() })
                }
                _ => None,
            })
            .collect(),
        // Error subtypes (error_max_turns, error_during_execution) carry no result text.
        Some("result") => vec![AskEvent::Result {
            text: v["result"].as_str().or(v["subtype"].as_str()).unwrap_or("").into(),
            is_error: v["is_error"].as_bool().unwrap_or(false),
        }],
        _ => Vec::new(),
    }
}

#[derive(Default)]
struct RunState {
    /// 0 until the first run.
    id: u64,
    prompt: String,
    started: u64,
    events: VecDeque<AskEvent>,
    /// Events the cap dropped from the front.
    first: usize,
    bytes: usize,
    running: bool,
    pgid: Option<i32>,
}

impl RunState {
    fn push(&mut self, e: AskEvent) {
        self.bytes += e.size();
        self.events.push_back(e);
        while self.bytes > CAP && self.events.len() > 1 {
            let old = self.events.pop_front().expect("more than one event");
            self.bytes -= old.size();
            self.first += 1;
        }
    }
}

pub enum StartError {
    Busy,
    Spawn(std::io::Error),
}

pub struct Ask {
    exe: String,
    repo: PathBuf,
    state: Mutex<RunState>,
    /// Bumped on every change, so viewers wake up.
    tick: watch::Sender<u64>,
}

impl Ask {
    pub fn new(exe: impl Into<String>, repo: PathBuf) -> Self {
        Ask { exe: exe.into(), repo, state: Mutex::new(RunState::default()), tick: watch::channel(0).0 }
    }

    fn update(&self, f: impl FnOnce(&mut RunState)) {
        f(&mut self.state.lock().expect("ask state lock"));
        self.tick.send_modify(|n| *n += 1);
    }

    /// Starts a run unless one is going. Call it inside the tokio runtime.
    pub fn start(self: &Arc<Self>, prompt: String) -> Result<(), StartError> {
        let mut s = self.state.lock().expect("ask state lock");
        if s.running {
            return Err(StartError::Busy);
        }
        let mut child = Command::new(&self.exe)
            .args(claude_args(&prompt, &self.repo))
            .current_dir(&self.repo)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .map_err(StartError::Spawn)?;
        eprintln!("prompt: {prompt}");
        let pgid = child.id().and_then(|p| i32::try_from(p).ok());
        *s = RunState { id: s.id + 1, prompt, started: now_ms(), running: true, pgid, ..RunState::default() };
        drop(s);
        self.tick.send_modify(|n| *n += 1);

        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
        let ask = Arc::clone(self);
        let mut reader = tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                for e in parse_events(&line) {
                    if let AskEvent::Bash { command } = &e {
                        eprintln!("bash: {command}");
                    }
                    ask.update(|s| s.push(e));
                }
            }
        });
        let tail = tokio::spawn(async move {
            let mut tail = VecDeque::new();
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if tail.len() == 20 {
                    tail.pop_front();
                }
                tail.push_back(line);
            }
            Vec::from(tail)
        });
        let ask = Arc::clone(self);
        tokio::spawn(async move {
            let code = child.wait().await.ok().and_then(|s| s.code());
            // Children claude left behind (a background job) would hold the pipes open.
            if let Some(pg) = pgid {
                signal(pg, libc::SIGTERM);
            }
            if tokio::time::timeout(Duration::from_secs(2), &mut reader).await.is_err() {
                reader.abort();
            }
            let stderr = tokio::time::timeout(Duration::from_secs(2), tail).await.ok().and_then(Result::ok).unwrap_or_default();
            eprintln!("exit: {code:?}");
            ask.update(|s| {
                s.push(AskEvent::Exit { code, stderr });
                s.running = false;
                s.pgid = None;
            });
        });
        Ok(())
    }

    /// SIGTERM to the run's process group, then SIGKILL after 3 s if it is still going.
    pub fn stop(self: &Arc<Self>) {
        let (id, pgid) = {
            let s = self.state.lock().expect("ask state lock");
            (s.id, s.pgid)
        };
        let Some(pg) = pgid else { return };
        signal(pg, libc::SIGTERM);
        let ask = Arc::clone(self);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(3)).await;
            let s = ask.state.lock().expect("ask state lock");
            if s.id == id && s.running {
                signal(pg, libc::SIGKILL);
            }
        });
    }

    /// The current run from its start, then its live events until it exits. A viewer
    /// that reconnects (a phone waking up) gets the whole run again. Idle before any run.
    pub fn follow(self: &Arc<Self>) -> mpsc::Receiver<AskEvent> {
        let (tx, rx) = mpsc::channel(64);
        let ask = Arc::clone(self);
        let mut tick = self.tick.subscribe();
        tokio::spawn(async move {
            let (run, start) = {
                let s = ask.state.lock().expect("ask state lock");
                (s.id, AskEvent::Start { prompt: s.prompt.clone(), started: s.started })
            };
            if run == 0 {
                let _ = tx.send(AskEvent::Idle).await;
                return;
            }
            if tx.send(start).await.is_err() {
                return;
            }
            // Index of the next event to send, counting the ones the cap dropped.
            let mut next = 0;
            loop {
                let (batch, done) = {
                    let s = ask.state.lock().expect("ask state lock");
                    if s.id != run {
                        (Vec::new(), true)
                    } else {
                        let from = next.max(s.first);
                        next = s.first + s.events.len();
                        (s.events.iter().skip(from - s.first).cloned().collect::<Vec<_>>(), !s.running)
                    }
                };
                for e in batch {
                    if tx.send(e).await.is_err() {
                        return;
                    }
                }
                if done || tick.changed().await.is_err() {
                    return;
                }
            }
        });
        rx
    }
}

fn signal(pgid: i32, sig: i32) {
    // SAFETY: killpg only sends a signal; a stale group id fails with ESRCH.
    unsafe {
        libc::killpg(pgid, sig);
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    // Lines trimmed from a real run (2026-10-13), as in the tray's selftest.
    #[test]
    fn parse_events_keeps_text_bash_and_result() {
        let bash = r#"{"type":"assistant","message":{"model":"claude-sonnet-5-5","role":"assistant","content":[{"type":"tool_use","id":"toolu_01","name":"Bash","input":{"command":"clockify summary today","description":"Run clockify summary for today"}}]}}"#;
        let text = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Total today: 0:00."},{"type":"tool_use","id":"t2","name":"mcp__trello__get_card","input":{"card":"AbC123"}}]}}"#;
        let ok = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":7885,"result":"Total today: 0:00."}"#;
        let err = r#"{"type":"result","subtype":"error_max_turns","is_error":true,"duration_ms":100}"#;
        assert_eq!(parse_events(bash), vec![AskEvent::Bash { command: "clockify summary today".into() }]);
        assert_eq!(parse_events(text), vec![AskEvent::Text { text: "Total today: 0:00.".into() }], "MCP tool_use ignored");
        assert_eq!(parse_events(ok), vec![AskEvent::Result { text: "Total today: 0:00.".into(), is_error: false }]);
        assert_eq!(parse_events(err), vec![AskEvent::Result { text: "error_max_turns".into(), is_error: true }]);
    }

    #[test]
    fn parse_events_ignores_the_rest() {
        for line in [
            r#"{"type":"system","subtype":"init","cwd":"/r","tools":["Bash","Read","Skill"],"model":"claude-sonnet-5-5"}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_01","content":"Total 0:00"}]}}"#,
            r#"{"type":"rate_limit_event"}"#,
            "not json",
            "",
            "[]",
        ] {
            assert_eq!(parse_events(line), vec![], "{line}");
        }
    }

    #[test]
    fn claude_args_match_the_tray() {
        let a = claude_args("--help me log 2h", Path::new("/r"));
        assert_eq!(a[0], "-p");
        assert_eq!(a[a.len() - 2..], ["--", "--help me log 2h"], "prompt last, after --");
        let after = |flag: &str| a[a.iter().position(|x| x == flag).unwrap() + 1].as_str();
        assert_eq!(after("--mcp-config"), "/r/.mcp.json");
        assert_eq!(after("--tools"), "Bash,Read,Skill");
        assert_eq!(after("--permission-mode"), "bypassPermissions");
        assert_eq!(after("--model"), "claude-sonnet-5-5");
        assert!(a.iter().any(|x| x == "--strict-mcp-config"));
    }

    #[test]
    fn the_cap_drops_the_oldest_events() {
        let mut s = RunState::default();
        for _ in 0..3 {
            s.push(AskEvent::Text { text: "x".repeat(30 * 1024) });
        }
        assert_eq!((s.events.len(), s.first, s.bytes), (2, 1, 60 * 1024));
        s.push(AskEvent::Text { text: "y".repeat(100 * 1024) });
        assert_eq!((s.events.len(), s.first), (1, 3), "one event over the cap stays");
    }

    fn fake_claude(name: &str, script: &str) -> (String, PathBuf) {
        let dir = std::env::temp_dir().join(format!("clockify-ask-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("claude");
        std::fs::write(&exe, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        (exe.to_string_lossy().into_owned(), dir)
    }

    async fn collect(ask: &Arc<Ask>) -> Vec<AskEvent> {
        let mut rx = ask.follow();
        let mut out = Vec::new();
        while let Some(e) = tokio::time::timeout(Duration::from_secs(10), rx.recv()).await.expect("events stopped coming") {
            out.push(e);
        }
        out
    }

    #[tokio::test]
    async fn a_run_streams_its_events_and_replays_them() {
        let (exe, dir) = fake_claude(
            "run",
            r#"echo '{"type":"assistant","message":{"content":[{"type":"text","text":"hi"},{"type":"tool_use","name":"Bash","input":{"command":"clockify summary today"}}]}}'
echo oops >&2
echo '{"type":"result","subtype":"success","is_error":false,"result":"done"}'"#,
        );
        let ask = Arc::new(Ask::new(exe, dir));
        assert_eq!(collect(&ask).await, vec![AskEvent::Idle]);

        assert!(ask.start("log 2h".into()).is_ok());
        let events = collect(&ask).await;

        assert!(matches!(&events[0], AskEvent::Start { prompt, .. } if prompt == "log 2h"), "{events:?}");
        assert_eq!(
            events[1..],
            [
                AskEvent::Text { text: "hi".into() },
                AskEvent::Bash { command: "clockify summary today".into() },
                AskEvent::Result { text: "done".into(), is_error: false },
                AskEvent::Exit { code: Some(0), stderr: vec!["oops".into()] },
            ]
        );
        assert_eq!(collect(&ask).await, events, "a viewer that comes back gets the whole run once");
    }

    #[tokio::test]
    async fn one_run_at_a_time_and_stop_ends_it() {
        let (exe, dir) = fake_claude("stop", "sleep 30");
        let ask = Arc::new(Ask::new(exe, dir));
        assert!(ask.start("a".into()).is_ok());
        assert!(matches!(ask.start("b".into()), Err(StartError::Busy)));

        ask.stop();
        let events = collect(&ask).await;

        assert!(matches!(events.last(), Some(AskEvent::Exit { code: None, .. })), "stopped by a signal: {events:?}");
        assert!(ask.start("c".into()).is_ok(), "free again after the exit");
        ask.stop();
    }
}

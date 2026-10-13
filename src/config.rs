//! Layered config: the RMS Clockify and Trello .env files, then the user's TOML file
//! (named config.yaml), then environment variables (highest priority).

use std::path::{Path, PathBuf};

use serde::Deserialize;

const RMS_ENV_PATH: &str = "/Users/loithai/Projects/accenturesoft/rms/mcp-servers/clockify/.env";
const RMS_TRELLO_ENV_PATH: &str = "/Users/loithai/Projects/accenturesoft/rms/mcp-servers/trello/.env";

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ProjectMapping {
    pub name: String,
    pub trello_board_id: String,
    pub clockify_project_id: String,
}

#[derive(Debug, Default, PartialEq)]
pub struct Config {
    pub api_key: String,
    pub workspace_id: String,
    pub default_project_id: String,
    pub trello_api_key: String,
    pub trello_token: String,
    pub user_id: String,
    pub project_mappings: Vec<ProjectMapping>,
}

/// The user's config file as written: an absent key keeps the lower layer's value.
/// Unknown keys (such as the retired trello_url) are ignored.
#[derive(Deserialize)]
struct FileConfig {
    api_key: Option<String>,
    workspace_id: Option<String>,
    default_project_id: Option<String>,
    trello_api_key: Option<String>,
    trello_token: Option<String>,
    user_id: Option<String>,
    project: Option<Vec<ProjectMapping>>,
}

/// Loads the config from the real files and the process environment.
pub fn load() -> Config {
    let home = std::env::var("HOME").unwrap_or_default();
    let config_path = PathBuf::from(home).join(".config/clockify-agent/config.yaml");
    load_from(Path::new(RMS_ENV_PATH), Path::new(RMS_TRELLO_ENV_PATH), &config_path, |k| {
        std::env::var(k).ok()
    })
}

fn load_from(rms_env: &Path, rms_trello_env: &Path, config_path: &Path, env: impl Fn(&str) -> Option<String>) -> Config {
    let mut cfg = Config::default();

    for (key, val) in read_env_file(rms_env) {
        match key.as_str() {
            "CLOCKIFY_API_KEY" => cfg.api_key = val,
            "CLOCKIFY_WORKSPACE_ID" => cfg.workspace_id = val,
            _ => {}
        }
    }
    for (key, val) in read_env_file(rms_trello_env) {
        match key.as_str() {
            "TRELLO_API_KEY" => cfg.trello_api_key = val,
            "TRELLO_TOKEN" => cfg.trello_token = val,
            _ => {}
        }
    }

    // A missing or unparseable file is skipped, as the Go version did.
    let file = std::fs::read_to_string(config_path).ok().and_then(|s| toml::from_str::<FileConfig>(&s).ok());
    if let Some(f) = file {
        set(&mut cfg.api_key, f.api_key);
        set(&mut cfg.workspace_id, f.workspace_id);
        set(&mut cfg.default_project_id, f.default_project_id);
        set(&mut cfg.trello_api_key, f.trello_api_key);
        set(&mut cfg.trello_token, f.trello_token);
        set(&mut cfg.user_id, f.user_id);
        if let Some(p) = f.project {
            cfg.project_mappings = p;
        }
    }

    let var = |k: &str| env(k).filter(|v| !v.is_empty());
    set(&mut cfg.api_key, var("CLOCKIFY_API_KEY"));
    set(&mut cfg.workspace_id, var("CLOCKIFY_WORKSPACE_ID"));
    set(&mut cfg.default_project_id, var("CLOCKIFY_DEFAULT_PROJECT_ID"));
    set(&mut cfg.trello_api_key, var("TRELLO_API_KEY"));
    set(&mut cfg.trello_token, var("TRELLO_TOKEN"));
    cfg
}

fn set(dst: &mut String, val: Option<String>) {
    if let Some(v) = val {
        *dst = v;
    }
}

/// KEY=VALUE pairs from a .env file: blank lines and # comments skipped, both sides
/// trimmed, split at the first '='. A missing file yields nothing.
fn read_env_file(path: &Path) -> Vec<(String, String)> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .filter(|(k, _)| !k.is_empty())
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

impl Config {
    /// The Clockify project for a Trello board: a board-id match (only when the card has
    /// a board id), then a case-insensitive substring match on the name either way round,
    /// then the default project.
    pub fn get_clockify_project_id(&self, board_id: &str, board_name: &str) -> String {
        if !board_id.is_empty()
            && let Some(m) = self.project_mappings.iter().find(|m| m.trello_board_id == board_id)
        {
            return m.clockify_project_id.clone();
        }
        if board_name.is_empty() {
            return self.default_project_id.clone();
        }
        let board = board_name.to_lowercase();
        self.project_mappings
            .iter()
            .find(|m| {
                let name = m.name.to_lowercase();
                board.contains(&name) || name.contains(&board)
            })
            .map_or_else(|| self.default_project_id.clone(), |m| m.clockify_project_id.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A fresh directory under the system temp dir, unique to this test.
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("clockify-config-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn missing_files_give_defaults() {
        let dir = temp_dir("missing");
        let cfg = load_from(&dir.join("a.env"), &dir.join("b.env"), &dir.join("config.yaml"), no_env);
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn layers_apply_in_order() {
        let dir = temp_dir("layers");
        std::fs::write(
            dir.join("clockify.env"),
            "# comment\n\nCLOCKIFY_API_KEY = rms-key\nCLOCKIFY_WORKSPACE_ID=rms-ws\nOTHER=x\n",
        )
        .unwrap();
        std::fs::write(dir.join("trello.env"), "TRELLO_API_KEY=rms=key\r\nTRELLO_TOKEN=rms-token\r\n").unwrap();
        std::fs::write(
            dir.join("config.yaml"),
            r#"
workspace_id = "file-ws"
user_id = "file-user"
trello_url = "http://ignored.local"

[[project]]
name = "Alpha"
trello_board_id = "b1"
clockify_project_id = "p1"
"#,
        )
        .unwrap();
        let env = HashMap::from([
            ("TRELLO_TOKEN", "env-token"),
            ("CLOCKIFY_DEFAULT_PROJECT_ID", "env-project"),
            ("CLOCKIFY_API_KEY", ""),
        ]);
        let cfg = load_from(&dir.join("clockify.env"), &dir.join("trello.env"), &dir.join("config.yaml"), |k| {
            env.get(k).map(|v| v.to_string())
        });
        assert_eq!(cfg.api_key, "rms-key"); // the file lacks it, and an empty env var is ignored
        assert_eq!(cfg.workspace_id, "file-ws"); // the file beats the RMS .env
        assert_eq!(cfg.trello_api_key, "rms=key"); // only the first '=' splits
        assert_eq!(cfg.trello_token, "env-token"); // env beats everything
        assert_eq!(cfg.default_project_id, "env-project");
        assert_eq!(cfg.user_id, "file-user");
        assert_eq!(
            cfg.project_mappings,
            vec![ProjectMapping { name: "Alpha".into(), trello_board_id: "b1".into(), clockify_project_id: "p1".into() }]
        );
    }

    #[test]
    fn broken_config_file_is_skipped() {
        let dir = temp_dir("broken");
        std::fs::write(dir.join("clockify.env"), "CLOCKIFY_API_KEY=rms-key\n").unwrap();
        std::fs::write(dir.join("config.yaml"), "api_key = \n[[[nope").unwrap();
        let cfg = load_from(&dir.join("clockify.env"), &dir.join("none.env"), &dir.join("config.yaml"), no_env);
        assert_eq!(cfg.api_key, "rms-key");
    }

    fn mapped() -> Config {
        Config {
            default_project_id: "default".into(),
            project_mappings: vec![
                ProjectMapping { name: "Name Only".into(), trello_board_id: String::new(), clockify_project_id: "p-name".into() },
                ProjectMapping { name: "RMS".into(), trello_board_id: "b-rms".into(), clockify_project_id: "p-rms".into() },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn project_by_board_id() {
        assert_eq!(mapped().get_clockify_project_id("b-rms", "Whatever"), "p-rms");
    }

    #[test]
    fn project_by_name_substring_either_way() {
        assert_eq!(mapped().get_clockify_project_id("b-x", "RMS Sprint Board"), "p-rms");
        assert_eq!(mapped().get_clockify_project_id("b-x", "name"), "p-name");
    }

    #[test]
    fn empty_board_id_never_matches_by_id() {
        // Go matched "Name Only" here because both board ids were empty (fix 8).
        assert_eq!(mapped().get_clockify_project_id("", "RMS"), "p-rms");
        assert_eq!(mapped().get_clockify_project_id("", ""), "default");
    }

    #[test]
    fn falls_back_to_default() {
        assert_eq!(mapped().get_clockify_project_id("b-x", "Unrelated"), "default");
        assert_eq!(mapped().get_clockify_project_id("b-x", ""), "default");
    }
}

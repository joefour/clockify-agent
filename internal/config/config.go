package config

import (
	"os"
	"path/filepath"
	"strings"

	"github.com/BurntSushi/toml"
)

var configPath = filepath.Join(os.Getenv("HOME"), ".config", "clockify-agent", "config.yaml")

const rmsEnvPath = "/Users/loithai/Projects/accenturesoft/rms/mcp-servers/clockify/.env"
const rmsTrelloEnvPath = "/Users/loithai/Projects/accenturesoft/rms/mcp-servers/trello/.env"

type ProjectMapping struct {
	Name               string `toml:"name"`
	TrelloBoardID      string `toml:"trello_board_id"`
	ClockifyProjectID  string `toml:"clockify_project_id"`
}

type Config struct {
	APIKey           string `toml:"api_key"`
	WorkspaceID      string `toml:"workspace_id"`
	DefaultProjectID string `toml:"default_project_id"`
	TrelloURL        string `toml:"trello_url"`
	TrelloAPIKey     string `toml:"trello_api_key"`
	TrelloToken      string `toml:"trello_token"`
	UserID           string `toml:"user_id"`
	ProjectMappings  []ProjectMapping `toml:"project"`
}

func Load() (*Config, error) {
	cfg := &Config{}

	// 1. Load from RMS .env as base (Clockify)
	if data, err := os.ReadFile(rmsEnvPath); err == nil {
		for _, line := range strings.Split(string(data), "\n") {
			line = strings.TrimSpace(line)
			if line == "" || strings.HasPrefix(line, "#") {
				continue
			}
			if idx := strings.Index(line, "="); idx > 0 {
				key := strings.TrimSpace(line[:idx])
				val := strings.TrimSpace(line[idx+1:])
				switch key {
				case "CLOCKIFY_API_KEY":
					cfg.APIKey = val
				case "CLOCKIFY_WORKSPACE_ID":
					cfg.WorkspaceID = val
				}
			}
		}
	}

	// 1b. Load from RMS Trello .env
	if data, err := os.ReadFile(rmsTrelloEnvPath); err == nil {
		for _, line := range strings.Split(string(data), "\n") {
			line = strings.TrimSpace(line)
			if line == "" || strings.HasPrefix(line, "#") {
				continue
			}
			if idx := strings.Index(line, "="); idx > 0 {
				key := strings.TrimSpace(line[:idx])
				val := strings.TrimSpace(line[idx+1:])
				switch key {
				case "TRELLO_API_KEY":
					cfg.TrelloAPIKey = val
				case "TRELLO_TOKEN":
					cfg.TrelloToken = val
				}
			}
		}
	}

	// 2. Override with user config file if it exists
	toml.DecodeFile(configPath, cfg)

	// 3. Override with env vars
	if val := os.Getenv("CLOCKIFY_API_KEY"); val != "" {
		cfg.APIKey = val
	}
	if val := os.Getenv("CLOCKIFY_WORKSPACE_ID"); val != "" {
		cfg.WorkspaceID = val
	}
	if val := os.Getenv("CLOCKIFY_DEFAULT_PROJECT_ID"); val != "" {
		cfg.DefaultProjectID = val
	}
	if val := os.Getenv("CLOCKIFY_TRELLO_URL"); val != "" {
		cfg.TrelloURL = val
	}
	if val := os.Getenv("TRELLO_API_KEY"); val != "" {
		cfg.TrelloAPIKey = val
	}
	if val := os.Getenv("TRELLO_TOKEN"); val != "" {
		cfg.TrelloToken = val
	}

	return cfg, nil
}

func (c *Config) GetClockifyProjectID(boardID, boardName string) string {
	// First: direct board ID match
	for _, m := range c.ProjectMappings {
		if m.TrelloBoardID == boardID {
			return m.ClockifyProjectID
		}
	}

	if boardName == "" {
		return c.DefaultProjectID
	}

	// Second: fuzzy name match (case-insensitive substring)
	boardNameLower := strings.ToLower(boardName)
	for _, m := range c.ProjectMappings {
		if strings.Contains(boardNameLower, strings.ToLower(m.Name)) ||
			strings.Contains(strings.ToLower(m.Name), boardNameLower) {
			return m.ClockifyProjectID
		}
	}

	// Fallback: default
	return c.DefaultProjectID
}

func (c *Config) Save() error {
	// Ensure directory exists
	dir := filepath.Dir(configPath)
	if err := os.MkdirAll(dir, 0755); err != nil {
		return err
	}

	f, err := os.Create(configPath)
	if err != nil {
		return err
	}
	defer f.Close()

	encoder := toml.NewEncoder(f)
	return encoder.Encode(c)
}

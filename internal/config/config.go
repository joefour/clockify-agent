package config

import (
	"os"
	"path/filepath"
	"strings"

	"github.com/BurntSushi/toml"
)

var configPath = filepath.Join(os.Getenv("HOME"), ".config", "clockify-agent", "config.yaml")

const rmsEnvPath = "/Users/loithai/Projects/accenturesoft/rms/mcp-servers/clockify/.env"

type Config struct {
	APIKey           string `toml:"api_key"`
	WorkspaceID      string `toml:"workspace_id"`
	DefaultProjectID string `toml:"default_project_id"`
	TrelloURL        string `toml:"trello_url"`
}

func Load() (*Config, error) {
	cfg := &Config{}

	// 1. Load from RMS .env as base
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

	return cfg, nil
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

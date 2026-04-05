package config

import (
	"os"
	"path/filepath"

	"github.com/BurntSushi/toml"
)

var configPath = filepath.Join(os.Getenv("HOME"), ".config", "clockify-agent", "config.yaml")

type Config struct {
	APIKey           string `toml:"api_key"`
	WorkspaceID      string `toml:"workspace_id"`
	DefaultProjectID string `toml:"default_project_id"`
	TrelloURL        string `toml:"trello_url"`
}

func Load() (*Config, error) {
	cfg := &Config{}

	_, err := toml.DecodeFile(configPath, cfg)
	if err != nil {
		return nil, err
	}

	// Override with env vars if set
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

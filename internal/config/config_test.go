package config

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/BurntSushi/toml"
)

func TestLoad_FileNotFound(t *testing.T) {
	// Create a temp directory with no config file
	// With RMS .env as base, Load() succeeds if rmsEnvPath exists
	// We test that it returns values (from RMS .env or defaults)
	tmpDir := t.TempDir()
	configPath = filepath.Join(tmpDir, "nonexistent.yaml")

	cfg, err := Load()
	// Now Load() doesn't fail if config file missing (RMS .env is base)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	// Should have loaded from RMS .env if it exists
	if cfg.APIKey == "" || cfg.WorkspaceID == "" {
		t.Log("APIKey or WorkspaceID is empty (RMS .env may not exist)")
	}
}

func TestLoad_EnvVarsOverrideFile(t *testing.T) {
	// Create a temp config file
	tmpDir := t.TempDir()
	configPath = filepath.Join(tmpDir, "config.yaml")

	fileConfig := Config{
		APIKey:           "file-api-key",
		WorkspaceID:      "file-workspace",
		DefaultProjectID: "file-project",
		TrelloURL:        "http://file.local",
	}

	f, err := os.Create(configPath)
	if err != nil {
		t.Fatalf("failed to create temp config file: %v", err)
	}
	defer f.Close()

	encoder := toml.NewEncoder(f)
	if err := encoder.Encode(fileConfig); err != nil {
		t.Fatalf("failed to encode config: %v", err)
	}
	f.Close()

	// Set env vars to override file values
	os.Setenv("CLOCKIFY_API_KEY", "env-api-key")
	os.Setenv("CLOCKIFY_WORKSPACE_ID", "env-workspace")
	os.Setenv("CLOCKIFY_DEFAULT_PROJECT_ID", "env-project")
	os.Setenv("CLOCKIFY_TRELLO_URL", "http://env.local")
	defer func() {
		os.Unsetenv("CLOCKIFY_API_KEY")
		os.Unsetenv("CLOCKIFY_WORKSPACE_ID")
		os.Unsetenv("CLOCKIFY_DEFAULT_PROJECT_ID")
		os.Unsetenv("CLOCKIFY_TRELLO_URL")
	}()

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() returned error: %v", err)
	}

	if cfg.APIKey != "env-api-key" {
		t.Errorf("APIKey: expected 'env-api-key', got '%s'", cfg.APIKey)
	}
	if cfg.WorkspaceID != "env-workspace" {
		t.Errorf("WorkspaceID: expected 'env-workspace', got '%s'", cfg.WorkspaceID)
	}
	if cfg.DefaultProjectID != "env-project" {
		t.Errorf("DefaultProjectID: expected 'env-project', got '%s'", cfg.DefaultProjectID)
	}
	if cfg.TrelloURL != "http://env.local" {
		t.Errorf("TrelloURL: expected 'http://env.local', got '%s'", cfg.TrelloURL)
	}
}

func TestSave(t *testing.T) {
	tmpDir := t.TempDir()
	configPath = filepath.Join(tmpDir, "config.yaml")

	cfg := &Config{
		APIKey:           "save-api-key",
		WorkspaceID:      "save-workspace",
		DefaultProjectID: "save-project",
		TrelloURL:        "http://save.local",
	}

	if err := cfg.Save(); err != nil {
		t.Fatalf("Save() returned error: %v", err)
	}

	// Verify the file was written correctly
	var loaded Config
	_, err := toml.DecodeFile(configPath, &loaded)
	if err != nil {
		t.Fatalf("failed to decode saved config: %v", err)
	}

	if loaded.APIKey != cfg.APIKey {
		t.Errorf("APIKey: expected '%s', got '%s'", cfg.APIKey, loaded.APIKey)
	}
	if loaded.WorkspaceID != cfg.WorkspaceID {
		t.Errorf("WorkspaceID: expected '%s', got '%s'", cfg.WorkspaceID, loaded.WorkspaceID)
	}
	if loaded.DefaultProjectID != cfg.DefaultProjectID {
		t.Errorf("DefaultProjectID: expected '%s', got '%s'", cfg.DefaultProjectID, loaded.DefaultProjectID)
	}
	if loaded.TrelloURL != cfg.TrelloURL {
		t.Errorf("TrelloURL: expected '%s', got '%s'", cfg.TrelloURL, loaded.TrelloURL)
	}
}

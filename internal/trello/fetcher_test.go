package trello

import (
	"encoding/json"
	"testing"
)

func TestParseCardInfo(t *testing.T) {
	jsonStr := `{"cardNumber":"1234","title":"Fix login bug","status":"In-progress"}`
	var info CardInfo
	err := json.Unmarshal([]byte(jsonStr), &info)
	if err != nil {
		t.Fatalf("failed to unmarshal valid JSON: %v", err)
	}
	if info.CardNumber != "1234" {
		t.Errorf("expected CardNumber 1234, got %s", info.CardNumber)
	}
	if info.Title != "Fix login bug" {
		t.Errorf("expected Title 'Fix login bug', got %s", info.Title)
	}
	if info.Status != "In-progress" {
		t.Errorf("expected Status 'In-progress', got %s", info.Status)
	}
}

func TestParseCardInfoInvalidJSON(t *testing.T) {
	jsonStr := `not valid json`
	var info CardInfo
	err := json.Unmarshal([]byte(jsonStr), &info)
	if err == nil {
		t.Error("expected error for invalid JSON, got nil")
	}
}

func TestFetchCardCommandFailure(t *testing.T) {
	// Integration test: would require real trello binary or exec mocking.
	// Skipping since Go 1.22 lacks exec.FakeCommand.
	t.Skip("requires exec mocking - integration test with real trello binary needed")
}

func TestFetchCardSuccess(t *testing.T) {
	// Integration test: would require exec mocking.
	t.Skip("requires exec mocking - integration test with real trello binary needed")
}

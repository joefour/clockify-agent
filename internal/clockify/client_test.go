package clockify

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

func TestCreateEntry_Success(t *testing.T) {
	// Arrange
	expectedEntry := TimeEntry{
		ID:          "entry123",
		ProjectID:   "project456",
		TaskID:      "",
		Name:        "Test Entry",
		Description: "Test Description",
		Start:       "2026-04-05T09:00:00Z",
		End:         "2026-04-05T10:00:00Z",
	}

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		// Verify request method and URL
		if r.Method != http.MethodPost {
			t.Errorf("expected POST method, got %s", r.Method)
		}

		expectedPath := "/workspaces/ws123/time-entries"
		if r.URL.Path != expectedPath {
			t.Errorf("expected path %s, got %s", expectedPath, r.URL.Path)
		}

		// Verify auth header
		apiKey := r.Header.Get("X-Api-Key")
		if apiKey != "test-api-key" {
			t.Errorf("expected X-Api-Key header 'test-api-key', got '%s'", apiKey)
		}

		// Verify request body
		var req TimeEntryRequest
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
			t.Fatalf("failed to decode request body: %v", err)
		}

		if req.ProjectID != "project456" {
			t.Errorf("expected ProjectID 'project456', got '%s'", req.ProjectID)
		}
		if req.Name != "Test Entry" {
			t.Errorf("expected Name 'Test Entry', got '%s'", req.Name)
		}

		// Send response
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusCreated)
		json.NewEncoder(w).Encode(expectedEntry)
	}))
	defer server.Close()

	client := NewClient("test-api-key", "ws123")
	client.baseURL = server.URL
	client.httpClient = &http.Client{Timeout: 10 * time.Second}

	// Act
	entry := TimeEntryRequest{
		ProjectID:   "project456",
		Name:        "Test Entry",
		Description: "Test Description",
		Start:       "2026-04-05T09:00:00Z",
		End:         "2026-04-05T10:00:00Z",
	}

	result, err := client.CreateEntry(entry)

	// Assert
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	if result.ID != expectedEntry.ID {
		t.Errorf("expected ID '%s', got '%s'", expectedEntry.ID, result.ID)
	}
	if result.ProjectID != expectedEntry.ProjectID {
		t.Errorf("expected ProjectID '%s', got '%s'", expectedEntry.ProjectID, result.ProjectID)
	}
}

func TestCreateEntry_APIError(t *testing.T) {
	// Arrange
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusBadRequest)
		w.Header().Set("Content-Type", "application/json")
		w.Write([]byte(`{"message": "Invalid request"}`))
	}))
	defer server.Close()

	client := NewClient("test-api-key", "ws123")
	client.baseURL = server.URL
	client.httpClient = &http.Client{Timeout: 10 * time.Second}

	// Act
	entry := TimeEntryRequest{
		ProjectID: "project456",
		Name:      "Test Entry",
		Start:     "2026-04-05T09:00:00Z",
		End:       "2026-04-05T10:00:00Z",
	}

	_, err := client.CreateEntry(entry)

	// Assert
	if err == nil {
		t.Fatal("expected error, got nil")
	}
}

func TestListEntriesForDate(t *testing.T) {
	// Arrange
	expectedEntries := []TimeEntry{
		{
			ID:          "entry1",
			ProjectID:   "project1",
			Name:        "Entry 1",
			Description: "Desc 1",
			Start:       "2026-04-05T09:00:00Z",
			End:         "2026-04-05T10:00:00Z",
		},
		{
			ID:          "entry2",
			ProjectID:   "project2",
			Name:        "Entry 2",
			Description: "Desc 2",
			Start:       "2026-04-05T11:00:00Z",
			End:         "2026-04-05T12:00:00Z",
		},
	}

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		// Verify request method and URL
		if r.Method != http.MethodGet {
			t.Errorf("expected GET method, got %s", r.Method)
		}

		expectedPath := "/workspaces/ws123/time-entries"
		if r.URL.Path != expectedPath {
			t.Errorf("expected path %s, got %s", expectedPath, r.URL.Path)
		}

		// Verify query params
		query := r.URL.Query()
		start := query.Get("start")
		end := query.Get("end")
		if start != "2026-04-05" {
			t.Errorf("expected start '2026-04-05', got '%s'", start)
		}
		if end != "2026-04-05" {
			t.Errorf("expected end '2026-04-05', got '%s'", end)
		}

		// Send response
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		json.NewEncoder(w).Encode(expectedEntries)
	}))
	defer server.Close()

	client := NewClient("test-api-key", "ws123")
	client.baseURL = server.URL
	client.httpClient = &http.Client{Timeout: 10 * time.Second}

	// Act
	entries, err := client.ListEntriesForDate("2026-04-05")

	// Assert
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	if len(entries) != 2 {
		t.Fatalf("expected 2 entries, got %d", len(entries))
	}

	if entries[0].ID != "entry1" {
		t.Errorf("expected first entry ID 'entry1', got '%s'", entries[0].ID)
	}
	if entries[1].ID != "entry2" {
		t.Errorf("expected second entry ID 'entry2', got '%s'", entries[1].ID)
	}
}

func TestListProjects(t *testing.T) {
	// Arrange
	expectedProjects := []Project{
		{ID: "proj1", Name: "Project 1"},
		{ID: "proj2", Name: "Project 2"},
	}

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		// Verify request method and URL
		if r.Method != http.MethodGet {
			t.Errorf("expected GET method, got %s", r.Method)
		}

		expectedPath := "/workspaces/ws123/projects"
		if r.URL.Path != expectedPath {
			t.Errorf("expected path %s, got %s", expectedPath, r.URL.Path)
		}

		// Send response
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		json.NewEncoder(w).Encode(expectedProjects)
	}))
	defer server.Close()

	client := NewClient("test-api-key", "ws123")
	client.baseURL = server.URL
	client.httpClient = &http.Client{Timeout: 10 * time.Second}

	// Act
	projects, err := client.ListProjects()

	// Assert
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	if len(projects) != 2 {
		t.Fatalf("expected 2 projects, got %d", len(projects))
	}

	if projects[0].ID != "proj1" || projects[0].Name != "Project 1" {
		t.Errorf("expected first project {ID: 'proj1', Name: 'Project 1'}, got {ID: '%s', Name: '%s'}", projects[0].ID, projects[0].Name)
	}
	if projects[1].ID != "proj2" || projects[1].Name != "Project 2" {
		t.Errorf("expected second project {ID: 'proj2', Name: 'Project 2'}, got {ID: '%s', Name: '%s'}", projects[1].ID, projects[1].Name)
	}
}

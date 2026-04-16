package clockify

import (
	"bytes"
	"encoding/json"
	"fmt"
	"net/http"
	"time"
)

type Client struct {
	apiKey      string
	workspaceID string
	baseURL     string
	httpClient  *http.Client
}

func NewClient(apiKey, workspaceID string) *Client {
	return &Client{
		apiKey:      apiKey,
		workspaceID: workspaceID,
		baseURL:     "https://api.clockify.me/api/v1",
		httpClient:  &http.Client{Timeout: 10 * time.Second},
	}
}

func (c *Client) CreateEntry(entry TimeEntryRequest) (*TimeEntry, error) {
	url := fmt.Sprintf("%s/workspaces/%s/time-entries", c.baseURL, c.workspaceID)

	body, err := json.Marshal(entry)
	if err != nil {
		return nil, fmt.Errorf("failed to marshal request: %w", err)
	}

	req, err := http.NewRequest(http.MethodPost, url, bytes.NewReader(body))
	if err != nil {
		return nil, fmt.Errorf("failed to create request: %w", err)
	}

	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Api-Key", c.apiKey)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("failed to send request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode >= 400 {
		return nil, fmt.Errorf("API error: status %d", resp.StatusCode)
	}

	var result TimeEntry
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, fmt.Errorf("failed to decode response: %w", err)
	}

	return &result, nil
}

func (c *Client) ListEntriesForDate(date string) ([]TimeEntry, error) {
	return c.ListEntries(date, date)
}

func (c *Client) ListEntries(start, end string) ([]TimeEntry, error) {
	url := fmt.Sprintf("%s/workspaces/%s/time-entries?start=%s&end=%s", c.baseURL, c.workspaceID, start, end)

	req, err := http.NewRequest(http.MethodGet, url, nil)
	if err != nil {
		return nil, fmt.Errorf("failed to create request: %w", err)
	}

	req.Header.Set("X-Api-Key", c.apiKey)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("failed to send request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode >= 400 {
		return nil, fmt.Errorf("API error: status %d", resp.StatusCode)
	}

	var result []TimeEntry
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, fmt.Errorf("failed to decode response: %w", err)
	}

	return result, nil
}

// GetDetailedReport fetches time entries using the detailed report API
// NOTE: This endpoint requires special permissions and may return 403
func (c *Client) GetDetailedReport(start, end string) ([]TimeEntry, error) {
	url := fmt.Sprintf("%s/report/workspaces/%s/reports/detailed", c.baseURL, c.workspaceID)

	reqBody := DetailedReportRequest{
		DateRangeStart: start,
		DateRangeEnd:   end,
	}
	reqBody.DetailedFilter.Groups = []string{"PROJECT"}
	reqBody.Users = map[string]any{"contains": "ALL"}
	reqBody.Projects = map[string]any{"contains": "ALL"}

	body, err := json.Marshal(reqBody)
	if err != nil {
		return nil, fmt.Errorf("failed to marshal request: %w", err)
	}

	req, err := http.NewRequest(http.MethodPost, url, bytes.NewReader(body))
	if err != nil {
		return nil, fmt.Errorf("failed to create request: %w", err)
	}

	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Api-Key", c.apiKey)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("failed to send request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode >= 400 {
		return nil, fmt.Errorf("API error: status %d", resp.StatusCode)
	}

	var result DetailedReportResponse
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, fmt.Errorf("failed to decode response: %w", err)
	}

	return result.TimeEntries, nil
}

// ListEntriesForUser fetches time entries for a specific user (uses GET method)
// start and end should be in YYYY-MM-DD format
func (c *Client) ListEntriesForUser(userID, start, end string) ([]TimeEntry, error) {
	url := fmt.Sprintf("%s/workspaces/%s/user/%s/time-entries?start=%sT00:00:00Z&end=%sT23:59:59Z", c.baseURL, c.workspaceID, userID, start, end)

	req, err := http.NewRequest(http.MethodGet, url, nil)
	if err != nil {
		return nil, fmt.Errorf("failed to create request: %w", err)
	}

	req.Header.Set("X-Api-Key", c.apiKey)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("failed to send request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode >= 400 {
		return nil, fmt.Errorf("API error: status %d", resp.StatusCode)
	}

	var result []TimeEntry
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, fmt.Errorf("failed to decode response: %w", err)
	}

	return result, nil
}

func (c *Client) WorkspaceID() string {
	return c.workspaceID
}

func (c *Client) ListProjects() ([]Project, error) {
	url := fmt.Sprintf("%s/workspaces/%s/projects", c.baseURL, c.workspaceID)

	req, err := http.NewRequest(http.MethodGet, url, nil)
	if err != nil {
		return nil, fmt.Errorf("failed to create request: %w", err)
	}

	req.Header.Set("X-Api-Key", c.apiKey)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("failed to send request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode >= 400 {
		return nil, fmt.Errorf("API error: status %d", resp.StatusCode)
	}

	var result []Project
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, fmt.Errorf("failed to decode response: %w", err)
	}

	return result, nil
}

func (c *Client) ListTasks(projectID string) ([]Task, error) {
	url := fmt.Sprintf("%s/workspaces/%s/projects/%s/tasks", c.baseURL, c.workspaceID, projectID)

	req, err := http.NewRequest(http.MethodGet, url, nil)
	if err != nil {
		return nil, fmt.Errorf("failed to create request: %w", err)
	}

	req.Header.Set("X-Api-Key", c.apiKey)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("failed to send request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode >= 400 {
		return nil, fmt.Errorf("API error: status %d", resp.StatusCode)
	}

	var result []Task
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, fmt.Errorf("failed to decode response: %w", err)
	}

	return result, nil
}

func (c *Client) CreateTask(projectID, name string) (*Task, error) {
	url := fmt.Sprintf("%s/workspaces/%s/projects/%s/tasks", c.baseURL, c.workspaceID, projectID)

	body, err := json.Marshal(map[string]string{"name": name})
	if err != nil {
		return nil, fmt.Errorf("failed to marshal request: %w", err)
	}

	req, err := http.NewRequest(http.MethodPost, url, bytes.NewReader(body))
	if err != nil {
		return nil, fmt.Errorf("failed to create request: %w", err)
	}

	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Api-Key", c.apiKey)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("failed to send request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode >= 400 {
		return nil, fmt.Errorf("API error: status %d", resp.StatusCode)
	}

	var result Task
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, fmt.Errorf("failed to decode response: %w", err)
	}

	return &result, nil
}

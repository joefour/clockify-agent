package clockify

type TimeEntryRequest struct {
	ProjectID   string `json:"projectId"`
	TaskID      string `json:"taskId,omitempty"`
	Name        string `json:"name"`
	Description string `json:"description"`
	Start       string `json:"start"`
	End         string `json:"end"`
	Billable    bool   `json:"billable"`
}

type TimeInterval struct {
	Start string `json:"start"`
	End   string `json:"end"`
}

type TaskRef struct {
	ID   string `json:"id"`
	Name string `json:"name"`
}

type Task struct {
	ID        string `json:"id"`
	Name      string `json:"name"`
	ProjectID string `json:"projectId"`
}

type TimeEntry struct {
	ID          string       `json:"id"`
	ProjectID   string       `json:"projectId"`
	TaskID      string       `json:"taskId"`
	Name        string       `json:"name"`
	Description string       `json:"description"`
	TimeInterval TimeInterval `json:"timeInterval"`
	Task        *TaskRef     `json:"task,omitempty"`
}

type Project struct {
	ID   string `json:"id"`
	Name string `json:"name"`
}

// Detailed Report types
type DetailedReportRequest struct {
	DateRangeStart string `json:"dateRangeStart"`
	DateRangeEnd   string `json:"dateRangeEnd"`
	DetailedFilter struct {
		Groups []string `json:"groups"`
	} `json:"detailedFilter"`
	Users    map[string]any `json:"users"`
	Projects map[string]any `json:"projects"`
}

type DetailedReportResponse struct {
	TimeEntries []TimeEntry `json:"timeentries"`
}

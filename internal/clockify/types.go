package clockify

type TimeEntryRequest struct {
	ProjectID   string `json:"projectId"`
	TaskID      string `json:"taskId,omitempty"`
	Name        string `json:"name"`
	Description string `json:"description"`
	Start       string `json:"timeStart"`
	End         string `json:"timeEnd"`
}

type TimeEntry struct {
	ID          string `json:"id"`
	ProjectID   string `json:"projectId"`
	TaskID      string `json:"taskId"`
	Name        string `json:"name"`
	Description string `json:"description"`
	Start       string `json:"timeStart"`
	End         string `json:"timeEnd"`
}

type Project struct {
	ID   string `json:"id"`
	Name string `json:"name"`
}

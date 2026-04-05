package trello

import (
	"encoding/json"
	"os/exec"
)

func FetchCard(url string) (*CardInfo, error) {
	cmd := exec.Command("trello", "card", url)
	stdout, err := cmd.Output()
	if err != nil {
		return nil, err
	}
	var info CardInfo
	if err := json.Unmarshal(stdout, &info); err != nil {
		return nil, err
	}
	return &info, nil
}

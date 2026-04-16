package trello

import (
	"encoding/json"
	"fmt"
	"os/exec"
	"strings"

	"github.com/loithai/clockify-agent/internal/config"
)

func FetchCard(url string) (*CardInfo, error) {
	shortID, cardNumber := extractIDs(url)

	// Use trello-agent meta to get card title via LLM
	metaCmd := exec.Command("trello-agent", "meta", url)
	metaOutput, err := metaCmd.Output()
	if err == nil {
		lines := strings.Split(string(metaOutput), "\n")
		if len(lines) > 0 {
			title := strings.TrimPrefix(lines[0], "# ")
			return fetchCardWithAPI(shortID, cardNumber, title)
		}
	}

	// Fallback to basic trello command
	return fetchCardBasic(url, cardNumber)
}

func extractIDs(url string) (shortID, cardNumber string) {
	// Handle both short and long Trello URLs:
	// Short: https://trello.com/c/QCxVCjm3
	// Long:  https://trello.com/c/QCxVCjm3/320-use-caseid-to-validate-group-family-members
	parts := strings.Split(url, "/c/")
	if len(parts) < 2 {
		return "", ""
	}
	pathAfterC := strings.Split(parts[1], "/")

	shortID = pathAfterC[0]

	// Long URL has card number in path: /c/shortid/320-title-slug
	if len(pathAfterC) >= 2 {
		titleSlug := pathAfterC[1]
		// Extract number before first dash
		numParts := strings.SplitN(titleSlug, "-", 2)
		cardNumber = numParts[0]
		if cardNumber == "" {
			cardNumber = shortID
		}
	} else {
		cardNumber = shortID
	}

	return shortID, cardNumber
}

func fetchCardWithAPI(shortID, cardNumber, title string) (*CardInfo, error) {
	cfg, err := config.Load()
	if err != nil {
		return &CardInfo{CardNumber: cardNumber, Title: title}, nil
	}

	apiKey := cfg.TrelloAPIKey
	apiToken := cfg.TrelloToken

	info := &CardInfo{CardNumber: cardNumber, Title: title}

	if apiKey == "" || apiToken == "" || shortID == "" {
		return info, nil
	}

	// Get card with board ID
	cardCmd := exec.Command("curl", "-s",
		fmt.Sprintf("https://api.trello.com/1/cards/%s?key=%s&token=%s", shortID, apiKey, apiToken))
	cardOut, err := cardCmd.Output()
	if err == nil {
		var cardResult struct {
			Name    string `json:"name"`
			IDBoard string `json:"idBoard"`
		}
		if err := json.Unmarshal(cardOut, &cardResult); err == nil {
			info.BoardID = cardResult.IDBoard
			if info.Title == "" {
				info.Title = cardResult.Name
			}
		}
	}

	// If we have board ID, get board name
	if info.BoardID != "" {
		boardCmd := exec.Command("curl", "-s",
			fmt.Sprintf("https://api.trello.com/1/boards/%s?key=%s&token=%s", info.BoardID, apiKey, apiToken))
		boardOut, err := boardCmd.Output()
		if err == nil {
			var boardResult struct {
				Name string `json:"name"`
			}
			if err := json.Unmarshal(boardOut, &boardResult); err == nil {
				info.BoardName = boardResult.Name
			}
		}
	}

	return info, nil
}

func fetchCardBasic(url, cardNumber string) (*CardInfo, error) {
	cmd := exec.Command("trello", "card", url)
	stdout, err := cmd.Output()
	if err != nil {
		return nil, err
	}
	var info CardInfo
	if err := json.Unmarshal(stdout, &info); err != nil {
		return nil, err
	}
	if cardNumber != "" {
		info.CardNumber = cardNumber
	}
	return &info, nil
}

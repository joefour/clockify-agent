package trello

type CardInfo struct {
	CardNumber string `json:"cardNumber"`
	Title      string `json:"title"`
	Status     string `json:"status"`
	BoardID    string `json:"boardId"`
	BoardName  string `json:"boardName"`
}

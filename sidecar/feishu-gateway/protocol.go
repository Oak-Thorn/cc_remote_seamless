package main

import (
	"encoding/json"
	"fmt"
)

// Command - messages FROM Tauri TO Go sidecar
type Command struct {
	Type      string          `json:"type"`
	AppID     string          `json:"app_id,omitempty"`
	AppSecret string          `json:"app_secret,omitempty"`
	ChatID    string          `json:"chat_id,omitempty"`
	Text      string          `json:"text,omitempty"`
	Card      json.RawMessage `json:"card,omitempty"`
}

// Event - messages FROM Go sidecar TO Tauri
type Event struct {
	Type      string `json:"type"`
	ChatID    string `json:"chat_id,omitempty"`
	Text      string `json:"text,omitempty"`
	Sender    string `json:"sender,omitempty"`
	MessageID string `json:"message_id,omitempty"`
	Reason    string `json:"reason,omitempty"`
	Message   string `json:"message,omitempty"`
}

func encodeEvent(e Event) string {
	b, _ := json.Marshal(e)
	return string(b)
}

func decodeCommand(line string) (Command, error) {
	var cmd Command
	err := json.Unmarshal([]byte(line), &cmd)
	if err != nil {
		return cmd, fmt.Errorf("decode command: %w", err)
	}
	return cmd, nil
}

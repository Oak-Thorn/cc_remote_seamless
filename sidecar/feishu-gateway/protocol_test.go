package main

import (
	"testing"
)

func TestEncodeEvent(t *testing.T) {
	e := Event{Type: "connected"}
	result := encodeEvent(e)
	if result != `{"type":"connected"}` {
		t.Errorf("unexpected: %s", result)
	}
}

func TestDecodeCommand(t *testing.T) {
	line := `{"type":"connect","app_id":"cli_123","app_secret":"sec_456"}`
	cmd, err := decodeCommand(line)
	if err != nil {
		t.Fatal(err)
	}
	if cmd.Type != "connect" || cmd.AppID != "cli_123" || cmd.AppSecret != "sec_456" {
		t.Errorf("unexpected: %+v", cmd)
	}
}

func TestDecodeCommandInvalid(t *testing.T) {
	_, err := decodeCommand("not json")
	if err == nil {
		t.Error("expected error for invalid JSON")
	}
}

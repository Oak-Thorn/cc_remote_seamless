package main

import (
	"bufio"
	"fmt"
	"os"
)

func main() {
	var client *FeishuClient
	scanner := bufio.NewScanner(os.Stdin)

	for scanner.Scan() {
		line := scanner.Text()
		cmd, err := decodeCommand(line)
		if err != nil {
			emitEvent(Event{Type: "error", Message: err.Error()})
			continue
		}

		switch cmd.Type {
		case "connect":
			client = NewFeishuClient(cmd.AppID, cmd.AppSecret, func(chatID, text, sender, messageID string) {
				emitEvent(Event{
					Type:      "message_received",
					ChatID:    chatID,
					Text:      text,
					Sender:    sender,
					MessageID: messageID,
				})
			})
			if err := client.Connect(); err != nil {
				emitEvent(Event{Type: "error", Message: err.Error()})
			} else {
				emitEvent(Event{Type: "connected"})
			}

		case "send_text":
			if client == nil {
				emitEvent(Event{Type: "error", Message: "not connected"})
				continue
			}
			if err := client.SendText(cmd.ChatID, cmd.Text); err != nil {
				emitEvent(Event{Type: "error", Message: err.Error()})
			}

		case "send_card":
			if client == nil {
				emitEvent(Event{Type: "error", Message: "not connected"})
				continue
			}
			if err := client.SendCard(cmd.ChatID, string(cmd.Card)); err != nil {
				emitEvent(Event{Type: "error", Message: err.Error()})
			}

		case "disconnect":
			if client != nil {
				client.Disconnect()
				client = nil
			}
			emitEvent(Event{Type: "disconnected", Reason: "requested"})

		default:
			emitEvent(Event{Type: "error", Message: fmt.Sprintf("unknown command: %s", cmd.Type)})
		}
	}
}

func emitEvent(e Event) {
	fmt.Println(encodeEvent(e))
}

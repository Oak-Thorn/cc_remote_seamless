package main

import (
	"context"
	"encoding/json"
	"fmt"
	"sync"

	lark "github.com/larksuite/oapi-sdk-go/v3"
	larkcore "github.com/larksuite/oapi-sdk-go/v3/core"
	"github.com/larksuite/oapi-sdk-go/v3/event/dispatcher"
	larkim "github.com/larksuite/oapi-sdk-go/v3/service/im/v1"
	larkws "github.com/larksuite/oapi-sdk-go/v3/ws"
)

type FeishuClient struct {
	apiClient *lark.Client
	wsClient  *larkws.Client
	onMessage func(chatID, text, sender, messageID string)
	cancel    context.CancelFunc
	mu        sync.Mutex
}

func NewFeishuClient(appID, appSecret string, onMessage func(chatID, text, sender, messageID string)) *FeishuClient {
	apiClient := lark.NewClient(appID, appSecret)

	eventHandler := dispatcher.NewEventDispatcher("", "")
	eventHandler.OnP2MessageReceiveV1(func(ctx context.Context, event *larkim.P2MessageReceiveV1) error {
		msg := event.Event.Message
		if msg == nil || msg.ChatId == nil || msg.MessageId == nil {
			return nil
		}
		chatID := *msg.ChatId
		msgID := *msg.MessageId
		sender := ""
		if event.Event.Sender != nil && event.Event.Sender.SenderId != nil && event.Event.Sender.SenderId.OpenId != nil {
			sender = *event.Event.Sender.SenderId.OpenId
		}

		msgType := ""
		if msg.MessageType != nil {
			msgType = *msg.MessageType
		}
		if msgType != "text" {
			return nil
		}

		var content struct {
			Text string `json:"text"`
		}
		if msg.Content != nil {
			_ = json.Unmarshal([]byte(*msg.Content), &content)
		}

		if onMessage != nil {
			onMessage(chatID, content.Text, sender, msgID)
		}
		return nil
	})

	wsClient := larkws.NewClient(appID, appSecret,
		larkws.WithEventHandler(eventHandler),
		larkws.WithLogLevel(larkcore.LogLevelInfo),
	)

	return &FeishuClient{
		apiClient: apiClient,
		wsClient:  wsClient,
		onMessage: onMessage,
	}
}

func (c *FeishuClient) Connect() error {
	c.mu.Lock()
	defer c.mu.Unlock()

	ctx, cancel := context.WithCancel(context.Background())
	c.cancel = cancel

	errCh := make(chan error, 1)
	go func() {
		errCh <- c.wsClient.Start(ctx)
	}()

	// wsClient.Start returns nil quickly on success (connection established),
	// or an error if initial handshake fails.
	// We give it a short window to detect immediate failures.
	select {
	case err := <-errCh:
		if err != nil {
			cancel()
			return fmt.Errorf("websocket connect failed: %w", err)
		}
	default:
	}

	return nil
}

func (c *FeishuClient) SendText(chatID, text string) error {
	content, _ := json.Marshal(map[string]string{"text": text})
	req := larkim.NewCreateMessageReqBuilder().
		ReceiveIdType("chat_id").
		Body(larkim.NewCreateMessageReqBodyBuilder().
			ReceiveId(chatID).
			MsgType("text").
			Content(string(content)).
			Build()).
		Build()

	resp, err := c.apiClient.Im.Message.Create(context.Background(), req)
	if err != nil {
		return fmt.Errorf("send failed: %w", err)
	}
	if !resp.Success() {
		return fmt.Errorf("send failed: code=%d msg=%s", resp.Code, resp.Msg)
	}
	return nil
}

func (c *FeishuClient) SendCard(chatID, cardJSON string) error {
	req := larkim.NewCreateMessageReqBuilder().
		ReceiveIdType("chat_id").
		Body(larkim.NewCreateMessageReqBodyBuilder().
			ReceiveId(chatID).
			MsgType("interactive").
			Content(cardJSON).
			Build()).
		Build()

	resp, err := c.apiClient.Im.Message.Create(context.Background(), req)
	if err != nil {
		return fmt.Errorf("send card failed: %w", err)
	}
	if !resp.Success() {
		return fmt.Errorf("send card failed: code=%d msg=%s", resp.Code, resp.Msg)
	}
	return nil
}

func (c *FeishuClient) Disconnect() {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.cancel != nil {
		c.cancel()
		c.cancel = nil
	}
}

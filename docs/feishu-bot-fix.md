# 飞书 Bot 发送消息修复方案

## 问题描述

飞书机器人无法发送消息给用户。表现为：调用 `bot.sendText()` 后飞书端收不到任何消息，且无错误日志输出。

## 根因分析

通过逐层诊断（凭证 → Token → 发送 API），定位到三个叠加问题：

### 问题 1：WebSocket 连接方式错误（根本原因）

原实现直接连接一个不存在的端点：

```javascript
// 错误 — 该 URL 不是飞书合法的 WebSocket 端点
new WebSocket(`wss://open.feishu.cn/open-apis/bot/v2/ws?app_id=...&app_secret=...`);
```

飞书长连接模式需要通过官方 SDK (`@larksuiteoapi/node-sdk`) 的 `WSClient` 来建立连接。SDK 内部会先通过 HTTP 获取 gateway URL，再建立 WebSocket 通道并处理鉴权、心跳、重连。

**后果：** WebSocket 连接无法建立 → 收不到任何消息事件 → `chatId` 永远为 null。

### 问题 2：`chatId` 为 null 导致发送失败

`chatId` 仅在收到飞书消息事件时被赋值。由于 WebSocket 连接本身就失败了，`chatId` 永远为 null，调用发送 API 时 `receive_id: null` 会被飞书服务端拒绝。

### 问题 3：API 错误被静默吞掉

原 `_sendViaApi` 方法不检查返回的 `code` 字段，飞书返回的错误响应（如 `{"code": 99991400, "msg": "..."}` ）被忽略，开发者无法察觉问题。

## 修复方案

### 核心改动：引入官方 SDK

**新增依赖：**

```bash
npm install @larksuiteoapi/node-sdk
```

**重写 `src/feishu/bot.js`：**

| 改动点 | 原实现 | 新实现 |
|--------|--------|--------|
| 接收消息 | 手动 `ws` 连接无效端点 | `lark.WSClient` + `EventDispatcher` |
| 发送消息 | 手动 `https.request` + 手动 token 管理 | `client.im.message.create`（SDK 自动管理 token） |
| 错误处理 | 静默吞掉 | `console.error` 输出 + `emit('error')` |
| chatId 保护 | 无 | 空值检查 + 明确错误提示 |

### 关键代码

```javascript
const lark = require('@larksuiteoapi/node-sdk');

// 创建 API 客户端（自动管理 tenant_access_token）
this.client = new lark.Client({ appId, appSecret });

// 创建 WebSocket 长连接客户端（接收事件）
this.wsClient = new lark.WSClient({ appId, appSecret, loggerLevel: lark.LoggerLevel.info });
await this.wsClient.start({
  eventDispatcher: new lark.EventDispatcher({}).register({
    'im.message.receive_v1': async (data) => {
      const { chat_id, content, message_id } = data.message;
      this.chatId = chat_id;
      this.emit('message', { text: JSON.parse(content).text, messageId: message_id, chatId: chat_id });
    },
  }),
});

// 发送消息（SDK 处理 token 刷新）
const res = await this.client.im.message.create({
  params: { receive_id_type: 'chat_id' },
  data: { receive_id: chatId, content: JSON.stringify({ text }), msg_type: 'text' },
});
if (res.code !== 0) console.error(`[FeishuBot] send failed: code=${res.code} msg=${res.msg}`);
```

## 飞书开放平台配置要求

使用长连接模式接收事件的前提配置：

1. 飞书开放平台 → 应用 → 事件与回调 → 订阅方式选择 **长连接模式**（非 Webhook）
2. 添加事件订阅：`im.message.receive_v1`
3. 权限：`im:message`、`im:message:send_as_bot`
4. 加密密钥（Encrypt Key）留空 — 长连接模式不需要

## 诊断验证

Token 获取已通过独立脚本验证（凭证有效）：

```
=== Layer 2: Token Refresh ===
HTTP Status: 200
Response code: 0
Token: t-g1045mkw...
✅ Token works.
```

发送功能依赖 `chatId`，需要先从飞书收到一条消息后才能回复（这是正常行为 — 机器人不能主动给未对话过的用户发消息）。

## 测试覆盖

`tests/feishu/bot.test.js` — 4 个测试全部通过：

- `emits connected after connect()` — 连接建立
- `emits message on text event` — 事件接收
- `sendText calls client.im.message.create` — 发送调用
- `sendText logs error when no chatId` — 空 chatId 保护

## 影响范围

| 文件 | 改动 |
|------|------|
| `package.json` | 新增 `@larksuiteoapi/node-sdk` 依赖 |
| `src/feishu/bot.js` | 完全重写（89行 → 70行） |
| `tests/feishu/bot.test.js` | 重写 mock 适配新 SDK |

其他模块（`FeishuBridge`、`SessionRouter`、`PtyManager`）无需改动 — `bot.js` 对外接口不变（`connect()`、`sendText()`、`disconnect()`、`on('message')`）。

# Slash Commands

## Overview

When a user sends a message starting with `/` from an IM platform (Feishu or Telegram), the system intercepts it and executes a local command instead of injecting it into the agent. The result is replied back to the chat.

## Commands

| Command | Description | Example |
|---|---|---|
| `/status` | Show current bound session state | `/status` |
| `/list` | List all active sessions | `/list` |
| `/switch <id>` | Switch to a different session (prefix match) | `/switch abc123` |
| `/change [agent]` | Switch agent type (no args = list available) | `/change pi` |
| `/mute` | Suppress agent→IM message forwarding | `/mute` |
| `/unmute` | Resume agent→IM message forwarding | `/unmute` |
| `/full` | Show last agent output untruncated | `/full` |
| `/p <text>` | Send prompt to current session (checks busy state) | `/p what model are you` |
| `/t <text>` | Test message, log only | `/t hello` |
| `/allow` | Allow pending permission request | `/allow` |
| `/deny` | Deny pending permission request | `/deny` |
| `/always` | Allow-always pending permission (writes rule) | `/always` |
| `/answer <n>` | Answer a question (single/multi select) | `/answer 1` |
| `/clear` | Clear pending input for current session | `/clear` |
| `/stop` | Stop current session | `/stop` |
| `/radar` | Rediscover running agents, add missing sessions | `/radar` |
| `/skill <query>` | Search available skills | `/skill debug` |
| `/help` | Show command list | `/help` |

## Architecture

```
IM message arrives (Feishu / Telegram)
       │
       ▼
 starts with '/'?
    ┌───┴───┐
    │ yes   │ no
    ▼       ▼
 slash.rs   inject_input (existing path)
 execute()
    │
    ├─→ SlashResult::Reply(text)         → send_text back to chat
    ├─→ SlashResult::Inject(text)        → inject into agent session
    ├─→ SlashResult::BindingChanged{..}  → reply + emit binding event
    └─→ SlashResult::Noop               → do nothing
```

## Implementation

### Files

- `src-tauri/src/engine/slash.rs` — command parser + executor
- `src-tauri/src/engine/mod.rs` — integrate slash routing, mute check
- `src-tauri/src/engine/router.rs` — BindingStore (mute, pin, last_output)

### Mute Behavior

When muted, `handle_agent_event` skips forwarding output to platforms for that chat. The output is still stored in `last_output` so `/full` works.

### Permission Commands

`/allow`, `/deny`, `/always` resolve the pending permission waiter for the session bound to the current chat. They use the same `PermissionWaiters` map as the desktop popup.

### Answer Command

`/answer` resolves Elicitation (AskUserQuestion) requests:
- `/answer 1` — select option 1
- `/answer 1 3` — multi-select options 1 and 3 (space-separated)
- `/answer 1,2,3` — multi-select (comma-separated)

### Change Agent

`/change` switches the agent type for the current binding:
- `/change` — lists available agents
- `/change pi` — switch to Pi agent (binds to most recent Pi session)
- `/change claude-code` — switch back to Claude Code

### Busy Check

`/p` checks if the target session is busy before injecting. If busy, it returns an error message instead of queuing.

### Error Feedback

When `/p` inject fails (no binding, agent not found, inject error), the failure reason is sent back to the IM chat so the user can diagnose the problem.

### Radar

`/radar` triggers `rediscover()` on all registered AgentConnectors, which re-scans session registry files (`~/.cc-remote/pi-sessions/` for Pi, `~/.claude/sessions/` for Claude Code). Sessions whose processes are still alive are added to the session list. This is useful when the app starts after agents are already running, or when a session was missed during initial discovery.

### Pin Behavior

Sessions can be "pinned" via the desktop UI (`pin_session` command). Pinned bindings are not overwritten by the auto-bind logic that follows Hook events.

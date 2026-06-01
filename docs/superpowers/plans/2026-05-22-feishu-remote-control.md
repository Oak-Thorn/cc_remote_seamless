# Feishu Remote Control Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Feishu remote control to clawd-on-desk, allowing users to send prompts to and receive replies from a running Claude Code / Codex CLI session via Feishu private bot chat.

**Architecture:** PTY Proxy (`clawd-wrap`) wraps Agent CLI in a pseudo-terminal, exposes I/O via Unix Socket. clawd-on-desk connects to the socket for read/write, and bridges to Feishu via WebSocket long-connection. Hook events provide Agent state awareness for input timing.

**Tech Stack:** Electron (existing), Node.js, node-pty, `ws`, `strip-ansi`, Feishu Open Platform WebSocket API

---

## File Structure

```
bin/
  clawd-wrap                  # CLI entry point (npm bin executable)
src/
  pty/
    proxy.js                  # PTY spawn + Unix Socket server (runs in clawd-wrap process)
    manager.js                # Connects to PTY sockets, tracks sessions (runs in Electron main)
    protocol.js               # Shared message type constants and helpers
  feishu/
    bot.js                    # Feishu WebSocket connection, auth, message send/receive
    commands.js               # Slash command registry and execution
    formatter.js              # ANSI stripping, summarization, output formatting
    index.js                  # FeishuBridge — orchestrates bot + router + pty
    boot.js                   # Initialization and lifecycle management
  session-router.js           # Coordinates Hook state + PTY state, controls injection timing
tests/
  pty/
    proxy.test.js
    manager.test.js
    protocol.test.js
  feishu/
    bot.test.js
    commands.test.js
    formatter.test.js
  session-router.test.js
  integration/
    feishu-to-agent.test.js
    multi-session.test.js
    e2e-smoke.test.js
```

---

## Phase 1 — MVP (Core Pipeline)

### Task 1: Project Setup

**Files:**
- Modify: `package.json`
- Create: `jest.config.js`
- Create: `src/pty/protocol.js`
- Create: `tests/pty/protocol.test.js`

- [ ] **Step 1: Fork and clone clawd-on-desk**

```bash
git clone https://github.com/rullerzhou-afk/clawd-on-desk.git .
git remote rename origin upstream
git remote add origin <your-repo-url>
```

- [ ] **Step 2: Install new dependencies**

```bash
npm install node-pty ws strip-ansi
npm install --save-dev jest
```

- [ ] **Step 3: Add test script to package.json**

In `package.json`, add to `"scripts"`:
```json
{
  "scripts": {
    "test": "jest --forceExit",
    "test:watch": "jest --watch"
  }
}
```

- [ ] **Step 4: Create jest.config.js**

```javascript
// jest.config.js
module.exports = {
  testEnvironment: 'node',
  testMatch: ['**/tests/**/*.test.js'],
};
```

- [ ] **Step 5: Write protocol module test**

```javascript
// tests/pty/protocol.test.js
const { MessageType, encode, decode } = require('../../src/pty/protocol');

describe('protocol', () => {
  test('encode/decode input message', () => {
    const msg = { type: MessageType.INPUT, data: 'hello\n' };
    const encoded = encode(msg);
    const decoded = decode(encoded);
    expect(decoded).toEqual(msg);
  });

  test('encode/decode output message', () => {
    const msg = { type: MessageType.OUTPUT, data: 'response text' };
    const encoded = encode(msg);
    const decoded = decode(encoded);
    expect(decoded).toEqual(msg);
  });

  test('encode/decode state message', () => {
    const msg = { type: MessageType.STATE, waiting: true };
    const encoded = encode(msg);
    const decoded = decode(encoded);
    expect(decoded).toEqual(msg);
  });

  test('decode handles incomplete frames gracefully', () => {
    const partial = Buffer.from('{"type":"inp');
    expect(() => decode(partial)).toThrow();
  });
});
```

- [ ] **Step 6: Run test to verify it fails**

Run: `npx jest tests/pty/protocol.test.js`
Expected: FAIL — module not found

- [ ] **Step 7: Implement protocol module**

```javascript
// src/pty/protocol.js
const DELIMITER = '\n';

const MessageType = {
  INPUT: 'input',
  OUTPUT: 'output',
  RESIZE: 'resize',
  KILL: 'kill',
  EXIT: 'exit',
  STATE: 'state',
};

function encode(msg) {
  return JSON.stringify(msg) + DELIMITER;
}

function decode(buf) {
  const str = typeof buf === 'string' ? buf : buf.toString('utf-8');
  return JSON.parse(str.trim());
}

module.exports = { MessageType, encode, decode, DELIMITER };
```

- [ ] **Step 8: Run test to verify it passes**

Run: `npx jest tests/pty/protocol.test.js`
Expected: PASS (4 tests)

- [ ] **Step 9: Commit**

```bash
git add src/pty/protocol.js tests/pty/protocol.test.js jest.config.js package.json package-lock.json
git commit -m "feat: add PTY socket protocol module with encode/decode"
```

---

### Task 2: PTY Proxy (clawd-wrap)

**Files:**
- Create: `src/pty/proxy.js`
- Create: `bin/clawd-wrap`
- Create: `tests/pty/proxy.test.js`

- [ ] **Step 1: Write proxy test**

```javascript
// tests/pty/proxy.test.js
const net = require('net');
const path = require('path');
const { spawn } = require('child_process');
const { MessageType, encode, decode, DELIMITER } = require('../../src/pty/protocol');

const CLAWD_WRAP = path.resolve(__dirname, '../../bin/clawd-wrap');

describe('PTY Proxy', () => {
  let proc;
  let socketPath;

  afterEach((done) => {
    if (proc && !proc.killed) {
      proc.kill('SIGTERM');
      proc.on('exit', done);
    } else {
      done();
    }
  });

  test('starts agent and creates unix socket', (done) => {
    proc = spawn('node', [CLAWD_WRAP, '--agent', 'echo', '--', 'echo', 'hello'], {
      env: { ...process.env, CLAWD_PTY_DIR: '/tmp' },
    });

    proc.stderr.on('data', (data) => {
      const line = data.toString().trim();
      if (line.startsWith('SOCKET:')) {
        socketPath = line.replace('SOCKET:', '');
        expect(socketPath).toMatch(/^\/tmp\/clawd-pty-/);
        done();
      }
    });
  }, 5000);

  test('relays output over socket', (done) => {
    proc = spawn('node', [CLAWD_WRAP, '--agent', 'echo', '--', 'echo', 'hello'], {
      env: { ...process.env, CLAWD_PTY_DIR: '/tmp' },
    });

    proc.stderr.on('data', (data) => {
      const line = data.toString().trim();
      if (!line.startsWith('SOCKET:')) return;
      socketPath = line.replace('SOCKET:', '');

      setTimeout(() => {
        const client = net.createConnection(socketPath, () => {
          let buf = '';
          client.on('data', (chunk) => {
            buf += chunk.toString();
            const lines = buf.split(DELIMITER).filter(Boolean);
            for (const l of lines) {
              const msg = JSON.parse(l);
              if (msg.type === MessageType.OUTPUT && msg.data.includes('hello')) {
                client.end();
                done();
                return;
              }
            }
          });
        });
      }, 200);
    });
  }, 5000);

  test('accepts input over socket', (done) => {
    proc = spawn('node', [CLAWD_WRAP, '--agent', 'cat', '--', 'cat'], {
      env: { ...process.env, CLAWD_PTY_DIR: '/tmp' },
    });

    proc.stderr.on('data', (data) => {
      const line = data.toString().trim();
      if (!line.startsWith('SOCKET:')) return;
      socketPath = line.replace('SOCKET:', '');

      setTimeout(() => {
        const client = net.createConnection(socketPath, () => {
          client.write(encode({ type: MessageType.INPUT, data: 'injected\n' }));

          let buf = '';
          client.on('data', (chunk) => {
            buf += chunk.toString();
            if (buf.includes('injected')) {
              client.end();
              done();
            }
          });
        });
      }, 200);
    });
  }, 5000);
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx jest tests/pty/proxy.test.js`
Expected: FAIL — bin/clawd-wrap not found

- [ ] **Step 3: Implement proxy.js**

```javascript
// src/pty/proxy.js
const pty = require('node-pty');
const net = require('net');
const fs = require('fs');
const crypto = require('crypto');
const { MessageType, encode, DELIMITER } = require('./protocol');

class PtyProxy {
  constructor({ command, args, agent, socketDir }) {
    this.agent = agent;
    this.sessionId = crypto.randomBytes(4).toString('hex');
    this.socketPath = `${socketDir}/clawd-pty-${agent}-${this.sessionId}.sock`;
    this.clients = new Set();
    this.lastInputTime = 0;
    this._idleTimer = null;

    this.proc = pty.spawn(command, args, {
      name: 'xterm-256color',
      cols: process.stdout.columns || 80,
      rows: process.stdout.rows || 24,
      cwd: process.cwd(),
      env: process.env,
    });

    this._setupOutputRelay();
    this._setupSocketServer();
    this._setupSignals();
  }

  _setupOutputRelay() {
    this.proc.onData((data) => {
      process.stdout.write(data);

      const msg = encode({ type: MessageType.OUTPUT, data });
      for (const client of this.clients) {
        client.write(msg);
      }

      // Idle detection: no output for 500ms → signal waiting
      clearTimeout(this._idleTimer);
      this._idleTimer = setTimeout(() => {
        const stateMsg = encode({ type: MessageType.STATE, waiting: true });
        for (const client of this.clients) {
          client.write(stateMsg);
        }
      }, 500);
    });

    this.proc.onExit(({ exitCode }) => {
      clearTimeout(this._idleTimer);
      const msg = encode({ type: MessageType.EXIT, code: exitCode });
      for (const client of this.clients) {
        client.write(msg);
        client.end();
      }
      this._cleanup();
      process.exit(exitCode);
    });
  }

  _setupSocketServer() {
    if (fs.existsSync(this.socketPath)) {
      fs.unlinkSync(this.socketPath);
    }

    this.server = net.createServer((client) => {
      this.clients.add(client);
      client.on('data', (raw) => {
        const lines = raw.toString().split(DELIMITER).filter(Boolean);
        for (const line of lines) {
          try {
            const msg = JSON.parse(line);
            this._handleClientMessage(msg);
          } catch (e) {}
        }
      });
      client.on('close', () => this.clients.delete(client));
      client.on('error', () => this.clients.delete(client));
    });

    this.server.listen(this.socketPath, () => {
      process.stderr.write(`SOCKET:${this.socketPath}\n`);
    });
  }

  _handleClientMessage(msg) {
    switch (msg.type) {
      case MessageType.INPUT:
        this.proc.write(msg.data);
        break;
      case MessageType.RESIZE:
        this.proc.resize(msg.cols, msg.rows);
        break;
      case MessageType.KILL:
        this.proc.kill();
        break;
    }
  }

  _setupSignals() {
    process.stdout.on('resize', () => {
      const cols = process.stdout.columns;
      const rows = process.stdout.rows;
      this.proc.resize(cols, rows);
    });

    if (process.stdin.isTTY) {
      process.stdin.setRawMode(true);
    }
    process.stdin.resume();
    process.stdin.on('data', (data) => {
      this.lastInputTime = Date.now();
      this.proc.write(data.toString());
      // Notify clients of terminal activity
      const msg = encode({ type: MessageType.STATE, waiting: false, terminalActive: true });
      for (const client of this.clients) {
        client.write(msg);
      }
    });

    process.on('SIGTERM', () => this._cleanup());
    process.on('SIGINT', () => {
      this.proc.write('\x03');
    });
  }

  _cleanup() {
    try { fs.unlinkSync(this.socketPath); } catch (e) {}
    this.server.close();
  }
}

module.exports = { PtyProxy };
```

- [ ] **Step 4: Implement bin/clawd-wrap**

```javascript
#!/usr/bin/env node
// bin/clawd-wrap
const { PtyProxy } = require('../src/pty/proxy');

const args = process.argv.slice(2);

let agent = 'claude';
let command;
let commandArgs;

const agentIdx = args.indexOf('--agent');
if (agentIdx !== -1) {
  agent = args[agentIdx + 1];
  args.splice(agentIdx, 2);
}

const dashIdx = args.indexOf('--');
if (dashIdx !== -1) {
  command = args[dashIdx + 1];
  commandArgs = args.slice(dashIdx + 2);
} else {
  command = agent;
  commandArgs = args;
}

const socketDir = process.env.CLAWD_PTY_DIR || '/tmp';

new PtyProxy({ command, args: commandArgs, agent, socketDir });
```

- [ ] **Step 5: Make executable and add bin field**

```bash
chmod +x bin/clawd-wrap
```

In `package.json`:
```json
{ "bin": { "clawd-wrap": "./bin/clawd-wrap" } }
```

- [ ] **Step 6: Run test to verify it passes**

Run: `npx jest tests/pty/proxy.test.js`
Expected: PASS (3 tests)

- [ ] **Step 7: Commit**

```bash
git add src/pty/proxy.js bin/clawd-wrap tests/pty/proxy.test.js package.json
git commit -m "feat: add clawd-wrap PTY proxy with Unix Socket I/O"
```

---

### Task 3: PTY Manager (Electron Side)

**Files:**
- Create: `src/pty/manager.js`
- Create: `tests/pty/manager.test.js`

- [ ] **Step 1: Write manager test**

```javascript
// tests/pty/manager.test.js
const net = require('net');
const fs = require('fs');
const { PtyManager } = require('../../src/pty/manager');
const { MessageType, encode, DELIMITER } = require('../../src/pty/protocol');

describe('PtyManager', () => {
  let manager;
  let mockServer;
  let socketPath;

  beforeEach((done) => {
    socketPath = `/tmp/clawd-pty-claude-test${Date.now()}.sock`;
    manager = new PtyManager({ socketDir: '/tmp' });

    mockServer = net.createServer((conn) => {
      conn.on('data', (raw) => {
        const lines = raw.toString().split(DELIMITER).filter(Boolean);
        for (const line of lines) {
          const msg = JSON.parse(line);
          if (msg.type === MessageType.INPUT) {
            conn.write(encode({ type: MessageType.OUTPUT, data: msg.data }));
          }
        }
      });
    });
    mockServer.listen(socketPath, done);
  });

  afterEach((done) => {
    manager.disconnectAll();
    mockServer.close(() => {
      try { fs.unlinkSync(socketPath); } catch (e) {}
      done();
    });
  });

  test('connects to a socket and registers session', async () => {
    await manager.connect(socketPath);
    const sessions = manager.getSessions();
    expect(sessions.length).toBe(1);
    expect(sessions[0].agent).toBe('claude');
  });

  test('sends input to connected session', async () => {
    const session = await manager.connect(socketPath);
    const output = await new Promise((resolve) => {
      session.on('output', (data) => resolve(data));
      manager.sendInput(session.id, 'test message\n');
    });
    expect(output).toContain('test message');
  });

  test('getActiveSession returns current active', async () => {
    await manager.connect(socketPath);
    const active = manager.getActiveSession();
    expect(active).toBeTruthy();
    expect(active.agent).toBe('claude');
  });

  test('removes session on socket disconnect', async () => {
    await manager.connect(socketPath);
    expect(manager.getSessions().length).toBe(1);
    mockServer.close();
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(manager.getSessions().length).toBe(0);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx jest tests/pty/manager.test.js`
Expected: FAIL — module not found

- [ ] **Step 3: Implement manager.js**

```javascript
// src/pty/manager.js
const net = require('net');
const path = require('path');
const fs = require('fs');
const EventEmitter = require('events');
const { MessageType, encode, DELIMITER } = require('./protocol');

class Session extends EventEmitter {
  constructor({ id, agent, socketPath, socket }) {
    super();
    this.id = id;
    this.agent = agent;
    this.socketPath = socketPath;
    this.socket = socket;
    this.state = 'idle';
    this.lastOutputBuffer = '';
    this._buf = '';

    socket.on('data', (raw) => {
      this._buf += raw.toString();
      const lines = this._buf.split(DELIMITER);
      this._buf = lines.pop();
      for (const line of lines) {
        if (!line) continue;
        try {
          this._handleMessage(JSON.parse(line));
        } catch (e) {}
      }
    });

    socket.on('close', () => this.emit('disconnected'));
    socket.on('error', () => this.emit('disconnected'));
  }

  _handleMessage(msg) {
    switch (msg.type) {
      case MessageType.OUTPUT:
        this.lastOutputBuffer += msg.data;
        this.emit('output', msg.data);
        break;
      case MessageType.EXIT:
        this.emit('exit', msg.code);
        break;
      case MessageType.STATE:
        this.state = msg.waiting ? 'idle' : 'busy';
        if (msg.terminalActive) this.emit('terminalActive');
        this.emit('stateChange', this.state);
        break;
    }
  }

  write(data) {
    this.socket.write(encode({ type: MessageType.INPUT, data }));
  }

  clearBuffer() {
    const content = this.lastOutputBuffer;
    this.lastOutputBuffer = '';
    return content;
  }
}

class PtyManager extends EventEmitter {
  constructor({ socketDir = '/tmp' } = {}) {
    super();
    this.socketDir = socketDir;
    this.sessions = new Map();
    this.activeSessionId = null;
  }

  async connect(socketPath) {
    return new Promise((resolve, reject) => {
      const socket = net.createConnection(socketPath, () => {
        const filename = path.basename(socketPath);
        const match = filename.match(/^clawd-pty-(\w+)-(\w+)\.sock$/);
        const agent = match ? match[1] : 'unknown';
        const id = match ? `${match[1]}-${match[2]}` : filename;

        const session = new Session({ id, agent, socketPath, socket });
        this.sessions.set(id, session);

        if (!this.activeSessionId) {
          this.activeSessionId = id;
        }

        session.on('disconnected', () => {
          this.sessions.delete(id);
          if (this.activeSessionId === id) {
            this.activeSessionId = this.sessions.keys().next().value || null;
          }
          this.emit('sessionRemoved', id);
        });

        this.emit('sessionAdded', session);
        resolve(session);
      });
      socket.on('error', reject);
    });
  }

  sendInput(sessionId, data) {
    const session = this.sessions.get(sessionId);
    if (session) session.write(data);
  }

  getSessions() {
    return Array.from(this.sessions.values());
  }

  getActiveSession() {
    return this.sessions.get(this.activeSessionId) || null;
  }

  setActiveSession(sessionId) {
    if (this.sessions.has(sessionId)) {
      this.activeSessionId = sessionId;
      return true;
    }
    return false;
  }

  async discover() {
    const files = fs.readdirSync(this.socketDir)
      .filter(f => f.startsWith('clawd-pty-') && f.endsWith('.sock'));
    const results = [];
    for (const f of files) {
      const socketPath = path.join(this.socketDir, f);
      if (this._alreadyConnected(socketPath)) continue;
      try {
        const session = await this.connect(socketPath);
        results.push(session);
      } catch (e) {
        try { fs.unlinkSync(socketPath); } catch (e) {}
      }
    }
    return results;
  }

  _alreadyConnected(socketPath) {
    for (const s of this.sessions.values()) {
      if (s.socketPath === socketPath) return true;
    }
    return false;
  }

  disconnectAll() {
    for (const session of this.sessions.values()) {
      session.socket.end();
    }
    this.sessions.clear();
    this.activeSessionId = null;
  }
}

module.exports = { PtyManager, Session };
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx jest tests/pty/manager.test.js`
Expected: PASS (4 tests)

- [ ] **Step 5: Commit**

```bash
git add src/pty/manager.js tests/pty/manager.test.js
git commit -m "feat: add PTY Manager for session tracking and socket communication"
```

---

### Task 4: Feishu Bot Module

**Files:**
- Create: `src/feishu/bot.js`
- Create: `tests/feishu/bot.test.js`

- [ ] **Step 1: Write bot test**

```javascript
// tests/feishu/bot.test.js
const { FeishuBot } = require('../../src/feishu/bot');

jest.mock('ws', () => {
  const EventEmitter = require('events');
  class MockWebSocket extends EventEmitter {
    constructor(url) {
      super();
      this.url = url;
      this.sent = [];
      this.readyState = 1;
      MockWebSocket.instances.push(this);
      setTimeout(() => this.emit('open'), 10);
    }
    send(data) { this.sent.push(JSON.parse(data)); }
    close() { this.emit('close'); }
    removeAllListeners() { return this; }
  }
  MockWebSocket.instances = [];
  MockWebSocket.OPEN = 1;
  return MockWebSocket;
});

describe('FeishuBot', () => {
  let bot;

  beforeEach(() => {
    require('ws').instances = [];
    bot = new FeishuBot({ appId: 'test_id', appSecret: 'test_secret' });
  });

  afterEach(() => { bot.disconnect(); });

  test('emits connected after WebSocket opens', (done) => {
    bot.on('connected', () => done());
    bot.connect();
  });

  test('emits message on text event', (done) => {
    bot.on('connected', () => {
      const ws = require('ws').instances[0];
      bot.on('message', (msg) => {
        expect(msg.text).toBe('hello');
        expect(msg.messageId).toBe('msg_001');
        done();
      });
      ws.emit('message', JSON.stringify({
        header: { event_type: 'im.message.receive_v1' },
        event: {
          message: {
            message_id: 'msg_001',
            message_type: 'text',
            content: JSON.stringify({ text: 'hello' }),
            chat_id: 'chat_001',
          },
        },
      }));
    });
    bot.connect();
  });

  test('sendText queues message', (done) => {
    bot.on('connected', () => {
      bot.sendText('reply', 'chat_001');
      expect(bot._pendingReplies.length).toBe(1);
      done();
    });
    bot.connect();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx jest tests/feishu/bot.test.js`
Expected: FAIL — module not found

- [ ] **Step 3: Implement bot.js**

```javascript
// src/feishu/bot.js
const EventEmitter = require('events');
const https = require('https');

class FeishuBot extends EventEmitter {
  constructor({ appId, appSecret }) {
    super();
    this.appId = appId;
    this.appSecret = appSecret;
    this.ws = null;
    this.token = null;
    this.chatId = null;
    this._pendingReplies = [];
    this._reconnectDelay = 1000;
    this._maxReconnectDelay = 60000;
  }

  async connect() {
    const WebSocket = require('ws');
    this.ws = new WebSocket(
      `wss://open.feishu.cn/open-apis/bot/v2/ws?app_id=${this.appId}&app_secret=${this.appSecret}`
    );

    this.ws.on('open', () => {
      this._reconnectDelay = 1000;
      this.emit('connected');
    });

    this.ws.on('message', (raw) => {
      try {
        const data = JSON.parse(raw.toString());
        this._handleEvent(data);
      } catch (e) {
        this.emit('error', e);
      }
    });

    this.ws.on('close', () => {
      this.emit('disconnected');
      this._scheduleReconnect();
    });

    this.ws.on('error', (err) => this.emit('error', err));
  }

  _handleEvent(data) {
    if (data.header?.event_type === 'im.message.receive_v1') {
      const message = data.event?.message;
      if (!message || message.message_type !== 'text') return;
      const content = JSON.parse(message.content);
      this.chatId = message.chat_id || this.chatId;
      this.emit('message', {
        text: content.text,
        messageId: message.message_id,
        chatId: this.chatId,
      });
    }
  }

  sendText(text, chatId) {
    const target = chatId || this.chatId;
    if (!this.token) {
      this._pendingReplies.push({ text, chatId: target });
      this._refreshAndFlush(target);
      return;
    }
    this._sendViaApi(text, target);
  }

  async _refreshAndFlush(chatId) {
    try {
      await this._refreshToken();
      while (this._pendingReplies.length) {
        const { text, chatId: cid } = this._pendingReplies.shift();
        await this._sendViaApi(text, cid || chatId);
      }
    } catch (e) {
      this.emit('error', e);
    }
  }

  _sendViaApi(text, chatId) {
    const body = JSON.stringify({
      receive_id: chatId,
      msg_type: 'text',
      content: JSON.stringify({ text }),
    });

    return new Promise((resolve, reject) => {
      const req = https.request({
        hostname: 'open.feishu.cn',
        path: '/open-apis/im/v1/messages?receive_id_type=chat_id',
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Authorization': `Bearer ${this.token}`,
        },
      }, (res) => {
        let data = '';
        res.on('data', (c) => { data += c; });
        res.on('end', () => resolve(JSON.parse(data)));
      });
      req.on('error', reject);
      req.write(body);
      req.end();
    });
  }

  async _refreshToken() {
    return new Promise((resolve, reject) => {
      const req = https.request({
        hostname: 'open.feishu.cn',
        path: '/open-apis/auth/v3/tenant_access_token/internal',
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
      }, (res) => {
        let data = '';
        res.on('data', (c) => { data += c; });
        res.on('end', () => {
          this.token = JSON.parse(data).tenant_access_token;
          resolve();
        });
      });
      req.on('error', reject);
      req.write(JSON.stringify({ app_id: this.appId, app_secret: this.appSecret }));
      req.end();
    });
  }

  _scheduleReconnect() {
    setTimeout(() => {
      this._reconnectDelay = Math.min(this._reconnectDelay * 2, this._maxReconnectDelay);
      this.connect();
    }, this._reconnectDelay);
  }

  disconnect() {
    if (this.ws) {
      this.ws.removeAllListeners();
      this.ws.close();
      this.ws = null;
    }
  }
}

module.exports = { FeishuBot };
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx jest tests/feishu/bot.test.js`
Expected: PASS (3 tests)

- [ ] **Step 5: Commit**

```bash
git add src/feishu/bot.js tests/feishu/bot.test.js
git commit -m "feat: add Feishu Bot with WebSocket connection and message handling"
```

---

### Task 5: Formatter Module

**Files:**
- Create: `src/feishu/formatter.js`
- Create: `tests/feishu/formatter.test.js`

- [ ] **Step 1: Write formatter test**

```javascript
// tests/feishu/formatter.test.js
const { formatReply, stripAnsi } = require('../../src/feishu/formatter');

describe('formatter', () => {
  test('strips ANSI codes', () => {
    expect(stripAnsi('\x1b[32mOK\x1b[0m')).toBe('OK');
  });

  test('short reply returned as-is', () => {
    const r = formatReply('short');
    expect(r.summary).toBe('short');
    expect(r.truncated).toBe(false);
  });

  test('long reply truncated with head + tail', () => {
    const r = formatReply('A'.repeat(600));
    expect(r.truncated).toBe(true);
    expect(r.summary).toContain('...');
    expect(r.summary).toContain('/full');
  });

  test('code block over 10 lines is truncated', () => {
    const lines = Array.from({ length: 20 }, (_, i) => `line ${i}`);
    const text = '```js\n' + lines.join('\n') + '\n```';
    const r = formatReply(text);
    expect(r.summary).toContain('line 0');
    expect(r.summary).toContain('line 9');
    expect(r.summary).not.toContain('line 15');
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx jest tests/feishu/formatter.test.js`
Expected: FAIL

- [ ] **Step 3: Implement formatter.js**

```javascript
// src/feishu/formatter.js
const stripAnsiLib = require('strip-ansi');

const MAX_SHORT = 500;
const HEAD_CHARS = 200;
const TAIL_CHARS = 100;
const MAX_CODE_LINES = 10;

function stripAnsi(text) {
  return stripAnsiLib(text);
}

function formatReply(rawText) {
  const text = stripAnsi(rawText).trim();

  if (text.length <= MAX_SHORT) {
    return { summary: truncateCodeBlocks(text), full: text, truncated: false };
  }

  const head = text.slice(0, HEAD_CHARS);
  const tail = text.slice(-TAIL_CHARS);
  const summary = `${head}\n...\n${tail}\n(共 ${text.length} 字，发 /full 查看完整)`;
  return { summary, full: text, truncated: true };
}

function truncateCodeBlocks(text) {
  return text.replace(/```(\w*)\n([\s\S]*?)```/g, (match, lang, code) => {
    const lines = code.split('\n');
    if (lines.length <= MAX_CODE_LINES) return match;
    const truncated = lines.slice(0, MAX_CODE_LINES).join('\n');
    return `\`\`\`${lang}\n${truncated}\n... (${lines.length - MAX_CODE_LINES} lines omitted)\n\`\`\``;
  });
}

module.exports = { formatReply, stripAnsi, truncateCodeBlocks };
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx jest tests/feishu/formatter.test.js`
Expected: PASS (4 tests)

- [ ] **Step 5: Commit**

```bash
git add src/feishu/formatter.js tests/feishu/formatter.test.js
git commit -m "feat: add output formatter with ANSI stripping and summarization"
```

---

### Task 6: Session Router

**Files:**
- Create: `src/session-router.js`
- Create: `tests/session-router.test.js`

- [ ] **Step 1: Write session-router test**

```javascript
// tests/session-router.test.js
const EventEmitter = require('events');
const { SessionRouter } = require('../src/session-router');

class MockSession extends EventEmitter {
  constructor(id, agent) {
    super();
    this.id = id;
    this.agent = agent;
    this.state = 'idle';
    this.lastInput = null;
  }
  write(data) { this.lastInput = data; }
  clearBuffer() { return 'buffered'; }
}

describe('SessionRouter', () => {
  let router;
  let mockManager;
  let session;

  beforeEach(() => {
    session = new MockSession('claude-abc', 'claude');
    mockManager = {
      sessions: new Map([['claude-abc', session]]),
      activeSessionId: 'claude-abc',
      getActiveSession() { return this.sessions.get(this.activeSessionId); },
    };
    router = new SessionRouter({ ptyManager: mockManager });
  });

  test('injects when idle', () => {
    const r = router.inject('hello\n');
    expect(r.status).toBe('sent');
    expect(session.lastInput).toBe('hello\n');
  });

  test('queues when busy', () => {
    session.state = 'busy';
    const r = router.inject('hello\n');
    expect(r.status).toBe('queued');
    expect(session.lastInput).toBeNull();
  });

  test('flushes on idle', (done) => {
    session.state = 'busy';
    router.inject('msg\n');
    router.on('injected', (d) => { expect(d).toBe('msg\n'); done(); });
    session.state = 'idle';
    router.onAgentIdle('claude-abc');
  });

  test('conflict when terminal active', () => {
    router.reportTerminalActivity('claude-abc');
    const r = router.inject('x\n');
    expect(r.status).toBe('conflict');
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx jest tests/session-router.test.js`
Expected: FAIL

- [ ] **Step 3: Implement session-router.js**

```javascript
// src/session-router.js
const EventEmitter = require('events');

const TERMINAL_ACTIVITY_WINDOW = 2000;

class SessionRouter extends EventEmitter {
  constructor({ ptyManager }) {
    super();
    this.ptyManager = ptyManager;
    this.queue = [];
    this.terminalActivity = new Map();
  }

  inject(data) {
    const session = this.ptyManager.getActiveSession();
    if (!session) return { status: 'no_session' };

    const lastActivity = this.terminalActivity.get(session.id) || 0;
    if (Date.now() - lastActivity < TERMINAL_ACTIVITY_WINDOW) {
      return { status: 'conflict', message: '桌面端正在输入，已排队' };
    }

    if (session.state === 'idle') {
      session.write(data);
      this.emit('injected', data);
      return { status: 'sent' };
    }

    this.queue.push({ sessionId: session.id, data });
    return { status: 'queued', position: this.queue.length };
  }

  onAgentIdle(sessionId) {
    const idx = this.queue.findIndex(q => q.sessionId === sessionId);
    if (idx === -1) return;
    const { data } = this.queue.splice(idx, 1)[0];
    const session = this.ptyManager.sessions.get(sessionId);
    if (session) {
      session.write(data);
      this.emit('injected', data);
    }
  }

  reportTerminalActivity(sessionId) {
    this.terminalActivity.set(sessionId, Date.now());
  }
}

module.exports = { SessionRouter };
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx jest tests/session-router.test.js`
Expected: PASS (4 tests)

- [ ] **Step 5: Commit**

```bash
git add src/session-router.js tests/session-router.test.js
git commit -m "feat: add SessionRouter with queue and conflict detection"
```

---

### Task 7: FeishuBridge + Integration

**Files:**
- Create: `src/feishu/index.js`
- Create: `src/feishu/boot.js`
- Create: `tests/integration/feishu-to-agent.test.js`

- [ ] **Step 1: Write integration test**

```javascript
// tests/integration/feishu-to-agent.test.js
const net = require('net');
const fs = require('fs');
const { PtyManager } = require('../../src/pty/manager');
const { SessionRouter } = require('../../src/session-router');
const { FeishuBridge } = require('../../src/feishu/index');
const { MessageType, encode, DELIMITER } = require('../../src/pty/protocol');

describe('Integration: Feishu → Agent', () => {
  let mockServer, socketPath, ptyManager, router, bridge, received;

  beforeEach(async () => {
    socketPath = `/tmp/clawd-pty-claude-integ${Date.now()}.sock`;
    received = [];
    mockServer = net.createServer((conn) => {
      conn.on('data', (raw) => {
        for (const line of raw.toString().split(DELIMITER).filter(Boolean)) {
          const msg = JSON.parse(line);
          if (msg.type === MessageType.INPUT) received.push(msg.data);
        }
      });
    });
    await new Promise(r => mockServer.listen(socketPath, r));
    ptyManager = new PtyManager({ socketDir: '/tmp' });
    await ptyManager.connect(socketPath);
    router = new SessionRouter({ ptyManager });
    bridge = new FeishuBridge({ ptyManager, router });
  });

  afterEach((done) => {
    ptyManager.disconnectAll();
    mockServer.close(() => { try { fs.unlinkSync(socketPath); } catch(e){} done(); });
  });

  test('text injected to agent', () => {
    ptyManager.getActiveSession().state = 'idle';
    const r = bridge.handleFeishuMessage({ text: 'do it' });
    expect(r.status).toBe('sent');
    expect(received).toContain('do it\n');
  });

  test('slash command not injected', () => {
    ptyManager.getActiveSession().state = 'idle';
    const r = bridge.handleFeishuMessage({ text: '/status' });
    expect(r.status).toBe('command');
    expect(received.length).toBe(0);
  });

  test('queued when busy', () => {
    ptyManager.getActiveSession().state = 'busy';
    const r = bridge.handleFeishuMessage({ text: 'do it' });
    expect(r.status).toBe('queued');
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx jest tests/integration/feishu-to-agent.test.js`
Expected: FAIL — FeishuBridge not found

- [ ] **Step 3: Implement feishu/index.js**

```javascript
// src/feishu/index.js
const { formatReply } = require('./formatter');

class FeishuBridge {
  constructor({ ptyManager, router, bot = null }) {
    this.ptyManager = ptyManager;
    this.router = router;
    this.bot = bot;
    this.muted = false;
    this.lastFullReply = '';
  }

  handleFeishuMessage({ text }) {
    if (text.startsWith('/')) {
      const reply = this._handleCommand(text);
      return { status: 'command', reply };
    }

    const result = this.router.inject(text + '\n');
    const replyMap = {
      sent: '✓ 已发送',
      queued: `⏳ Agent 忙碌中，已排队（位置: ${result.position || 1}）`,
      conflict: `⚠️ ${result.message}`,
      no_session: '❌ 无活跃 session，请先在终端启动 Agent',
    };
    return { status: result.status, reply: replyMap[result.status] || '❌ 未知错误' };
  }

  _handleCommand(text) {
    const [cmd, ...args] = text.split(' ');
    switch (cmd) {
      case '/status': {
        const s = this.ptyManager.getActiveSession();
        return s ? `${s.agent} · ${s.id} · ${s.state}` : '无活跃 session';
      }
      case '/sessions': {
        const list = this.ptyManager.getSessions();
        if (!list.length) return '无活跃 session';
        return list.map(s =>
          `${s.id === this.ptyManager.activeSessionId ? '→ ' : '  '}${s.agent} · ${s.id} · ${s.state}`
        ).join('\n');
      }
      case '/switch': {
        if (!args[0]) return '用法: /switch <agent>';
        const match = this.ptyManager.getSessions().find(s => s.agent === args[0] || s.id === args[0]);
        if (!match) return `未找到: ${args[0]}`;
        this.ptyManager.setActiveSession(match.id);
        return `已切换到 ${match.agent} · ${match.id}`;
      }
      case '/mute': this.muted = true; return '已静音，发 /unmute 恢复';
      case '/unmute': this.muted = false; return '已恢复推送';
      case '/full': return this.lastFullReply || '暂无完整回复';
      case '/help': return '/status · /sessions · /switch <agent> · /mute · /unmute · /full';
      default: return `未知命令: ${cmd}，发 /help 查看帮助`;
    }
  }

  pushReplyToFeishu(rawOutput) {
    if (this.muted || !this.bot) return;
    const { summary, full } = formatReply(rawOutput);
    this.lastFullReply = full;
    this.bot.sendText(summary);
  }
}

module.exports = { FeishuBridge };
```

- [ ] **Step 4: Implement feishu/boot.js**

```javascript
// src/feishu/boot.js
const { FeishuBot } = require('./bot');
const { FeishuBridge } = require('./index');
const { PtyManager } = require('../pty/manager');
const { SessionRouter } = require('../session-router');

class FeishuRemoteControl {
  constructor({ appId, appSecret, socketDir = '/tmp', hookServer = null }) {
    this.ptyManager = new PtyManager({ socketDir });
    this.router = new SessionRouter({ ptyManager: this.ptyManager });
    this.bot = new FeishuBot({ appId, appSecret });
    this.bridge = new FeishuBridge({ ptyManager: this.ptyManager, router: this.router, bot: this.bot });
    this.hookServer = hookServer;
    this._discoveryInterval = null;
    this._bindEvents();
  }

  _bindEvents() {
    this.bot.on('message', (msg) => {
      const result = this.bridge.handleFeishuMessage(msg);
      if (result.reply) this.bot.sendText(result.reply);
    });

    if (this.hookServer) {
      this.hookServer.on('hookEvent', (event) => {
        const session = this.ptyManager.getActiveSession();
        if (!session) return;
        if (event.type === 'Stop') {
          session.state = 'idle';
          this.router.onAgentIdle(session.id);
          const output = session.clearBuffer();
          if (output) this.bridge.pushReplyToFeishu(output);
        } else if (event.type === 'UserPromptSubmit') {
          session.state = 'busy';
        }
      });
    }

    this.ptyManager.on('sessionAdded', (session) => {
      session.on('terminalActive', () => {
        this.router.reportTerminalActivity(session.id);
      });
      session.on('stateChange', (state) => {
        if (state === 'idle') this.router.onAgentIdle(session.id);
      });
    });
  }

  async start() {
    await this.ptyManager.discover();
    await this.bot.connect();
    this._discoveryInterval = setInterval(() => this.ptyManager.discover(), 5000);
  }

  stop() {
    clearInterval(this._discoveryInterval);
    this.bot.disconnect();
    this.ptyManager.disconnectAll();
  }
}

module.exports = { FeishuRemoteControl };
```

- [ ] **Step 5: Run tests**

Run: `npx jest`
Expected: All PASS

- [ ] **Step 6: Commit**

```bash
git add src/feishu/index.js src/feishu/boot.js tests/integration/feishu-to-agent.test.js
git commit -m "feat: add FeishuBridge and boot module — MVP complete"
```

---

## Phase 2 — Settings UI & E2E

### Task 8: Settings UI

**Files:**
- Modify: existing settings HTML/JS (locate via grep)

- [ ] **Step 1: Find settings file**

```bash
grep -r "settings" src/ --include="*.html" -l
```

- [ ] **Step 2: Add Feishu config section**

```html
<div class="settings-section" id="feishu-settings">
  <h3>飞书远程控制</h3>
  <div class="setting-item">
    <label>App ID</label>
    <input type="text" id="feishu-app-id" placeholder="cli_axxxxxxxxx" />
  </div>
  <div class="setting-item">
    <label>App Secret</label>
    <input type="password" id="feishu-app-secret" />
  </div>
  <div class="setting-item">
    <span id="feishu-status">未连接</span>
    <button id="feishu-test-btn">测试连接</button>
  </div>
</div>
```

- [ ] **Step 3: Add IPC handler in main.js**

```javascript
const { ipcMain } = require('electron');
const { FeishuRemoteControl } = require('./feishu/boot');

let feishu = null;

ipcMain.on('feishu:save', async (event, { appId, appSecret }) => {
  store.set('feishu', { appId, appSecret });
  if (feishu) feishu.stop();
  feishu = new FeishuRemoteControl({ appId, appSecret, hookServer });
  try {
    await feishu.start();
    event.reply('feishu:status', { connected: true });
  } catch (err) {
    event.reply('feishu:status', { connected: false });
  }
});
```

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: add Feishu config UI in Settings panel"
```

---

### Task 9: E2E Smoke Test

**Files:**
- Create: `tests/integration/e2e-smoke.test.js`

- [ ] **Step 1: Write E2E test**

```javascript
// tests/integration/e2e-smoke.test.js
const { spawn } = require('child_process');
const net = require('net');
const path = require('path');
const { DELIMITER, MessageType } = require('../../src/pty/protocol');

const CLAWD_WRAP = path.resolve(__dirname, '../../bin/clawd-wrap');

describe('E2E', () => {
  let proc;

  afterEach((done) => {
    if (proc && !proc.killed) { proc.kill('SIGTERM'); proc.on('exit', done); }
    else done();
  });

  test('inject input → receive echoed output via socket', (done) => {
    proc = spawn('node', [
      CLAWD_WRAP, '--agent', 'test', '--',
      'node', '-e', 'process.stdin.resume();process.stdin.on("data",d=>process.stdout.write("ECHO:"+d))',
    ], { env: { ...process.env, CLAWD_PTY_DIR: '/tmp' } });

    proc.stderr.on('data', (data) => {
      const line = data.toString().trim();
      if (!line.startsWith('SOCKET:')) return;
      const sock = line.replace('SOCKET:', '');
      setTimeout(() => {
        const c = net.createConnection(sock, () => {
          c.write(JSON.stringify({ type: MessageType.INPUT, data: 'hi\n' }) + '\n');
          let buf = '';
          c.on('data', (chunk) => {
            buf += chunk.toString();
            if (buf.includes('ECHO:hi')) { c.end(); done(); }
          });
        });
      }, 300);
    });
  }, 10000);
});
```

- [ ] **Step 2: Run test**

Run: `npx jest tests/integration/e2e-smoke.test.js`
Expected: PASS

- [ ] **Step 3: Run full suite**

Run: `npx jest --forceExit`
Expected: All PASS

- [ ] **Step 4: Commit**

```bash
git add tests/integration/e2e-smoke.test.js
git commit -m "test: add E2E smoke test for full pipeline"
```

---

## Summary

| Phase | Tasks | Delivers |
|-------|-------|----------|
| Phase 1 | Tasks 1-7 | MVP: Feishu ↔ Agent injection + reply push + slash commands |
| Phase 2 | Tasks 8-9 | Settings UI + E2E verification |

**Total: 9 tasks, ~40 steps.** After Task 7 the core pipeline is functional for manual testing with a real Feishu bot.

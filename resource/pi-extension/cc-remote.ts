import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import * as http from "http";

function readHookPort(): number {
  const envPort = process.env.CC_REMOTE_HOOK_PORT;
  if (envPort) {
    const n = parseInt(envPort, 10);
    if (!Number.isNaN(n)) return n;
  }
  try {
    const cfgPath = path.join(os.homedir(), ".cc-remote", "config.toml");
    const text = fs.readFileSync(cfgPath, "utf8");
    const m = text.match(/hook_port\s*=\s*(\d+)/);
    if (m) return parseInt(m[1], 10);
  } catch {}
  return 23399;
}

const HOOK_PORT = readHookPort();
const BASE = `http://127.0.0.1:${HOOK_PORT}`;
const SESSION_ID = `pi-${process.pid}-${Date.now()}`;
const REGISTRY_DIR = path.join(os.homedir(), ".cc-remote", "pi-sessions");
const REGISTRY_FILE = path.join(REGISTRY_DIR, `${SESSION_ID}.json`);

async function post(endpoint: string, body: unknown, timeoutMs = 5000): Promise<any> {
  const ctrl = new AbortController();
  const timer = setTimeout(() => ctrl.abort(), timeoutMs);
  try {
    const res = await fetch(`${BASE}${endpoint}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
      signal: ctrl.signal,
    });
    if (!res.ok) return null;
    const ct = res.headers.get("content-type") || "";
    if (ct.includes("application/json")) return await res.json();
    return null;
  } catch {
    return null;
  } finally {
    clearTimeout(timer);
  }
}

function startInjectServer(pi: ExtensionAPI): number {
  const server = http.createServer((req, res) => {
    if (req.method !== "POST" || req.url !== "/inject") {
      res.statusCode = 404;
      res.end();
      return;
    }
    let body = "";
    req.on("data", (chunk) => (body += chunk));
    req.on("end", async () => {
      try {
        const { text } = JSON.parse(body || "{}");
        if (typeof text !== "string" || !text.length) {
          res.statusCode = 400;
          res.end(JSON.stringify({ error: "missing text" }));
          return;
        }
        await pi.sendUserMessage(text, { deliverAs: "followUp" });
        res.statusCode = 200;
        res.setHeader("content-type", "application/json");
        res.end(JSON.stringify({ ok: true }));
      } catch (e: any) {
        res.statusCode = 500;
        res.end(JSON.stringify({ error: String(e?.message ?? e) }));
      }
    });
  });
  server.listen(0, "127.0.0.1");
  const addr = server.address();
  const port = typeof addr === "object" && addr ? addr.port : 0;
  return port;
}

function writeRegistry(port: number) {
  try {
    fs.mkdirSync(REGISTRY_DIR, { recursive: true });
    fs.writeFileSync(
      REGISTRY_FILE,
      JSON.stringify({
        session_id: SESSION_ID,
        pid: process.pid,
        cwd: process.cwd(),
        inject_port: port,
        started_at: Date.now(),
      }),
    );
  } catch {}
}

function cleanupRegistry() {
  try {
    fs.unlinkSync(REGISTRY_FILE);
  } catch {}
}

export default function (pi: ExtensionAPI) {
  const cwd = process.cwd();
  const injectPort = startInjectServer(pi);
  writeRegistry(injectPort);

  post("/pi/session_start", {
    session_id: SESSION_ID,
    cwd,
    pid: process.pid,
    inject_port: injectPort,
  });

  const onExit = () => {
    cleanupRegistry();
    post("/pi/session_end", { session_id: SESSION_ID });
  };
  process.on("exit", onExit);
  for (const sig of ["SIGINT", "SIGTERM", "SIGHUP"] as const) {
    process.on(sig, () => {
      onExit();
    });
  }

  pi.on("session_shutdown", async () => {
    cleanupRegistry();
    post("/pi/session_end", { session_id: SESSION_ID });
  });

  pi.on("input", async (event: any) => {
    post("/pi/input", { session_id: SESSION_ID, cwd, text: event.text });
    return { action: "continue" };
  });

  pi.on("agent_start", async () => {
    post("/pi/agent_start", { session_id: SESSION_ID, cwd });
  });

  pi.on("agent_end", async () => {
    post("/pi/stop", { session_id: SESSION_ID, cwd });
  });

  pi.on("tool_call", async (event: any) => {
    const toolName = event.toolName ?? event.name ?? "unknown";
    const input = event.input ?? {};
    post("/pi/pre_tool", {
      session_id: SESSION_ID,
      cwd,
      tool_name: toolName,
      tool_input: input,
    });

    const decision = await post(
      "/pi/permission",
      { session_id: SESSION_ID, cwd, tool_name: toolName, tool_input: input },
      600_000,
    );
    if (decision && decision.behavior === "deny") {
      return { block: true, reason: decision.message ?? "Denied by cc-remote" };
    }
    return undefined;
  });

  pi.on("tool_result", async (event: any) => {
    post("/pi/post_tool", {
      session_id: SESSION_ID,
      cwd,
      tool_name: event.toolName ?? "unknown",
      is_error: Boolean(event.isError),
      output: event.output ?? null,
    });
  });

  pi.on("session_before_compact", async () => {
    post("/pi/pre_compact", { session_id: SESSION_ID, cwd });
  });

  pi.on("session_compact", async () => {
    post("/pi/post_compact", { session_id: SESSION_ID, cwd });
  });
}

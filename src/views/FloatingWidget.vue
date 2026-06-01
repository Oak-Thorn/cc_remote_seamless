<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from "vue";
import { useSessionStore } from "../stores/sessions";
import { useMessageStore } from "../stores/messages";
import { useSettingsStore, type FloatingIcon } from "../stores/settings";
import StatusBadge from "../components/StatusBadge.vue";
import FloatingIconView, { type FloatingIconColor } from "../components/icons/FloatingIcon.vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow, currentMonitor, PhysicalPosition, PhysicalSize } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";

const store = useSessionStore();
const msgStore = useMessageStore();
const settingsStore = useSettingsStore();
const selected = ref<string>("");
const collapsed = ref(true);
const customSvgContent = ref<string>("");
let expandLocked = false;
let unlisten: UnlistenFn | null = null;
let unlistenMessages: UnlistenFn | null = null;
let unlistenBinding: UnlistenFn | null = null;
let unlistenIcon: UnlistenFn | null = null;
let collapseTimer: number | null = null;

const EXPANDED_WIDTH = 330;
const EXPANDED_HEIGHT = 225;
const COLLAPSED_SIZE = 54;

function scheduleCollapse() {
  if (collapseTimer) clearTimeout(collapseTimer);
  collapseTimer = window.setTimeout(() => {
    collapsed.value = true;
    resizeWindow(COLLAPSED_SIZE, COLLAPSED_SIZE);
  }, 5000);
}

function expand() {
  if (expandLocked) return;
  if (collapseTimer) clearTimeout(collapseTimer);
  collapsed.value = false;
  resizeWindow(EXPANDED_WIDTH, EXPANDED_HEIGHT);
}

function onMouseLeave() {
  scheduleCollapse();
}

async function resizeWindow(w: number, h: number) {
  try {
    const win = getCurrentWindow();
    const monitor = await currentMonitor();
    const scaleFactor = monitor?.scaleFactor || 1;
    const pw = Math.round(w * scaleFactor);
    const ph = Math.round(h * scaleFactor);
    await win.setSize(new PhysicalSize(pw, ph));
    if (monitor) {
      const x = monitor.position.x + monitor.size.width - pw;
      const pos = await win.outerPosition();
      await win.setPosition(new PhysicalPosition(x, pos.y));
    }
  } catch (e) {
    console.warn("[FloatingWidget] resize failed:", e);
  }
}

const statusColor = computed<FloatingIconColor>(() => {
  const sessions = store.sessions;
  if (sessions.some(s => s.state === "WaitingPermission")) return "red";
  if (sessions.some(s => s.state === "Busy")) return "orange";
  return "green";
});

onMounted(async () => {
  store.refresh();
  msgStore.loadAll();
  invoke("set_sound_preference", { kind: "idle", name: settingsStore.idleSound }).catch(() => {});
  invoke("set_sound_preference", { kind: "permission", name: settingsStore.permissionSound }).catch(() => {});
  unlisten = await listen("sessions-updated", () => store.refresh());
  unlistenMessages = await listen("messages-updated", () => msgStore.loadAll());
  unlistenBinding = await listen<{ session_id: string }>("binding-changed", (event) => {
    selected.value = event.payload.session_id;
  });
  unlistenIcon = await listen<FloatingIcon>("floating-icon-changed", (event) => {
    settingsStore.setFloatingIcon(event.payload);
  });
  await loadCustomIconSvg();

  try {
    const win = getCurrentWindow();
    const monitor = await currentMonitor();
    if (monitor) {
      const scaleFactor = monitor.scaleFactor || 1;
      const x = monitor.position.x + monitor.size.width - Math.round(COLLAPSED_SIZE * scaleFactor);
      const y = monitor.position.y + Math.round(monitor.size.height / 3);
      await win.setSize(new PhysicalSize(Math.round(COLLAPSED_SIZE * scaleFactor), Math.round(COLLAPSED_SIZE * scaleFactor)));
      await win.setPosition(new PhysicalPosition(x, y));
    }
    await win.show();
  } catch (e) {
    console.warn("[FloatingWidget] window positioning failed:", e);
    getCurrentWindow().show();
  }

  scheduleCollapse();
});

onUnmounted(() => {
  if (collapseTimer) clearTimeout(collapseTimer);
  unlisten?.();
  unlistenMessages?.();
  unlistenBinding?.();
  unlistenIcon?.();
});

watch(selected, (id) => {
  if (id) {
    msgStore.loadForSession(id);
    invoke("pin_session", { sessionId: id }).catch((e) => {
      console.warn("[FloatingWidget] pin_session failed:", e);
    });
  }
});

function sessionName(s: { id: string; agent: string; workingDir: string | null }) {
  const dir = s.workingDir ? (s.workingDir.split("/").pop() || "~") : "~";
  return `${dir} [${s.agent}]`;
}

function shortContent(text: string) {
  const line = text.split("\n")[0];
  return line.length > 60 ? line.slice(0, 60) + "..." : line;
}

function closeWindow() {
  expandLocked = true;
  collapsed.value = true;
  resizeWindow(COLLAPSED_SIZE, COLLAPSED_SIZE);
  setTimeout(() => { expandLocked = false; }, 500);
}

function openSettings() {
  invoke("open_settings").catch((e) => {
    console.warn("[FloatingWidget] open_settings failed:", e);
  });
}

function openTerminal(sessionId: string) {
  invoke("open_terminal", { sessionId }).catch((e) => {
    console.error("[FloatingWidget] open_terminal failed:", e);
  });
}

async function loadCustomIconSvg() {
  const svg = await invoke<string>("read_icon_svg", { name: settingsStore.floatingIcon }).catch(() => "");
  customSvgContent.value = svg;
}

watch(() => settingsStore.floatingIcon, () => loadCustomIconSvg());
</script>

<template>
  <div v-if="collapsed" class="collapsed-dot" @mouseenter="expand">
    <FloatingIconView :icon="settingsStore.floatingIcon" :color="statusColor" :svgContent="customSvgContent" />
  </div>
  <div v-else class="float-widget" @mouseleave="onMouseLeave">
    <div class="titlebar" data-tauri-drag-region>
      <span class="titlebar-text" data-tauri-drag-region>CC Remote</span>
      <div class="titlebar-buttons">
        <button class="titlebar-btn" @click="openSettings" title="Settings">&#x2699;</button>
        <button class="titlebar-btn titlebar-btn-close" @click="closeWindow" title="Close">&#x2715;</button>
      </div>
    </div>
    <div class="session-list">
      <div v-if="store.sessions.length === 0" class="row">
        <span class="empty">no session</span>
      </div>
      <label v-for="s in store.sessions" :key="s.id" class="row">
        <input type="radio" :value="s.id" v-model="selected" class="radio" />
        <StatusBadge :state="s.state" />
        <span class="session-name">{{ sessionName(s) }}</span>
        <button class="open-term-btn" @click.prevent="openTerminal(s.id)" title="Open Terminal">&gt;_</button>
      </label>
    </div>
    <div class="message-panel">
      <div v-if="msgStore.messages.length === 0" class="msg-empty">no messages</div>
      <div v-for="m in msgStore.messages" :key="m.id" class="msg-row">
        <span class="msg-source">{{ m.source }}</span>
        <span class="msg-text">{{ shortContent(m.text) }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.collapsed-dot { width: 54px; height: 54px; display: flex; align-items: center; justify-content: center; cursor: pointer; transition: transform 0.2s; padding: 6px; }
.collapsed-dot:hover { transform: scale(1.15); }
.float-widget { background: #ffffff; border-radius: 10px; border: 1px solid #e2e8f0; overflow: hidden; display: flex; flex-direction: column; min-height: 80px; max-height: 450px; box-shadow: 0 4px 12px rgba(0,0,0,0.1); }
.titlebar { height: 34px; display: flex; align-items: center; justify-content: space-between; padding: 0 14px; background: #f8fafc; cursor: grab; user-select: none; -webkit-user-select: none; border-bottom: 1px solid #e2e8f0; flex-shrink: 0; }
.titlebar-text { font-size: 12px; font-weight: 500; color: #94a3b8; }
.titlebar-buttons { display: flex; gap: 4px; }
.titlebar-btn { border: none; background: transparent; cursor: pointer; width: 24px; height: 24px; border-radius: 4px; font-size: 13px; color: #64748b; display: flex; align-items: center; justify-content: center; }
.titlebar-btn:hover { background: #e2e8f0; }
.titlebar-btn-close:hover { background: #fecaca; color: #dc2626; }
.session-list { padding: 8px 14px; flex-shrink: 0; }
.row { display: flex; align-items: center; gap: 8px; height: 32px; cursor: pointer; }
.open-term-btn { border: none; background: transparent; cursor: pointer; width: 22px; height: 22px; border-radius: 4px; font-size: 11px; color: #94a3b8; display: flex; align-items: center; justify-content: center; margin-left: auto; flex-shrink: 0; font-family: monospace; }
.open-term-btn:hover { background: #e2e8f0; color: #3b82f6; }
.radio { width: 14px; height: 14px; accent-color: #3b82f6; margin: 0; }
.session-name { font-weight: 600; font-size: 13px; color: #1f2937; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex: 1; }
.empty { font-size: 13px; color: #94a3b8; }
.message-panel { border-top: 1px solid #e2e8f0; padding: 8px 14px; overflow-y: auto; flex: 1; max-height: 240px; }
.msg-empty { font-size: 12px; color: #94a3b8; padding: 4px 0; }
.msg-row { display: flex; gap: 8px; align-items: baseline; padding: 3px 0; font-size: 12px; }
.msg-source { font-weight: 600; color: #64748b; flex-shrink: 0; min-width: 40px; }
.msg-text { color: #374151; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>

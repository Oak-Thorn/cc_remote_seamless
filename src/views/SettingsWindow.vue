<script setup lang="ts">
import { ref, onMounted } from "vue";
import { useSettingsStore } from "../stores/settings";
import FloatingIconView from "../components/icons/FloatingIcon.vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";

const settings = useSettingsStore();
const activeTab = ref<"icon" | "sound" | "config" | "about">("icon");
const configPath = ref("");
const configContent = ref("");

// Dynamically discover all SVG icons from the resources directory.
// Adding a new .svg file to src-tauri/resources/icons/ will auto-register it.
const iconModules = import.meta.glob(
  "../../src-tauri/resources/icons/*.svg",
  { query: "?component", eager: true }
);
const icons = Object.keys(iconModules).map((path) => {
  const filename = path.split("/").pop()?.replace(".svg", "") || "";
  return { id: filename, label: filename.charAt(0).toUpperCase() + filename.slice(1) };
});

function select(id: string) {
  settings.setFloatingIcon(id);
  emit("floating-icon-changed", id).catch(() => {});
}

function close() {
  getCurrentWindow().close();
}

onMounted(async () => {
  await invoke("fix_svg_icons").catch(() => {});
  const raw = await invoke<string>("get_config_path").catch(() => "~/.cc-remote/config.toml");
  const home = await invoke<string>("get_home_dir").catch(() => "");
  configPath.value = home && raw.startsWith(home) ? "~" + raw.slice(home.length) : raw;
  configContent.value = await invoke<string>("read_config_file").catch(() => "");
  await settings.loadAvailableSounds();
  await reloadIcons();
});

function openConfigDir() {
  invoke("open_config_dir").catch((e) => console.warn("open_config_dir failed:", e));
}

function previewSound(soundName: string) {
  invoke("play_sound", { name: soundName }).catch(() => {});
}

function openCustomDir(kind: string) {
  invoke("open_custom_dir", { kind }).catch(() => {});
}

const customIcons = ref<string[]>([]);
const customIconSvgs = ref<Record<string, string>>({});

async function reloadIcons() {
  const names = await invoke<string[]>("list_available_icons").catch(() => []) || [];
  // Filter out icons already in the bundled set
  const bundledIds = new Set(icons.map((i) => i.id));
  customIcons.value = names.filter((n) => !bundledIds.has(n));
  // Load SVG content for each custom icon
  const svgs: Record<string, string> = {};
  for (const name of customIcons.value) {
    const svg = await invoke<string>("read_icon_svg", { name }).catch(() => "");
    if (svg) svgs[name] = svg;
  }
  customIconSvgs.value = svgs;
}

async function reloadSounds() {
  await settings.reloadSounds();
}
</script>

<template>
  <div class="settings-page">
    <div class="titlebar" data-tauri-drag-region>
      <span data-tauri-drag-region>Settings</span>
      <button class="close-btn" @click="close">&#x2715;</button>
    </div>
    <div class="layout">
      <nav class="sidebar">
        <div class="nav-item" :class="{ active: activeTab === 'icon' }" @click="activeTab = 'icon'">Floating Icon</div>
        <div class="nav-item" :class="{ active: activeTab === 'sound' }" @click="activeTab = 'sound'">Sound</div>
        <div class="nav-item" :class="{ active: activeTab === 'config' }" @click="activeTab = 'config'">Config</div>
        <div class="nav-item" :class="{ active: activeTab === 'about' }" @click="activeTab = 'about'">About</div>
      </nav>
      <main class="content">
        <!-- Floating Icon -->
        <div v-if="activeTab === 'icon'" class="panel">
          <h3>Choose Floating Icon</h3>
          <div class="custom-dir-header">
            <span class="custom-dir-label">自定义路径</span>
            <code class="custom-dir-path">~/.cc-remote/icons/</code>
            <button class="open-dir-btn" @click="openCustomDir('icons')">Open</button>
            <button class="open-dir-btn" @click="reloadIcons">Load</button>
          </div>
          <div class="icon-grid">
            <div
              v-for="icon in icons"
              :key="icon.id"
              class="icon-card"
              :class="{ active: settings.floatingIcon === icon.id }"
              @click="select(icon.id)"
            >
              <div class="icon-preview">
                <FloatingIconView :icon="icon.id" color="original" />
                <FloatingIconView :icon="icon.id" color="green" />
                <FloatingIconView :icon="icon.id" color="orange" />
                <FloatingIconView :icon="icon.id" color="red" />
              </div>
              <span class="icon-label">{{ icon.label }}</span>
            </div>
          </div>
          <div v-if="customIcons.length" class="icon-grid" style="margin-top: 12px;">
            <div
              v-for="name in customIcons"
              :key="'custom-'+name"
              class="icon-card"
              :class="{ active: settings.floatingIcon === name }"
              @click="select(name)"
            >
              <div class="icon-preview">
                <FloatingIconView :icon="name" color="original" :svgContent="customIconSvgs[name]" />
                <FloatingIconView :icon="name" color="green" :svgContent="customIconSvgs[name]" />
                <FloatingIconView :icon="name" color="orange" :svgContent="customIconSvgs[name]" />
                <FloatingIconView :icon="name" color="red" :svgContent="customIconSvgs[name]" />
              </div>
              <span class="icon-label">{{ name }}</span>
            </div>
          </div>
        </div>
        <!-- Config -->
        <div v-else-if="activeTab === 'sound'" class="panel">
          <h3>Notification Sounds</h3>
          <div class="custom-dir-header">
            <span class="custom-dir-label">自定义路径</span>
            <code class="custom-dir-path">~/.cc-remote/sounds/</code>
            <button class="open-dir-btn" @click="openCustomDir('sounds')">Open</button>
            <button class="open-dir-btn" @click="reloadSounds">Load</button>
          </div>
          <div class="sound-section">
            <label class="sound-label">Task Complete (Idle)</label>
            <div class="sound-grid">
              <div
                v-for="s in settings.availableSounds" :key="'idle-'+s.name"
                class="sound-chip" :class="{ active: settings.idleSound === s.name }"
                @click="settings.setIdleSound(s.name); previewSound(s.name)"
              >{{ s.name }}</div>
            </div>
          </div>
          <div class="sound-section">
            <label class="sound-label">Permission Request</label>
            <div class="sound-grid">
              <div
                v-for="s in settings.availableSounds" :key="'perm-'+s.name"
                class="sound-chip" :class="{ active: settings.permissionSound === s.name }"
                @click="settings.setPermissionSound(s.name); previewSound(s.name)"
              >{{ s.name }}</div>
            </div>
          </div>
        </div>
        <!-- Config File -->
        <div v-else-if="activeTab === 'config'" class="panel">
          <h3>Config File</h3>
          <div class="config-header">
            <code class="config-path-text">{{ configPath }}</code>
            <button class="open-dir-btn" @click="openConfigDir">Open Directory</button>
          </div>
          <textarea class="config-viewer" readonly :value="configContent || '(file not found or empty)'" />
        </div>
        <!-- About -->
        <div v-else-if="activeTab === 'about'" class="panel">
          <h3>About</h3>
          <div class="about-info">
            <div class="about-row"><span class="label">Name</span><span>CC Remote Seamless</span></div>
            <div class="about-row"><span class="label">Version</span><span>0.1.0</span></div>
            <div class="about-row"><span class="label">Description</span><span>Remote control panel for Claude Code sessions via IM platforms</span></div>
            <div class="about-row"><span class="label">Tech</span><span>Tauri 2 + Vue 3 + Rust</span></div>
            <div class="about-row"><span class="label">Author</span><span>thorn</span></div>
          </div>
        </div>
      </main>
    </div>
  </div>
</template>

<style scoped>
.settings-page { background: #ffffff; height: 100vh; display: flex; flex-direction: column; overflow: hidden; }
.titlebar { height: 36px; display: flex; align-items: center; justify-content: space-between; padding: 0 14px; background: #f8fafc; cursor: grab; user-select: none; -webkit-user-select: none; border-bottom: 1px solid #e2e8f0; flex-shrink: 0; font-size: 13px; font-weight: 600; color: #374151; }
.close-btn { border: none; background: transparent; cursor: pointer; width: 24px; height: 24px; border-radius: 6px; font-size: 13px; color: #64748b; display: flex; align-items: center; justify-content: center; }
.close-btn:hover { background: #fecaca; color: #dc2626; }
.layout { display: flex; flex: 1; overflow: hidden; }
.sidebar { width: 120px; background: #f8fafc; border-right: 1px solid #e2e8f0; padding: 12px 0; flex-shrink: 0; }
.nav-item { padding: 8px 16px; font-size: 12px; color: #64748b; cursor: pointer; transition: all 0.15s; }
.nav-item:hover { background: #f1f5f9; color: #374151; }
.nav-item.active { background: #eff6ff; color: #3b82f6; font-weight: 600; border-right: 2px solid #3b82f6; }
.content { flex: 1; padding: 20px; overflow-y: auto; }
.panel h3 { font-size: 14px; font-weight: 600; color: #1f2937; margin: 0 0 16px 0; }
.icon-grid { display: flex; flex-wrap: wrap; gap: 14px; }
.icon-card { border: 2px solid #e2e8f0; border-radius: 12px; padding: 12px; cursor: pointer; transition: all 0.2s; display: flex; flex-direction: column; align-items: center; gap: 8px; }
.icon-card:hover { border-color: #93c5fd; }
.icon-card.active { border-color: #3b82f6; background: #eff6ff; }
.icon-preview { display: flex; gap: 4px; }
.icon-label { font-size: 11px; font-weight: 500; color: #374151; }
.config-header { display: flex; align-items: center; gap: 10px; margin-bottom: 12px; }
.config-path-text { font-size: 11px; background: #f1f5f9; padding: 4px 8px; border-radius: 4px; color: #475569; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.open-dir-btn { border: 1px solid #e2e8f0; background: #ffffff; cursor: pointer; padding: 4px 10px; border-radius: 4px; font-size: 11px; color: #3b82f6; white-space: nowrap; flex-shrink: 0; }
.open-dir-btn:hover { background: #eff6ff; }
.config-viewer { width: 100%; flex: 1; min-height: 200px; height: calc(100% - 80px); border: 1px solid #e2e8f0; border-radius: 6px; padding: 10px; font-family: monospace; font-size: 12px; color: #374151; background: #f8fafc; resize: none; outline: none; }
.about-info { display: flex; flex-direction: column; gap: 12px; }
.about-row { display: flex; gap: 12px; font-size: 13px; }
.about-row .label { font-weight: 600; color: #64748b; min-width: 80px; flex-shrink: 0; }
.about-row span:last-child { color: #1f2937; }
.sound-section { margin-bottom: 18px; }
.sound-label { font-size: 12px; font-weight: 600; color: #475569; margin-bottom: 8px; display: block; }
.sound-grid { display: flex; flex-wrap: wrap; gap: 8px; }
.sound-chip { padding: 5px 12px; border: 1px solid #e2e8f0; border-radius: 16px; font-size: 11px; color: #475569; cursor: pointer; transition: all 0.15s; }
.sound-chip:hover { border-color: #93c5fd; color: #3b82f6; }
.sound-chip.active { background: #3b82f6; color: #fff; border-color: #3b82f6; }
.custom-dir-header { display: flex; align-items: center; gap: 10px; margin-bottom: 14px; padding: 6px 10px; background: #f8fafc; border: 1px solid #e2e8f0; border-radius: 6px; }
.custom-dir-label { font-size: 11px; font-weight: 600; color: #64748b; white-space: nowrap; }
.custom-dir-path { font-size: 11px; color: #475569; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.custom-icon-svg { width: 42px; height: 42px; display: flex; align-items: center; justify-content: center; }
.custom-icon-svg :deep(svg) { width: 100%; height: 100%; }
</style>

<script setup lang="ts">
import { useSessionStore } from "../stores/sessions";
import StatusBadge from "./StatusBadge.vue";

const store = useSessionStore();
const emit = defineEmits<{ select: [sessionId: string] }>();

function selectSession(id: string) {
  store.setActive(id);
  emit("select", id);
}

function dirName(path: string | null): string {
  if (!path) return "~";
  const parts = path.replace(/\/+$/, "").split("/");
  return parts[parts.length - 1] || "~";
}
</script>

<template>
  <div class="session-list">
    <div class="header">SESSIONS</div>
    <div
      v-for="session in store.sessions" :key="session.id"
      class="session-item" :class="{ active: session.id === store.activeSessionId }"
      @click="selectSession(session.id)"
    >
      <div class="session-row">
        <StatusBadge :state="session.state" />
        <span class="name">{{ dirName(session.workingDir) }} [{{ session.agent }}]</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.session-list { width: 180px; border-right: 1px solid var(--border); padding: 8px; overflow-y: auto; }
.header { font-size: 10px; color: var(--text-secondary); margin-bottom: 8px; }
.session-item { padding: 6px 8px; border-radius: 4px; cursor: pointer; margin-bottom: 4px; }
.session-item:hover, .session-item.active { background: var(--bg-hover); }
.session-row { display: flex; align-items: center; gap: 6px; }
.name { font-size: 12px; }
.meta { font-size: 10px; color: var(--text-secondary); margin-top: 2px; margin-left: 10px; }
</style>

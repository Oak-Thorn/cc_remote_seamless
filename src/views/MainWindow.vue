<script setup lang="ts">
import { onMounted, onUnmounted } from "vue";
import { listen } from "@tauri-apps/api/event";
import { useSessionStore } from "../stores/sessions";
import { useMessageStore } from "../stores/messages";
import SessionList from "../components/SessionList.vue";
import MessageFlow from "../components/MessageFlow.vue";

const sessionStore = useSessionStore();
const messageStore = useMessageStore();

onMounted(() => sessionStore.refresh());

function onSelectSession(id: string) {
  messageStore.loadForSession(id);
}

let unlistenMessages: (() => void) | null = null;
let unlistenSessions: (() => void) | null = null;

onMounted(async () => {
  unlistenMessages = await listen<{ session_id: string }>("messages-updated", (event) => {
    if (sessionStore.activeSessionId && event.payload.session_id === sessionStore.activeSessionId) {
      messageStore.loadForSession(sessionStore.activeSessionId);
    }
  });
  unlistenSessions = await listen("sessions-updated", () => {
    sessionStore.refresh();
  });
});

onUnmounted(() => {
  unlistenMessages?.();
  unlistenSessions?.();
});
</script>

<template>
  <div class="main-window">
    <SessionList @select="onSelectSession" />
    <div class="content">
      <div v-if="!sessionStore.activeSessionId" class="placeholder">Select a session</div>
      <MessageFlow v-else />
    </div>
  </div>
</template>

<style scoped>
.main-window { display: flex; height: 100vh; background: var(--bg-primary); }
.content { flex: 1; display: flex; flex-direction: column; }
.placeholder { flex: 1; display: flex; align-items: center; justify-content: center; color: var(--text-secondary); }
</style>

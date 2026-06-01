<script setup lang="ts">
import { useMessageStore } from "../stores/messages";
import { computed } from "vue";

const store = useMessageStore();

const sortedMessages = computed(() =>
  [...store.messages].sort((a, b) => a.timestamp - b.timestamp)
);

function sourceLabel(source: string) {
  const map: Record<string, string> = { cli: "CLI", feishu: "飞书", agent: "Agent" };
  return map[source] || source;
}

function formatTime(ts: number) {
  return new Date(ts * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}
</script>

<template>
  <div class="message-flow">
    <div v-if="store.loading" class="empty">Loading...</div>
    <div v-else-if="sortedMessages.length === 0" class="empty">No messages yet</div>
    <div v-else class="messages">
      <div v-for="msg in sortedMessages" :key="msg.id" class="message">
        <div class="msg-header">
          <span class="source" :class="`source-${msg.source}`">{{ sourceLabel(msg.source) }}</span>
          <span class="time">{{ formatTime(msg.timestamp) }}</span>
        </div>
        <div class="msg-body">{{ msg.text }}</div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.message-flow { flex: 1; padding: 8px; overflow-y: auto; }
.empty { color: var(--text-secondary); font-size: 12px; text-align: center; margin-top: 40px; }
.message { margin-bottom: 12px; }
.msg-header { display: flex; align-items: center; gap: 6px; margin-bottom: 3px; }
.source { font-size: 10px; padding: 1px 5px; border-radius: 2px; background: #334155; }
.source-feishu { background: #065f46; color: #6ee7b7; }
.source-agent { background: #1e3a5f; color: #93c5fd; }
.time { font-size: 10px; color: var(--text-secondary); }
.msg-body { padding: 6px 8px; background: var(--bg-secondary); border-radius: 4px; font-size: 12px; white-space: pre-wrap; }
</style>

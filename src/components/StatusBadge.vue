<script setup lang="ts">
defineProps<{
  state: "Idle" | "Busy" | "WaitingPermission";
}>();

const colors: Record<string, string> = {
  Idle: "#4ade80",
  Busy: "#facc15",
  WaitingPermission: "#ef4444",
};

const labels: Record<string, string> = {
  Idle: "idle",
  Busy: "busy",
  WaitingPermission: "waiting",
};
</script>

<template>
  <span class="badge">
    <span class="dot" :style="{ background: colors[state] }"></span>
    <span class="label">{{ labels[state] }}</span>
  </span>
</template>

<style scoped>
.badge { display: inline-flex; align-items: center; gap: 4px; }
.dot { width: 6px; height: 6px; border-radius: 50%; }
.label { font-size: 10px; color: var(--text-secondary); }
</style>

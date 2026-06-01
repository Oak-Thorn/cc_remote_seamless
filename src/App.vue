<script setup lang="ts">
import { computed } from "vue";
import FloatingWidget from "./views/FloatingWidget.vue";
import MainWindow from "./views/MainWindow.vue";
import PermissionPopup from "./views/PermissionPopup.vue";
import SettingsWindow from "./views/SettingsWindow.vue";

const view = computed(() => {
  const params = new URLSearchParams(window.location.search);
  return params.get("view") || "float";
});
</script>

<template>
  <FloatingWidget v-if="view === 'float'" />
  <MainWindow v-else-if="view === 'main'" />
  <PermissionPopup v-else-if="view === 'permission'" />
  <SettingsWindow v-else-if="view === 'settings'" />
</template>

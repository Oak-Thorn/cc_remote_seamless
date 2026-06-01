<script setup lang="ts">
import { computed } from "vue";

// Dynamically import all SVG icons from the resources directory.
// Adding a new .svg file to src-tauri/resources/icons/ will auto-register it.
const iconModules = import.meta.glob<{ default: any }>(
  "../../../src-tauri/resources/icons/*.svg",
  { query: "?component", eager: true }
);

// Build a lookup: filename without extension → component
const iconLookup: Record<string, any> = {};
for (const [path, mod] of Object.entries(iconModules)) {
  const filename = path.split("/").pop()?.replace(".svg", "") || "";
  iconLookup[filename] = mod.default;
}

export type FloatingIconColor = "green" | "orange" | "red" | "muted" | "original";

const COLOR_MAP: Record<FloatingIconColor, string> = {
  green: "#6FCF97",
  orange: "#FF9D23",
  red: "#FF3737",
  muted: "#94a3b8",
  original: "#374151",
};

const props = defineProps<{ icon: string; color: FloatingIconColor; svgContent?: string }>();

const current = computed(() => iconLookup[props.icon]);
const cssColor = computed(() => COLOR_MAP[props.color]);
</script>

<template>
  <div class="floating-icon-wrapper">
    <div class="placeholder" />
    <div v-if="current" class="icon-bg">
      <component :is="current" class="floating-icon" :style="{ color: cssColor }" />
    </div>
    <div v-else-if="svgContent" class="icon-bg">
      <div class="floating-icon" :style="{ color: cssColor }" v-html="svgContent" />
    </div>
  </div>
</template>

<style scoped>
.floating-icon-wrapper {
  position: relative;
  width: 42px;
  height: 42px;
}
.placeholder {
  position: absolute;
  inset: 0;
  border-radius: 50%;
  background: #e5e7eb;
}
.icon-bg {
  position: absolute;
  inset: 0;
  border-radius: 50%;
  background: #ffffff;
  display: flex;
  align-items: center;
  justify-content: center;
  box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.04);
  overflow: hidden;
}
.floating-icon {
  width: 70%;
  height: 70%;
  display: block;
}
.floating-icon :deep(svg) {
  width: 100%;
  height: 100%;
  display: block;
}
</style>

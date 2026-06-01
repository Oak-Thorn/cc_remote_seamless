import { defineStore } from "pinia";
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { SessionInfo } from "../types";

export const useSessionStore = defineStore("sessions", () => {
  const sessions = ref<SessionInfo[]>([]);
  const activeSessionId = ref<string | null>(null);

  async function refresh() {
    try {
      const raw = await invoke<SessionInfo[]>("get_sessions");
      console.log("[sessions] raw response from get_sessions:", JSON.stringify(raw));
      sessions.value = raw;
      console.log("[sessions] refresh got", sessions.value.length, "sessions");
    } catch (e) {
      console.error("[sessions] refresh failed:", e);
    }
  }

  function setActive(id: string) {
    activeSessionId.value = id;
  }

  return { sessions, activeSessionId, refresh, setActive };
});

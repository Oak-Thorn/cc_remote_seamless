import { defineStore } from "pinia";
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { StoredMessage } from "../types";

export const useMessageStore = defineStore("messages", () => {
  const messages = ref<StoredMessage[]>([]);
  const loading = ref(false);

  async function loadForSession(sessionId: string) {
    loading.value = true;
    messages.value = await invoke<StoredMessage[]>("get_messages", {
      sessionId,
      limit: 50,
    });
    loading.value = false;
  }

  async function loadAll() {
    loading.value = true;
    messages.value = await invoke<StoredMessage[]>("get_all_messages", {
      limit: 50,
    });
    loading.value = false;
  }

  return { messages, loading, loadForSession, loadAll };
});

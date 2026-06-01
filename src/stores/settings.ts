import { defineStore } from "pinia";
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

export type FloatingIcon = string;

/** Sound file discovered in the resources directory. */
export interface SoundInfo {
  name: string;
  extension: string;
}

export const useSettingsStore = defineStore("settings", () => {
  const floatingIcon = ref<string>(
    localStorage.getItem("floatingIcon") || "eagle"
  );
  const idleSound = ref<string>(
    localStorage.getItem("idleSound") || "Glass"
  );
  const permissionSound = ref<string>(
    localStorage.getItem("permissionSound") || "Hero"
  );
  const availableSounds = ref<SoundInfo[]>([]);
  const soundsLoaded = ref(false);

  function setFloatingIcon(icon: string) {
    floatingIcon.value = icon;
    localStorage.setItem("floatingIcon", icon);
  }

  function setIdleSound(sound: string) {
    idleSound.value = sound;
    localStorage.setItem("idleSound", sound);
    invoke("set_sound_preference", { kind: "idle", name: sound }).catch(() => {});
  }

  function setPermissionSound(sound: string) {
    permissionSound.value = sound;
    localStorage.setItem("permissionSound", sound);
    invoke("set_sound_preference", { kind: "permission", name: sound }).catch(() => {});
  }

  async function loadAvailableSounds() {
    if (soundsLoaded.value) return;
    try {
      availableSounds.value = await invoke<SoundInfo[]>("list_available_sounds");
      soundsLoaded.value = true;
    } catch {
      // Fallback: use hardcoded defaults if backend unavailable
      const defaults: SoundInfo[] = [
        "Glass", "Hero", "Blow", "Bottle", "Frog", "Funk",
        "Morse", "Ping", "Pop", "Purr", "Sosumi", "Submarine", "Tink",
      ].map((name) => ({ name, extension: "aiff" }));
      availableSounds.value = defaults;
      soundsLoaded.value = true;
    }
    // Sync stored preferences to backend on load
    invoke("set_sound_preference", { kind: "idle", name: idleSound.value }).catch(() => {});
    invoke("set_sound_preference", { kind: "permission", name: permissionSound.value }).catch(() => {});
  }

  async function reloadSounds() {
    soundsLoaded.value = false;
    await loadAvailableSounds();
  }

  return {
    floatingIcon, setFloatingIcon,
    idleSound, setIdleSound,
    permissionSound, setPermissionSound,
    availableSounds, soundsLoaded, loadAvailableSounds, reloadSounds,
  };
});
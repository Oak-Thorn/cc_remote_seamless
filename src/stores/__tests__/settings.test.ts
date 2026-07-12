import { describe, it, expect, beforeEach, vi } from "vitest";
import { setActivePinia, createPinia } from "pinia";
import { invoke } from "@tauri-apps/api/core";
import { useSettingsStore, type FloatingIcon, type SoundInfo } from "../settings";

// Mock Tauri invoke
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue([
    { name: "Glass", extension: "aiff" },
    { name: "Hero", extension: "aiff" },
    { name: "Blow", extension: "aiff" },
    { name: "Bottle", extension: "aiff" },
    { name: "Frog", extension: "aiff" },
    { name: "Funk", extension: "aiff" },
    { name: "Morse", extension: "aiff" },
    { name: "Ping", extension: "aiff" },
    { name: "Pop", extension: "aiff" },
    { name: "Purr", extension: "aiff" },
    { name: "Sosumi", extension: "aiff" },
    { name: "Submarine", extension: "aiff" },
    { name: "Tink", extension: "aiff" },
  ] as SoundInfo[]),
}));

describe("useSettingsStore", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    localStorage.clear();
  });

  describe("defaults", () => {
    it("has idleSound default 'Glass'", () => {
      const store = useSettingsStore();
      expect(store.idleSound).toBe("Glass");
    });

    it("has permissionSound default 'Hero'", () => {
      const store = useSettingsStore();
      expect(store.permissionSound).toBe("Hero");
    });

    it("has floatingIcon default 'eagle'", () => {
      const store = useSettingsStore();
      expect(store.floatingIcon).toBe("eagle");
    });

    it("starts with empty availableSounds and soundsLoaded false", () => {
      const store = useSettingsStore();
      expect(store.availableSounds).toEqual([]);
      expect(store.soundsLoaded).toBe(false);
    });

    it("has logToFile default false", () => {
      const store = useSettingsStore();
      expect(store.logToFile).toBe(false);
    });

    it("has autoApprove default false", () => {
      const store = useSettingsStore();
      expect(store.autoApprove).toBe(false);
    });
  });

  describe("setLogToFile", () => {
    it("updates the ref and persists to localStorage", () => {
      const store = useSettingsStore();
      store.setLogToFile(true);
      expect(store.logToFile).toBe(true);
      expect(localStorage.getItem("logToFile")).toBe("true");
    });

    it("invokes backend with enabled flag", () => {
      const store = useSettingsStore();
      vi.mocked(invoke).mockClear();
      store.setLogToFile(true);
      expect(invoke).toHaveBeenCalledWith("set_log_to_file", { enabled: true });
    });

    it("restores logToFile from localStorage on new store creation", () => {
      localStorage.setItem("logToFile", "true");
      setActivePinia(createPinia());
      const store = useSettingsStore();
      expect(store.logToFile).toBe(true);
    });
  });

  describe("setAutoApprove", () => {
    it("updates the ref and persists to localStorage", () => {
      const store = useSettingsStore();
      store.setAutoApprove(true);
      expect(store.autoApprove).toBe(true);
      expect(localStorage.getItem("autoApprove")).toBe("true");
    });

    it("invokes backend with enabled flag", () => {
      const store = useSettingsStore();
      vi.mocked(invoke).mockClear();
      store.setAutoApprove(true);
      expect(invoke).toHaveBeenCalledWith("set_auto_approve", { enabled: true });
    });

    it("syncAutoApprove pushes the current value to the backend", () => {
      localStorage.setItem("autoApprove", "true");
      setActivePinia(createPinia());
      const store = useSettingsStore();
      vi.mocked(invoke).mockClear();
      store.syncAutoApprove();
      expect(invoke).toHaveBeenCalledWith("set_auto_approve", { enabled: true });
    });

    it("restores autoApprove from localStorage on new store creation", () => {
      localStorage.setItem("autoApprove", "true");
      setActivePinia(createPinia());
      const store = useSettingsStore();
      expect(store.autoApprove).toBe(true);
    });
  });

  describe("setIdleSound", () => {
    it("updates the ref value", () => {
      const store = useSettingsStore();
      store.setIdleSound("Frog");
      expect(store.idleSound).toBe("Frog");
    });

    it("persists to localStorage", () => {
      const store = useSettingsStore();
      store.setIdleSound("Pop");
      expect(localStorage.getItem("idleSound")).toBe("Pop");
    });
  });

  describe("setPermissionSound", () => {
    it("updates the ref value", () => {
      const store = useSettingsStore();
      store.setPermissionSound("Tink");
      expect(store.permissionSound).toBe("Tink");
    });

    it("persists to localStorage", () => {
      const store = useSettingsStore();
      store.setPermissionSound("Blow");
      expect(localStorage.getItem("permissionSound")).toBe("Blow");
    });
  });

  describe("setFloatingIcon", () => {
    it("updates the ref value", () => {
      const store = useSettingsStore();
      store.setFloatingIcon("cat" as FloatingIcon);
      expect(store.floatingIcon).toBe("cat");
    });

    it("persists to localStorage", () => {
      const store = useSettingsStore();
      store.setFloatingIcon("sun" as FloatingIcon);
      expect(localStorage.getItem("floatingIcon")).toBe("sun");
    });
  });

  describe("localStorage persistence", () => {
    it("restores idleSound from localStorage on new store creation", () => {
      localStorage.setItem("idleSound", "Sosumi");
      setActivePinia(createPinia());
      const store = useSettingsStore();
      expect(store.idleSound).toBe("Sosumi");
    });

    it("restores permissionSound from localStorage on new store creation", () => {
      localStorage.setItem("permissionSound", "Submarine");
      setActivePinia(createPinia());
      const store = useSettingsStore();
      expect(store.permissionSound).toBe("Submarine");
    });

    it("restores floatingIcon from localStorage on new store creation", () => {
      localStorage.setItem("floatingIcon", "angry");
      setActivePinia(createPinia());
      const store = useSettingsStore();
      expect(store.floatingIcon).toBe("angry");
    });
  });

  describe("loadAvailableSounds", () => {
    it("loads sounds from backend", async () => {
      const store = useSettingsStore();
      await store.loadAvailableSounds();
      expect(store.soundsLoaded).toBe(true);
      expect(store.availableSounds.length).toBe(13);
      expect(store.availableSounds).toContainEqual({ name: "Glass", extension: "aiff" });
      expect(store.availableSounds).toContainEqual({ name: "Hero", extension: "aiff" });
    });

    it("is idempotent (only loads once)", async () => {
      const store = useSettingsStore();
      await store.loadAvailableSounds();
      const first = store.availableSounds;
      await store.loadAvailableSounds();
      // Should not double-load
      expect(store.availableSounds).toBe(first);
    });
  });
});
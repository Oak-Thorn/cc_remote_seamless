import { describe, it, expect, beforeEach, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { setActivePinia, createPinia } from "pinia";
import { invoke } from "@tauri-apps/api/core";
import SettingsWindow from "../SettingsWindow.vue";
import type { SoundInfo } from "../../stores/settings";

const mockSounds: SoundInfo[] = [
  "Glass", "Hero", "Blow", "Bottle", "Frog", "Funk",
  "Morse", "Ping", "Pop", "Purr", "Sosumi", "Submarine", "Tink",
].map((name) => ({ name, extension: "aiff" }));

// Mock Tauri APIs
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ close: vi.fn() }),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn((cmd: string) => {
    if (cmd === "list_available_sounds") return Promise.resolve(mockSounds);
    if (cmd === "get_config_path") return Promise.resolve("/fake/config.toml");
    if (cmd === "read_config_file") return Promise.resolve("");
    return Promise.resolve("");
  }),
}));

vi.mock("@tauri-apps/api/event", () => ({
  emit: vi.fn().mockResolvedValue(undefined),
}));

describe("SettingsWindow — Sound Tab", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    localStorage.clear();
  });

  function mountComponent() {
    return mount(SettingsWindow, {
      global: {
        stubs: {
          FloatingIconView: true,
        },
      },
    });
  }

  it("renders the Sound nav item", () => {
    const wrapper = mountComponent();
    const navItems = wrapper.findAll(".nav-item");
    const soundNav = navItems.find((n) => n.text().trim() === "Sound");
    expect(soundNav).toBeTruthy();
  });

  it("shows sound tab content when Sound nav is clicked", async () => {
    const wrapper = mountComponent();
    const navItems = wrapper.findAll(".nav-item");
    const soundNav = navItems.find((n) => n.text().trim() === "Sound");
    expect(soundNav).toBeTruthy();
    await soundNav!.trigger("click");
    expect(wrapper.find("h3").text()).toBe("Notification Sounds");
  });

  it("displays Task Complete (Idle) label", async () => {
    const wrapper = mountComponent();
    const navItems = wrapper.findAll(".nav-item");
    const soundNav = navItems.find((n) => n.text().trim() === "Sound");
    await soundNav!.trigger("click");
    const labels = wrapper.findAll(".sound-label");
    expect(labels[0].text()).toBe("Task Complete (Idle)");
  });

  it("displays Permission Request label", async () => {
    const wrapper = mountComponent();
    const navItems = wrapper.findAll(".nav-item");
    const soundNav = navItems.find((n) => n.text().trim() === "Sound");
    await soundNav!.trigger("click");
    const labels = wrapper.findAll(".sound-label");
    expect(labels[1].text()).toBe("Permission Request");
  });

  it("renders sound chips from availableSounds", async () => {
    const wrapper = mountComponent();
    const navItems = wrapper.findAll(".nav-item");
    const soundNav = navItems.find((n) => n.text().trim() === "Sound");
    await soundNav!.trigger("click");
    // Wait for async loadAvailableSounds
    await wrapper.vm.$nextTick();
    await new Promise((r) => setTimeout(r, 10));
    const soundGrids = wrapper.findAll(".sound-grid");
    expect(soundGrids).toHaveLength(2);
    expect(soundGrids[0].findAll(".sound-chip").length).toBe(mockSounds.length);
    expect(soundGrids[1].findAll(".sound-chip").length).toBe(mockSounds.length);
  });

  it("highlights active idle sound chip", async () => {
    const wrapper = mountComponent();
    const navItems = wrapper.findAll(".nav-item");
    const soundNav = navItems.find((n) => n.text().trim() === "Sound");
    await soundNav!.trigger("click");
    await wrapper.vm.$nextTick();
    await new Promise((r) => setTimeout(r, 10));
    const soundGrids = wrapper.findAll(".sound-grid");
    const idleChips = soundGrids[0].findAll(".sound-chip");
    const glassChip = idleChips.find((c) => c.text().trim() === "Glass");
    expect(glassChip).toBeTruthy();
    expect(glassChip!.classes()).toContain("active");
  });

  it("highlights active permission sound chip", async () => {
    const wrapper = mountComponent();
    const navItems = wrapper.findAll(".nav-item");
    const soundNav = navItems.find((n) => n.text().trim() === "Sound");
    await soundNav!.trigger("click");
    await wrapper.vm.$nextTick();
    await new Promise((r) => setTimeout(r, 10));
    const soundGrids = wrapper.findAll(".sound-grid");
    const permChips = soundGrids[1].findAll(".sound-chip");
    const heroChip = permChips.find((c) => c.text().trim() === "Hero");
    expect(heroChip).toBeTruthy();
    expect(heroChip!.classes()).toContain("active");
  });

  it("updates active chip on click", async () => {
    const wrapper = mountComponent();
    const navItems = wrapper.findAll(".nav-item");
    const soundNav = navItems.find((n) => n.text().trim() === "Sound");
    await soundNav!.trigger("click");
    await wrapper.vm.$nextTick();
    await new Promise((r) => setTimeout(r, 10));
    const soundGrids = wrapper.findAll(".sound-grid");
    const idleChips = soundGrids[0].findAll(".sound-chip");
    const frogChip = idleChips.find((c) => c.text().trim() === "Frog");
    expect(frogChip).toBeTruthy();
    await frogChip!.trigger("click");
    expect(frogChip!.classes()).toContain("active");
    const glassChip = idleChips.find((c) => c.text().trim() === "Glass");
    expect(glassChip!.classes()).not.toContain("active");
  });

  it("calls play_sound invoke on sound chip click (preview)", async () => {
    const wrapper = mountComponent();
    const navItems = wrapper.findAll(".nav-item");
    const soundNav = navItems.find((n) => n.text().trim() === "Sound");
    await soundNav!.trigger("click");
    await wrapper.vm.$nextTick();
    await new Promise((r) => setTimeout(r, 10));
    vi.mocked(invoke).mockClear();
    const soundGrids = wrapper.findAll(".sound-grid");
    const idleChips = soundGrids[0].findAll(".sound-chip");
    const popChip = idleChips.find((c) => c.text().trim() === "Pop");
    await popChip!.trigger("click");
    expect(invoke).toHaveBeenCalledWith("play_sound", { name: "Pop" });
  });
});
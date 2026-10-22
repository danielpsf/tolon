import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { api, onSettingsChanged } from "./api";
import type { SettingsDto } from "./api-types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => undefined) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined) }));

const settings: SettingsDto = {
  lead_minutes: 2,
  again_at_start: true,
  sound: "bell",
  sound_repeat: true,
  quiet: { enabled: false, start: "18:00", end: "09:00", weekends: true },
  ignore: { all_day: true, declined: true, cancelled: true, no_attendees: false, keywords: ["Focus", "Lunch", "OOO"] },
  appearance: "auto",
  interrupt_color: "theme",
  style: "swiss-moss",
  locale: null,
  start_at_login: false,
  start_at_login_status: null,
};

beforeEach(() => vi.clearAllMocks());

// docs/decisions.md, "Overnight M3 plan and the settings contract": names and arguments are the contract.
describe("settings commands", () => {
  it("reads every setting with settings_get", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(settings);
    await expect(api.settingsGet()).resolves.toEqual(settings);
    expect(invoke).toHaveBeenCalledWith("settings_get");
  });

  it("AC9.1 saves a change with settings_set and a patch of only the changed fields", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ ...settings, lead_minutes: 5 });
    await expect(api.settingsSet({ lead_minutes: 5 })).resolves.toMatchObject({ lead_minutes: 5 });
    expect(invoke).toHaveBeenCalledWith("settings_set", { patch: { lead_minutes: 5 } });
  });

  it("AC6.3 Play previews the chosen sound with sound_preview", async () => {
    await api.soundPreview("chime");
    expect(invoke).toHaveBeenCalledWith("sound_preview", { sound: "chime" });
  });

  it("L-071 opens a help link with app_open_url and the address as given", async () => {
    await api.appOpenUrl("https://example.com/a");
    expect(invoke).toHaveBeenCalledWith("app_open_url", { url: "https://example.com/a" });
  });

  it("listens to settings:changed", async () => {
    const handler = vi.fn();
    await onSettingsChanged(handler);
    expect(listen).toHaveBeenCalledWith("settings:changed", handler);
  });
});

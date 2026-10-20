import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api, onSettingsChanged } from "./api";
import type { SettingsDto } from "./api-types";
import { copy } from "./copy";
import { resetSettings, startSettingsFeed, useSettings } from "./store";
import { applyAppearance } from "./theme";

vi.mock("./api", async (original) => ({
  ...(await original<typeof import("./api")>()),
  api: { settingsGet: vi.fn(), settingsSet: vi.fn(), soundPreview: vi.fn() },
  onSettingsChanged: vi.fn(async () => () => undefined),
}));
vi.mock("./theme", () => ({ applyAppearance: vi.fn() }));

const stored: SettingsDto = {
  lead_minutes: 2,
  again_at_start: true,
  sound: "bell",
  sound_repeat: true,
  quiet: { enabled: false, start: "18:00", end: "09:00", weekends: true },
  ignore: { all_day: true, declined: true, cancelled: true, no_attendees: false, keywords: ["Focus", "Lunch", "OOO"] },
  appearance: "auto",
  locale: null,
  start_at_login: false,
};

/** A promise the test settles by hand, to look at the page while a save is on its way. */
function later<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const shown = () => useSettings.getState().settings;

beforeEach(async () => {
  vi.clearAllMocks();
  resetSettings();
  vi.mocked(api.settingsGet).mockResolvedValue(stored);
  await useSettings.getState().load();
});

afterEach(() => resetSettings());

describe("saving settings (AC9.1)", () => {
  it("AC9.1 shows a change at once, before the save answers, and keeps what the backend stored", async () => {
    const answer = later<SettingsDto>();
    vi.mocked(api.settingsSet).mockReturnValueOnce(answer.promise);
    const saving = useSettings.getState().save({ lead_minutes: 5 });
    expect(shown()?.lead_minutes).toBe(5);
    answer.resolve({ ...stored, lead_minutes: 5 });
    await saving;
    expect(shown()).toEqual({ ...stored, lead_minutes: 5 });
    expect(useSettings.getState().error).toBeNull();
  });

  it("AC9.1 rolls back and shows the error code when the save fails", async () => {
    vi.mocked(api.settingsSet).mockRejectedValueOnce({ code: "invalid:lead_minutes" });
    await useSettings.getState().save({ lead_minutes: 61 });
    expect(shown()).toEqual(stored);
    expect(useSettings.getState().error).toEqual({ code: "invalid:lead_minutes" });
    expect(copy().commandError(useSettings.getState().error!)).toBe("Choose a lead time from 1 to 60 minutes.");
  });

  it("clears the error with the next good save", async () => {
    vi.mocked(api.settingsSet).mockRejectedValueOnce({ code: "db_error" });
    await useSettings.getState().save({ sound: "chime" });
    vi.mocked(api.settingsSet).mockResolvedValueOnce({ ...stored, sound: "chime" });
    await useSettings.getState().save({ sound: "chime" });
    expect(useSettings.getState().error).toBeNull();
    expect(shown()?.sound).toBe("chime");
  });

  it("keeps the newer change on screen while an earlier save answers", async () => {
    const first = later<SettingsDto>();
    vi.mocked(api.settingsSet).mockReturnValueOnce(first.promise).mockResolvedValueOnce({ ...stored, sound: "none", lead_minutes: 5 });
    const one = useSettings.getState().save({ lead_minutes: 5 });
    const two = useSettings.getState().save({ sound: "none" });
    first.resolve({ ...stored, lead_minutes: 5 });
    await one;
    expect(shown()?.sound).toBe("none");
    await two;
    expect(shown()).toMatchObject({ lead_minutes: 5, sound: "none" });
  });

  it("rolls an earlier failed change back once the newer save answers, and keeps its error", async () => {
    const first = later<SettingsDto>();
    const second = later<SettingsDto>();
    vi.mocked(api.settingsSet).mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const one = useSettings.getState().save({ lead_minutes: 5 });
    const two = useSettings.getState().save({ sound: "none" });
    first.reject({ code: "db_error" });
    await one;
    expect(shown()).toMatchObject({ lead_minutes: 5, sound: "none" });
    second.resolve({ ...stored, sound: "none" });
    await two;
    expect(shown()).toEqual({ ...stored, sound: "none" });
    expect(useSettings.getState().error).toEqual({ code: "db_error" });
  });

  it("does nothing before the settings are loaded", async () => {
    resetSettings();
    await useSettings.getState().save({ lead_minutes: 5 });
    expect(api.settingsSet).not.toHaveBeenCalled();
  });

  it("shows a failed load as an error and no controls", async () => {
    resetSettings();
    vi.mocked(api.settingsGet).mockRejectedValueOnce({ code: "not_ready" });
    await useSettings.getState().load();
    expect(useSettings.getState()).toMatchObject({ settings: null, error: { code: "not_ready" } });
  });
});

describe("the settings feed", () => {
  it("reloads on settings:changed and applies appearance and language to the window (AC9.3)", async () => {
    resetSettings();
    await startSettingsFeed(null);
    const handler = vi.mocked(onSettingsChanged).mock.calls[0][0];
    expect(applyAppearance).toHaveBeenLastCalledWith("auto");

    vi.mocked(api.settingsGet).mockResolvedValueOnce({ ...stored, appearance: "dark", locale: "pt-PT" });
    handler();
    await vi.waitFor(() => expect(shown()?.locale).toBe("pt-PT"));
    expect(applyAppearance).toHaveBeenLastCalledWith("dark");
    expect(copy().locale).toBe("pt-PT");

    vi.mocked(api.settingsGet).mockResolvedValueOnce({ ...stored, appearance: "dark", locale: null });
    handler();
    await vi.waitFor(() => expect(shown()?.locale).toBeNull());
    expect(copy().locale).toBe("en-US");
  });
});

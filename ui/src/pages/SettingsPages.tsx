// Alarms, Quiet hours, Rules, Appearance, Language and Startup (mockups 03 and 04, F9).
// Every control saves on change through the settings store (AC9.1); there is no Save button.
import { useId, useState, type ReactNode } from "react";
import { api, asCommandError } from "../api";
import { APPEARANCES, INTERRUPT_COLORS, SOUNDS, STYLES, type SettingsDto, type SettingsPatch } from "../api-types";
import { copy, LOCALES, NATIVE_NAMES, RTL_LOCALES } from "../copy";
import { SNOOZE_MINUTES } from "../interrupt";
import { colorAttr, PREVIEW_TOKENS, tokensFor } from "../reminder-colors";
import { useSettings } from "../store";
import { addKeywords, errorText, MAX_KEYWORD_LENGTH, MAX_KEYWORDS, parseLead, splitTyped } from "../view";
import { AlarmPreview, useTestAlarmOpen } from "./AlarmPreview";

/** Saves a change; resolves true when it was stored, false when it was refused and rolled back. */
type Save = (patch: SettingsPatch) => Promise<boolean>;

/** Title, subtitle, the last error, the controls once the settings are in, and the save note. */
function SettingsPage({
  title,
  subtitle,
  children,
}: {
  title: string;
  subtitle: string;
  children: (settings: SettingsDto, save: Save) => ReactNode;
}) {
  const c = copy();
  const { settings, error, save } = useSettings();
  return (
    <>
      <header className="tl-head">
        <h1 className="sm-display-m">{title}</h1>
        <p className="sm-body sm-muted">{subtitle}</p>
      </header>
      {error && (
        <p className="sm-alert signal" role="alert">
          {c.commandError(error)}
        </p>
      )}
      {settings && (
        <div className="sm-stack tl-settings">
          {children(settings, save)}
          <p className="sm-caption">{c.t.calendars.changesSave}</p>
        </div>
      )}
    </>
  );
}

/** A switch with its label, and a one-line hint under it that describes it (aria-describedby). */
function Switch({
  label,
  hint,
  checked,
  disabled,
  onChange,
}: {
  label: string;
  hint?: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
}) {
  const hintId = useId();
  return (
    <div>
      <label className="sm-switch">
        <input
          type="checkbox"
          role="switch"
          checked={checked}
          disabled={disabled}
          aria-describedby={hint ? hintId : undefined}
          onChange={(e) => onChange(e.target.checked)}
        />
        {label}
      </label>
      {hint && (
        <span className="sm-caption tl-hint" id={hintId}>
          {hint}
        </span>
      )}
    </div>
  );
}

/** A checkbox whose label says what it ignores and whose hint explains it in one line. */
function Check({ label, hint, checked, onChange }: { label: string; hint: string; checked: boolean; onChange: (checked: boolean) => void }) {
  return (
    <label className="sm-check">
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} />
      <span>
        {label}
        <span className="sm-hint">{hint}</span>
      </span>
    </label>
  );
}

/**
 * Minutes before the start: saved as soon as the field holds 1 to 60. The stored value comes back on
 * blur, and at once when the save is refused.
 */
function LeadMinutes({ value, onSave }: { value: number; onSave: (minutes: number) => Promise<boolean> }) {
  const c = copy();
  const a = c.t.settings.alarms;
  const [draft, setDraft] = useState<string | null>(null);
  const invalid = draft !== null && parseLead(draft) === null;
  return (
    <div className={`sm-field${invalid ? " invalid" : ""}`}>
      <label className="sm-label" htmlFor="lead-minutes">
        {a.takeOver}
      </label>
      <div className="tl-inline">
        <input
          id="lead-minutes"
          className="sm-input tl-num"
          type="number"
          inputMode="numeric"
          min={1}
          max={60}
          step={1}
          aria-describedby={invalid ? "lead-suffix lead-error" : "lead-suffix"}
          aria-invalid={invalid}
          value={draft ?? String(value)}
          onChange={(e) => {
            const typed = e.target.value;
            setDraft(typed);
            const minutes = parseLead(typed);
            if (minutes === null || minutes === value) return;
            // A refused value rolls back in the store; show the stored one unless the field changed since.
            void onSave(minutes).then((saved) => saved || setDraft((now) => (now === typed ? null : now)));
          }}
          onBlur={() => setDraft(null)}
        />
        <span className="sm-body" id="lead-suffix">
          {a.leadSuffix(parseLead(draft ?? "") ?? value)}
        </span>
      </div>
      {invalid && (
        <span className="sm-error" id="lead-error" role="alert">
          {c.t.errors.invalid.lead_minutes}
        </span>
      )}
    </div>
  );
}

export function AlarmsPage() {
  const { t } = copy();
  const a = t.settings.alarms;
  const sounds = { bell: a.soundBell, chime: a.soundChime, none: a.soundNone };
  const keys: [string, string][] = [
    ["Enter", t.interrupt.joinCall],
    ["Esc", t.interrupt.snooze(SNOOZE_MINUTES)],
    ["D", t.interrupt.dismiss],
    [a.spaceKey, t.interrupt.alreadyThere],
  ];
  const preview = (sound: SettingsDto["sound"]) =>
    api.soundPreview(sound).catch((e) => useSettings.setState({ error: asCommandError(e) }));
  return (
    <SettingsPage title={t.nav.alarms} subtitle={a.subtitle}>
      {(s, save) => (
        <>
          <div className="sm-stack tight">
            <LeadMinutes value={s.lead_minutes} onSave={(lead_minutes) => save({ lead_minutes })} />
            <Switch
              label={a.againAtStart}
              checked={s.again_at_start}
              onChange={(again_at_start) => save({ again_at_start })}
            />
          </div>
          <div className="sm-field">
            <label className="sm-label" htmlFor="sound">
              {a.sound}
            </label>
            <div className="tl-inline">
              <select
                id="sound"
                className="sm-select tl-short"
                value={s.sound}
                onChange={(e) => save({ sound: e.target.value as SettingsDto["sound"] })}
              >
                {SOUNDS.map((sound) => (
                  <option key={sound} value={sound}>
                    {sounds[sound]}
                  </option>
                ))}
              </select>
              <button
                type="button"
                className="sm-btn"
                disabled={s.sound === "none"}
                onClick={() => void preview(s.sound)}
              >
                {a.play}
              </button>
              <Switch label={a.repeat} checked={s.sound_repeat} onChange={(sound_repeat) => save({ sound_repeat })} />
            </div>
          </div>
          <div>
            <h2 className="sm-label" id="keys-heading">
              {a.keyboard}
            </h2>
            <ul className="tl-keys" aria-labelledby="keys-heading">
              {keys.map(([key, action]) => (
                <li key={key}>
                  <kbd className="sm-kbd">{key}</kbd>
                  <span>{action}</span>
                </li>
              ))}
            </ul>
          </div>
        </>
      )}
    </SettingsPage>
  );
}

export function QuietHoursPage() {
  const c = copy();
  const { t } = c;
  const q = t.settings.quiet;
  return (
    <SettingsPage title={t.nav.quietHours} subtitle={q.subtitle}>
      {(s, save) => {
        const quiet = (patch: Partial<SettingsDto["quiet"]>) => save({ quiet: patch });
        const time = (field: "start" | "end", label: string) => (
          <div className="sm-field">
            <label className="sm-label" htmlFor={`quiet-${field}`}>
              {label}
            </label>
            <input
              id={`quiet-${field}`}
              className="sm-input tl-time"
              type="time"
              dir="ltr"
              required
              disabled={!s.quiet.enabled}
              value={s.quiet[field]}
              // An emptied field is not a time; keep the stored one.
              onChange={(e) => e.target.value && quiet({ [field]: e.target.value })}
            />
          </div>
        );
        return (
          <>
            <Switch label={q.enabled} checked={s.quiet.enabled} onChange={(enabled) => quiet({ enabled })} />
            <div className="tl-inline tl-times">
              {time("start", q.from)}
              {time("end", q.to)}
            </div>
            <Switch
              // Beta 1 weekends are Saturday and Sunday in every locale (docs/decisions.md).
              label={q.weekends(c.weekday(6), c.weekday(0))}
              hint={q.weekendsHint}
              checked={s.quiet.weekends}
              disabled={!s.quiet.enabled}
              onChange={(weekends) => quiet({ weekends })}
            />
          </>
        );
      }}
    </SettingsPage>
  );
}

/** The keyword list as chips: Enter, a comma or leaving the field adds what was typed; each chip has a Remove button. */
function Keywords({ value, onSave }: { value: string[]; onSave: (keywords: string[]) => void }) {
  const r = copy().t.settings.rules;
  const [typed, setTyped] = useState("");
  const commit = (text: string) => {
    const next = addKeywords(value, text);
    if (next.length !== value.length) onSave(next);
  };
  return (
    <div className="sm-field">
      <label className="sm-label" htmlFor="keywords">
        {r.keywords}
      </label>
      <div className="sm-tag-input">
        {value.map((word) => (
          <span key={word} className="sm-chip">
            <bdi>{word}</bdi>
            <button
              type="button"
              className="sm-chip-x"
              aria-label={r.removeKeyword(word)}
              onClick={() => onSave(value.filter((k) => k !== word))}
            >
              ×
            </button>
          </span>
        ))}
        <input
          id="keywords"
          type="text"
          autoComplete="off"
          maxLength={MAX_KEYWORD_LENGTH}
          disabled={value.length >= MAX_KEYWORDS}
          placeholder={r.keywordPlaceholder}
          aria-describedby="keywords-hint"
          value={typed}
          onChange={(e) => {
            const { done, rest } = splitTyped(e.target.value);
            if (done) commit(done);
            setTyped(rest);
          }}
          onKeyDown={(e) => {
            if (e.key !== "Enter" || e.nativeEvent.isComposing) return;
            e.preventDefault();
            commit(typed);
            setTyped("");
          }}
          onBlur={() => {
            commit(typed);
            setTyped("");
          }}
        />
      </div>
      <span className="sm-hint" id="keywords-hint">
        {r.keywordsHint}
      </span>
    </div>
  );
}

export function RulesPage() {
  const { t } = copy();
  const r = t.settings.rules;
  const cal = t.calendars;
  return (
    <SettingsPage title={t.nav.rules} subtitle={r.subtitle}>
      {(s, save) => {
        const ignore = (patch: Partial<SettingsDto["ignore"]>) => save({ ignore: patch });
        return (
          <>
            <div className="sm-choice-group">
              <Check
                label={cal.ignoreAllDay}
                hint={r.allDayHint}
                checked={s.ignore.all_day}
                onChange={(all_day) => ignore({ all_day })}
              />
              <Check
                label={cal.ignoreDeclined}
                hint={r.declinedHint}
                checked={s.ignore.declined}
                onChange={(declined) => ignore({ declined })}
              />
              <Check
                label={r.cancelled}
                hint={r.cancelledHint}
                checked={s.ignore.cancelled}
                onChange={(cancelled) => ignore({ cancelled })}
              />
              <Check
                label={cal.ignoreSolo}
                hint={r.noAttendeesHint}
                checked={s.ignore.no_attendees}
                onChange={(no_attendees) => ignore({ no_attendees })}
              />
            </div>
            <Keywords value={s.ignore.keywords} onSave={(keywords) => ignore({ keywords })} />
          </>
        );
      }}
    </SettingsPage>
  );
}

/** The appearance this window shows now (`data-theme` on <html>, set from the setting); light outside a page. */
function windowTheme(): "light" | "dark" {
  return typeof document !== "undefined" && document.documentElement.dataset.theme === "dark" ? "dark" : "light";
}

export function AppearancePage() {
  const c = copy();
  const { t } = c;
  const p = t.settings.appearance;
  const labels = { auto: t.settings.sameAsSystem, light: p.light, dark: p.dark };
  // The Default preview's appearance: the window's until the person flips it, and again after a Theme
  // change. Only the preview changes (L-043).
  const [shown, setShown] = useState<{ appearance: SettingsDto["appearance"]; tokens: "light" | "dark" } | null>(null);
  const [testing, setTesting] = useTestAlarmOpen();
  const [testError, setTestError] = useState<string | null>(null);

  async function testAlarm() {
    if (testing) return;
    setTesting(true);
    setTestError(null);
    try {
      await api.alarmsTest();
    } catch (e) {
      setTesting(false);
      setTestError(errorText(e, c));
    }
  }

  return (
    <SettingsPage title={t.nav.appearance} subtitle={p.subtitle}>
      {(s, save) => {
        const tokens = shown?.appearance === s.appearance ? shown.tokens : windowTheme();
        const other = tokens === "dark" ? "light" : "dark";
        const color = colorAttr(s.interrupt_color);
        return (
          <>
            <div className="sm-field">
              <span className="sm-label" id="theme-heading">
                {p.theme}
              </span>
              <div className="sm-segmented tl-segmented" role="radiogroup" aria-labelledby="theme-heading">
                {APPEARANCES.map((appearance) => (
                  <label key={appearance}>
                    <input
                      type="radio"
                      name="appearance"
                      value={appearance}
                      checked={s.appearance === appearance}
                      onChange={() => save({ appearance })}
                    />
                    <span>{labels[appearance]}</span>
                  </label>
                ))}
              </div>
            </div>
            <div className="sm-field">
              <span className="sm-label" id="style-heading">
                {p.style}
              </span>
              <div className="sm-segmented tl-segmented" role="radiogroup" aria-labelledby="style-heading">
                {STYLES.map((style) => (
                  <label key={style}>
                    <input
                      type="radio"
                      name="style"
                      value={style}
                      checked={s.style === style}
                      onChange={() => save({ style })}
                    />
                    <span>{p.styles[style]}</span>
                  </label>
                ))}
              </div>
            </div>
            <div className="sm-field">
              <span className="sm-label" id="color-heading">
                {p.color}
              </span>
              <span className="sm-caption" id="color-hint">
                {p.colorHint}
              </span>
              {/* Native radios: arrow keys move the choice and save it, Tab enters on the checked one. */}
              <div className="tl-colors" role="radiogroup" aria-labelledby="color-heading" aria-describedby="color-hint">
                {INTERRUPT_COLORS.map((choice) => (
                  <label key={choice} className="sm-swatch tl-swatch">
                    <input
                      type="radio"
                      name="interrupt_color"
                      value={choice}
                      checked={choice === color}
                      onChange={() => save({ interrupt_color: choice })}
                    />
                    <i aria-hidden="true">
                      {choice === "theme" ? (
                        <span className="tl-split">
                          <span data-color={choice} data-tokens="light">
                            Aa
                          </span>
                          <span data-color={choice} data-tokens="dark">
                            Aa
                          </span>
                        </span>
                      ) : (
                        <span data-color={choice} data-tokens={tokensFor(choice, tokens)}>
                          Aa
                        </span>
                      )}
                    </i>
                    <span>{p.colors[choice]}</span>
                  </label>
                ))}
              </div>
            </div>
            <div className="tl-color-preview">
              <style>{PREVIEW_TOKENS}</style>
              <AlarmPreview color={color} tokens={tokens} peek={s.style === "peek"} />
              <div className="sm-cluster">
                {/* Only Default changes with the appearance; the other colours look the same in both. In Peek the page follows it whatever the colour. */}
                {(color === "theme" || s.style === "peek") && (
                  <button
                    type="button"
                    className="sm-btn"
                    data-action="preview-theme"
                    onClick={() => setShown({ appearance: s.appearance, tokens: other })}
                  >
                    {other === "dark" ? p.previewDark : p.previewLight}
                  </button>
                )}
                <button
                  type="button"
                  className="sm-btn"
                  data-action="test-alarm"
                  aria-disabled={testing}
                  onClick={() => void testAlarm()}
                >
                  {p.testAlarm}
                </button>
              </div>
              {testError && (
                <p className="sm-alert signal" role="alert">
                  {testError}
                </p>
              )}
            </div>
          </>
        );
      }}
    </SettingsPage>
  );
}

export function LanguagePage() {
  const { t } = copy();
  const l = t.settings.language;
  return (
    <SettingsPage title={t.nav.language} subtitle={l.subtitle}>
      {(s, save) => (
        <div className="sm-field">
          <label className="sm-label" htmlFor="language">
            {l.label}
          </label>
          <select
            id="language"
            className="sm-select tl-short"
            value={s.locale ?? ""}
            onChange={(e) => save({ locale: e.target.value || null })}
          >
            <option value="">{t.settings.sameAsSystem}</option>
            {LOCALES.map((locale) => (
              <option key={locale} value={locale} lang={locale} dir={RTL_LOCALES.has(locale) ? "rtl" : "ltr"}>
                {NATIVE_NAMES[locale]}
              </option>
            ))}
          </select>
        </div>
      )}
    </SettingsPage>
  );
}

export function StartupPage() {
  const { t } = copy();
  const p = t.settings.startup;
  return (
    <SettingsPage title={t.nav.startup} subtitle={p.subtitle}>
      {(s, save) => (
        <div className="sm-stack tight">
          <Switch
            label={p.atLogin}
            hint={p.atLoginHint}
            checked={s.start_at_login}
            onChange={(start_at_login) => save({ start_at_login })}
          />
          {/* The switch shows what the OS reports (AC9.4); these say why it is off when it was asked to be on. */}
          {s.start_at_login_status === "requires_approval" && (
            <p className="sm-alert" role="status">
              {p.requiresApproval}
            </p>
          )}
          {s.start_at_login_status === "unavailable" && <p className="sm-caption">{p.unavailable}</p>}
        </div>
      )}
    </SettingsPage>
  );
}

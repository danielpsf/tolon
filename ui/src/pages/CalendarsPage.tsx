import { useEffect, useRef, useState, type FormEvent } from "react";
import { api } from "../api";
import type { CalendarDto } from "../api-types";
import { copy } from "../copy";
import { useCalendars } from "../store";
import { errorText, statusOf, titleOf } from "../view";

const TICK_MS = 30_000;

/** Re-renders every 30 s so "Synced 1 min ago" does not go stale. */
function useNow(): number {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), TICK_MS);
    return () => clearInterval(id);
  }, []);
  return now;
}

function Row({
  calendar,
  now,
  onToggle,
  onRemove,
}: {
  calendar: CalendarDto;
  now: number;
  onToggle: (enabled: boolean) => void;
  onRemove: () => void;
}) {
  const c = copy();
  const { calendars: t } = c.t;
  const status = statusOf(calendar, c, now);
  const dot = status.failed ? "signal" : calendar.enabled ? "" : "muted";
  return (
    <li className="sm-card tl-row">
      <span className={`sm-badge dot ${dot}`} aria-hidden="true" />
      <span className="tl-text">
        <span className="tl-name">{titleOf(calendar, c)}</span>
        {calendar.display_url && <span className="sm-caption">{calendar.display_url}</span>}
      </span>
      <span className={`tl-status${status.failed ? " failed" : ""}`}>{status.text}</span>
      <span className="tl-remove">
        <button
          type="button"
          className="sm-btn quiet sm"
          aria-label={t.removeLabel(calendar.name)}
          onClick={onRemove}
        >
          {t.remove}
        </button>
      </span>
      <label className="sm-switch">
        <input
          type="checkbox"
          role="switch"
          aria-label={t.toggleLabel(calendar.name)}
          checked={calendar.enabled}
          onChange={(e) => onToggle(e.target.checked)}
        />
      </label>
    </li>
  );
}

function ConfirmRemove({
  target,
  busy,
  onCancel,
  onConfirm,
}: {
  target: CalendarDto | null;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const { calendars: t } = copy().t;
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const d = dialog.current;
    if (!d) return;
    if (target && !d.open) d.showModal();
    if (!target && d.open) d.close();
  }, [target]);
  return (
    // The native dialog traps focus and closes on Esc; its `close` event clears the target.
    <dialog
      ref={dialog}
      className="sm-dialog"
      aria-labelledby="remove-title"
      aria-describedby="remove-consequence"
      onClose={onCancel}
    >
      {target && (
        <>
          <div className="sm-dialog-body">
            <h2 className="sm-heading" id="remove-title">
              {t.removeTitle(target.name)}
            </h2>
            <p className="sm-body" id="remove-consequence">
              {t.removeConsequence}
            </p>
          </div>
          <div className="sm-dialog-actions">
            <button type="button" className="sm-btn" autoFocus onClick={onCancel}>
              {t.cancel}
            </button>
            <button type="button" className="sm-btn danger" disabled={busy} onClick={onConfirm}>
              {t.removeConfirm}
            </button>
          </div>
        </>
      )}
    </dialog>
  );
}

export function CalendarsPage() {
  const c = copy();
  const { calendars: t } = c.t;
  const { calendars, loaded, loadError } = useCalendars();
  const now = useNow();

  const [adding, setAdding] = useState(false);
  const [url, setUrl] = useState("");
  const [pending, setPending] = useState(false);
  const [addError, setAddError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [removing, setRemoving] = useState<CalendarDto | null>(null);
  const [removeBusy, setRemoveBusy] = useState(false);
  const addButton = useRef<HTMLButtonElement>(null);
  // The dialog hands focus back to the Remove button it came from, which is gone after a removal.
  // Its `close` event fires after that hand-back, so focus moves to Add calendar there. A frame callback
  // would not do: WebView2 holds those back while its window is in the background.
  const focusAddOnClose = useRef(false);

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (pending) return;
    const value = url.trim();
    if (!value) return setAddError(c.commandError({ code: "invalid_url" }));
    setPending(true);
    setAddError(null);
    try {
      // A feed that fails to sync is still added: its row carries the status (AC1.2).
      await api.calendarsAddIcs(value);
      setUrl("");
      setAdding(false);
      addButton.current?.focus();
    } catch (err) {
      setAddError(errorText(err, c));
    } finally {
      setPending(false);
    }
  }

  async function toggle(calendar: CalendarDto, enabled: boolean) {
    setActionError(null);
    try {
      await api.calendarsSetEnabled(calendar.id, enabled);
    } catch (err) {
      setActionError(errorText(err, c));
    }
  }

  async function confirmRemove() {
    if (!removing) return;
    setRemoveBusy(true);
    setActionError(null);
    try {
      await api.calendarsRemove(removing.id);
    } catch (err) {
      setActionError(errorText(err, c));
    }
    setRemoveBusy(false);
    setRemoving(null);
    focusAddOnClose.current = true;
  }

  const open = () => setAdding(true);
  const shownError = actionError ?? (loadError ? errorText(loadError, c) : null);

  return (
    <>
      <header className="tl-head">
        <h1 className="sm-display-m">{t.title}</h1>
        <p className="sm-body sm-muted">{t.subtitle}</p>
      </header>

      {shownError && (
        <p className="sm-alert signal" role="alert">
          {shownError}
        </p>
      )}

      {loaded && calendars.length === 0 && !adding && <p className="sm-body sm-muted">{t.empty}</p>}
      <ul className="tl-rows" aria-label={t.title}>
        {calendars.map((calendar) => (
          <Row
            key={calendar.id}
            calendar={calendar}
            now={now}
            onToggle={(enabled) => void toggle(calendar, enabled)}
            onRemove={() => setRemoving(calendar)}
          />
        ))}
      </ul>

      <div className="sm-cluster">
        <button
          ref={addButton}
          type="button"
          className="sm-btn primary"
          aria-expanded={adding}
          aria-controls="add-calendar"
          onClick={open}
        >
          {t.addCalendar}
        </button>
        <button type="button" className="sm-btn" aria-expanded={adding} aria-controls="add-calendar" onClick={open}>
          {t.pasteIcalUrl}
        </button>
      </div>

      {adding && (
        <form id="add-calendar" className="sm-card tl-add sm-stack tight" aria-busy={pending} onSubmit={submit}>
          <div className={`sm-field${addError ? " invalid" : ""}`}>
            <label className="sm-label" htmlFor="calendar-url">
              {t.pasteIcalUrl}
            </label>
            <input
              id="calendar-url"
              className="sm-input"
              type="text"
              inputMode="url"
              autoComplete="off"
              autoCapitalize="off"
              spellCheck={false}
              autoFocus
              placeholder={t.urlPlaceholder}
              value={url}
              disabled={pending}
              aria-invalid={addError !== null}
              aria-describedby={addError ? "calendar-url-hint calendar-url-error" : "calendar-url-hint"}
              onChange={(e) => setUrl(e.target.value)}
            />
            <span className="sm-hint" id="calendar-url-hint">
              {t.urlHint}
            </span>
            {addError && (
              <span className="sm-error" id="calendar-url-error" role="alert">
                {addError}
              </span>
            )}
          </div>
          <div className="sm-cluster">
            <button type="submit" className={`sm-btn primary${pending ? " loading" : ""}`} aria-disabled={pending}>
              {pending ? t.adding : t.addCalendar}
            </button>
            <button
              type="button"
              className="sm-btn"
              disabled={pending}
              onClick={() => {
                setAdding(false);
                setAddError(null);
                setUrl("");
              }}
            >
              {t.cancel}
            </button>
          </div>
        </form>
      )}

      <ConfirmRemove
        target={removing}
        busy={removeBusy}
        onCancel={() => {
          setRemoving(null);
          if (focusAddOnClose.current) addButton.current?.focus();
          focusAddOnClose.current = false;
        }}
        onConfirm={() => void confirmRemove()}
      />
    </>
  );
}

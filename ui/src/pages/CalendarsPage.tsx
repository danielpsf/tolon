import { useEffect, useRef, useState, type FormEvent } from "react";
import { api, asCommandError } from "../api";
import type { CalendarDto, GoogleAccountDto } from "../api-types";
import { copy } from "../copy";
import { useCalendars } from "../store";
import { cancelsRename, errorText, MAX_NAME_LENGTH, renameValue, statusHelp, statusOf, titleParts } from "../view";

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
  const help = statusHelp(calendar, c);
  const dot = status.failed ? "signal" : calendar.enabled ? "" : "muted";
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const renameButton = useRef<HTMLButtonElement>(null);
  const field = useRef<HTMLInputElement>(null);
  // The edit replaces the Rename button, so focus goes back to it once the edit has closed.
  const backToRow = useRef(false);
  useEffect(() => {
    if (!editing && backToRow.current) {
      backToRow.current = false;
      renameButton.current?.focus();
    }
  }, [editing]);

  function startEdit() {
    setDraft(calendar.name);
    setError(null);
    setEditing(true);
  }

  function closeEdit() {
    backToRow.current = true;
    setEditing(false);
  }

  async function save(e: FormEvent) {
    e.preventDefault();
    if (busy) return;
    const name = renameValue(draft, calendar.name);
    if (name === null) return closeEdit();
    setBusy(true);
    setError(null);
    try {
      await api.calendarsRename(calendar.id, name);
      closeEdit();
    } catch (err) {
      setError(errorText(err, c));
      // The field is read only while saving, so it keeps the focus; a click on Save moved it away.
      field.current?.focus();
    }
    setBusy(false);
  }

  const [before, shownName, after] = titleParts(calendar, c);
  return (
    <li className="sm-card tl-row">
      <span className={`sm-badge dot ${dot}`} aria-hidden="true" />
      {editing ? (
        <form
          className="tl-edit sm-cluster"
          aria-busy={busy}
          onSubmit={(e) => void save(e)}
          // Esc cancels from the input and from Save and Cancel alike.
          onKeyDown={(e) => {
            if (cancelsRename(e.nativeEvent)) closeEdit();
          }}
        >
          <input
            ref={field}
            className="sm-input"
            type="text"
            autoComplete="off"
            spellCheck={false}
            autoFocus
            maxLength={MAX_NAME_LENGTH}
            aria-label={t.renameField(calendar.name)}
            placeholder={t.renamePlaceholder}
            aria-invalid={error !== null}
            value={draft}
            readOnly={busy}
            onChange={(e) => setDraft(e.target.value)}
          />
          <button type="submit" className="sm-btn primary sm" aria-disabled={busy}>
            {t.renameSave}
          </button>
          <button type="button" className="sm-btn quiet sm" disabled={busy} onClick={closeEdit}>
            {t.cancel}
          </button>
          {error && (
            <span className="sm-error" role="alert">
              {error}
            </span>
          )}
        </form>
      ) : (
        <span className="tl-text">
          <span className="tl-name" onClick={startEdit}>
            {before}
            <bdi>{shownName}</bdi>
            {after}
          </span>
          {calendar.display_url && <span className="sm-caption">{calendar.display_url}</span>}
          {calendar.account && (
            <span className="sm-caption">
              <bdi>{calendar.account}</bdi>
            </span>
          )}
        </span>
      )}
      {!editing && (
        <span className={`tl-status${status.failed ? " failed" : ""}`}>
          {status.text}
          {help && (
            <>
              {" "}
              <a href={help.href}>{help.text}</a>
            </>
          )}
        </span>
      )}
      <span className="tl-remove">
        <button
          ref={renameButton}
          type="button"
          className="sm-btn quiet sm"
          aria-label={t.renameLabel(calendar.name)}
          disabled={editing}
          onClick={startEdit}
        >
          {t.rename}
        </button>
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

/** The iCal form (`#add-calendar`), shared by the Calendars page and first run (F10). */
export function AddCalendarForm({
  onAdded,
  onCancel,
}: {
  /** The calendar as added; its `last_error` says whether the first sync failed (AC1.2). */
  onAdded: (calendar: CalendarDto) => void;
  onCancel: () => void;
}) {
  const c = copy();
  const { calendars: t } = c.t;
  const [url, setUrl] = useState("");
  const [pending, setPending] = useState(false);
  const [addError, setAddError] = useState<string | null>(null);

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (pending) return;
    const value = url.trim();
    if (!value) return setAddError(c.commandError({ code: "invalid_url" }));
    setPending(true);
    setAddError(null);
    try {
      // A feed that fails to sync is still added: its row carries the status (AC1.2).
      const added = await api.calendarsAddIcs(value);
      setPending(false);
      onAdded(added);
    } catch (err) {
      setAddError(errorText(err, c));
      setPending(false);
    }
  }

  return (
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
          dir="ltr"
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
        <button type="button" className="sm-btn" disabled={pending} onClick={onCancel}>
          {t.cancel}
        </button>
      </div>
    </form>
  );
}

/**
 * Asks Rust whether this build can sign in with Google and tells `set` only when it can. Without a
 * client ID, or when the question fails, `set` is never called, so the button stays hidden. Returns
 * the cleanup that drops a late answer.
 */
export function watchGoogleAvailable(set: (available: true) => void): () => void {
  let live = true;
  api.calendarsGoogleAvailable().then(
    (ok) => {
      if (live && ok) set(true);
    },
    () => undefined,
  );
  return () => {
    live = false;
  };
}

/** True once Rust says this build can sign in with Google (a client ID is configured). */
export function useGoogleAvailable(): boolean {
  const [available, setAvailable] = useState(false);
  useEffect(() => watchGoogleAvailable(setAvailable), []);
  return available;
}

/**
 * Whether going away now leaves a sign-in open in Rust: one still waiting for the browser, or a
 * finished one whose calendars are on offer. Rust then ends it and revokes the grant nobody
 * picked calendars from. Not while adding: `calendars_add_google` has taken the grant by then.
 */
export function opensSignIn(step: "idle" | "waiting" | "adding", hasAccount: boolean): boolean {
  return step === "waiting" || (hasAccount && step !== "adding");
}

/**
 * "Sign in with Google" (F13), shared by the Calendars page and first run: Google's page opens in the
 * browser, then the account's calendars are listed to pick from. The primary calendar starts ticked.
 */
export function GoogleSignIn({ onAdded }: { onAdded: (first: CalendarDto) => void }) {
  const c = copy();
  const { calendars: t } = c.t;
  const [step, setStep] = useState<"idle" | "waiting" | "adding">("idle");
  const [account, setAccount] = useState<GoogleAccountDto | null>(null);
  const [picked, setPicked] = useState<ReadonlySet<string>>(new Set());
  const [error, setError] = useState<string | null>(null);

  // The picker going away (the page changes, the window closes) abandons what is open.
  const abandoned = useRef(false);
  useEffect(() => {
    abandoned.current = opensSignIn(step, account !== null);
  }, [step, account]);
  useEffect(
    () => () => {
      if (abandoned.current) void api.calendarsGoogleCancel().catch(() => undefined);
    },
    [],
  );

  async function signIn() {
    if (step !== "idle") return;
    setError(null);
    setStep("waiting");
    try {
      const found = await api.calendarsGoogleSignIn(t.googleDoneTitle, t.googleDoneBody);
      setAccount(found);
      setPicked(new Set(found.calendars.filter((cal) => cal.primary && !cal.added).map((cal) => cal.id)));
    } catch (e) {
      const err = asCommandError(e);
      // Cancel was the person's own choice: nothing to say.
      if (err.code !== "google_cancelled") setError(c.commandError(err));
    }
    setStep("idle");
  }

  async function cancel() {
    setAccount(null);
    setError(null);
    await api.calendarsGoogleCancel().catch(() => undefined);
  }

  async function add(e: FormEvent) {
    e.preventDefault();
    if (!account || picked.size === 0 || step !== "idle") return;
    setStep("adding");
    setError(null);
    try {
      const added = await api.calendarsAddGoogle([...picked]);
      setAccount(null);
      setStep("idle");
      if (added[0]) onAdded(added[0]);
    } catch (err) {
      setError(errorText(err, c));
      setStep("idle");
    }
  }

  function toggle(id: string, on: boolean) {
    const next = new Set(picked);
    if (on) next.add(id);
    else next.delete(id);
    setPicked(next);
  }

  const waiting = step === "waiting";
  const adding = step === "adding";
  const nothingNew = account !== null && account.calendars.every((cal) => cal.added);
  return (
    <>
      {!account && (
        <div className="sm-cluster">
          <button
            type="button"
            className={`sm-btn${waiting ? " loading" : ""}`}
            aria-disabled={waiting}
            aria-describedby={waiting ? "google-waiting" : undefined}
            onClick={() => void signIn()}
          >
            {t.signInGoogle}
          </button>
          {waiting && (
            <button type="button" className="sm-btn quiet" onClick={() => void cancel()}>
              {t.cancel}
            </button>
          )}
        </div>
      )}
      {waiting && (
        <p className="sm-body sm-muted" id="google-waiting" role="status">
          {t.googleWaiting}
        </p>
      )}
      {error && (
        <p className="sm-alert signal" role="alert">
          {error}
        </p>
      )}
      {account && (
        <form
          id="google-calendars"
          className="sm-card tl-add sm-stack tight"
          aria-labelledby="google-calendars-title"
          aria-busy={adding}
          onSubmit={(e) => void add(e)}
        >
          <h2 className="sm-heading" id="google-calendars-title">
            {t.googlePickTitle(account.account)}
          </h2>
          <p className="sm-hint">{nothingNew ? t.googleNothingNew : t.googlePickHint}</p>
          {account.calendars.map((cal) => (
            <label className="sm-check" key={cal.id}>
              <input
                type="checkbox"
                checked={cal.added || picked.has(cal.id)}
                disabled={cal.added || adding}
                onChange={(e) => toggle(cal.id, e.target.checked)}
              />
              <span>
                <bdi>{cal.name}</bdi>
                {cal.added && <span className="sm-hint">{t.googleAlreadyAdded}</span>}
              </span>
            </label>
          ))}
          <div className="sm-cluster">
            {!nothingNew && (
              <button
                type="submit"
                className={`sm-btn primary${adding ? " loading" : ""}`}
                aria-disabled={adding || picked.size === 0}
              >
                {adding ? t.googleAdding : t.googleAdd}
              </button>
            )}
            <button type="button" className="sm-btn" disabled={adding} onClick={() => void cancel()}>
              {t.cancel}
            </button>
          </div>
        </form>
      )}
    </>
  );
}

export function CalendarsPage() {
  const c = copy();
  const { calendars: t } = c.t;
  const { calendars, loaded, loadError } = useCalendars();
  const now = useNow();
  const google = useGoogleAvailable();

  const [adding, setAdding] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [removing, setRemoving] = useState<CalendarDto | null>(null);
  const [removeBusy, setRemoveBusy] = useState(false);
  const addButton = useRef<HTMLButtonElement>(null);
  // The dialog hands focus back to the Remove button it came from, which is gone after a removal.
  // Its `close` event fires after that hand-back, so focus moves to Add calendar there. A frame callback
  // would not do: WebView2 holds those back while its window is in the background.
  const focusAddOnClose = useRef(false);

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
        <AddCalendarForm
          onAdded={() => {
            setAdding(false);
            addButton.current?.focus();
          }}
          onCancel={() => setAdding(false)}
        />
      )}

      {google && <GoogleSignIn onAdded={() => addButton.current?.focus()} />}

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

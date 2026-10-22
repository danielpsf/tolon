// About (F9 nav, F11 updates): the version, and the update prompt with its three buttons. This page
// and one line in the tray panel are the only places an update is offered; never a takeover.
// Under Updates, "Help us fix a problem" saves the diagnostics zip (L-072).
import { useEffect, useRef, useState, type Ref } from "react";
import { api } from "../api";
import type { DiagnosticsLink, DiagnosticsPlanDto, UpdateDto } from "../api-types";
import { copy } from "../copy";
import { updateView, useUpdate } from "../update";
import { errorText } from "../view";
import pkg from "../../package.json";

/** The Updates section, from the status and what the person did on this page. */
export function UpdateSection({
  update,
  checked,
  busy,
  error,
  onCheck,
  onInstall,
  onLater,
  onSkip,
  headingRef,
}: {
  update: UpdateDto | null;
  checked: boolean;
  busy: boolean;
  error: string | null;
  onCheck: () => void;
  onInstall: () => void;
  onLater: () => void;
  onSkip: () => void;
  /** Focus lands here after Later, Skip or a failed install, whose buttons are gone or disabled. */
  headingRef?: Ref<HTMLHeadingElement>;
}) {
  const a = copy().t.settings.about;
  const view = updateView(update, checked);
  return (
    <section className="sm-stack tight" aria-labelledby="updates-heading">
      <h2 className="sm-label" id="updates-heading" tabIndex={-1} ref={headingRef}>
        {a.updates}
      </h2>
      {view === "offer" && update?.available && (
        <>
          <p className="sm-body" role="status">
            {a.available(update.available)}
          </p>
          <div className="tl-inline">
            <button
              type="button"
              className="sm-btn primary"
              aria-describedby="update-restart"
              disabled={busy}
              onClick={onInstall}
            >
              {a.install}
            </button>
            <button type="button" className="sm-btn" disabled={busy} onClick={onLater}>
              {a.later}
            </button>
            <button type="button" className="sm-btn quiet" disabled={busy} onClick={onSkip}>
              {a.skip}
            </button>
          </div>
          <p className="sm-caption tl-hint" id="update-restart">
            {a.restartNote}
          </p>
        </>
      )}
      {view === "installing" && (
        <p className="sm-body" role="status">
          {a.installing}
        </p>
      )}
      {view === "current" && (
        <p className="sm-body" role="status">
          {a.upToDate}
        </p>
      )}
      {error && (
        <p className="sm-alert signal" role="alert">
          {error}
        </p>
      )}
      {(view === "idle" || view === "current") && (
        <div>
          <button type="button" className="sm-btn" disabled={busy} onClick={onCheck}>
            {a.check}
          </button>
        </div>
      )}
      <p className="sm-caption">{a.autoCheck}</p>
    </section>
  );
}

/** `sendTo` around its `{support}` placeholder: the text before and after the link. */
export function supportParts(template: string): [string, string] {
  const at = template.indexOf("{support}");
  return at < 0 ? [template, ""] : [template.slice(0, at), template.slice(at + "{support}".length)];
}

/** "Help us fix a problem" (L-072): save the diagnostics zip, show where it went, say where to send it. */
export function HelpSection({
  plan,
  saved,
  busy,
  error,
  onSave,
  onReveal,
  onOpen,
}: {
  plan: DiagnosticsPlanDto | null;
  saved: string | null;
  busy: boolean;
  error: string | null;
  onSave: () => void;
  onReveal: () => void;
  onOpen: (link: DiagnosticsLink) => void;
}) {
  const h = copy().t.settings.about.help;
  const [before, after] = supportParts(h.sendTo);
  /** A real link for reading and copying; a click asks Rust to open it, never this window. */
  const link = (href: string, which: DiagnosticsLink, text: string) => (
    <a
      href={href}
      onClick={(e) => {
        e.preventDefault();
        onOpen(which);
      }}
    >
      {text}
    </a>
  );
  return (
    <section className="sm-stack tight" aria-labelledby="help-heading">
      <h2 className="sm-label" id="help-heading">
        {h.heading}
      </h2>
      <p className="sm-body">{h.intro}</p>
      <div>
        <button type="button" className="sm-btn" disabled={busy || !plan} onClick={onSave}>
          {h.save}
        </button>
      </div>
      {saved && (
        <div className="tl-inline">
          <p className="sm-body" role="status">
            {h.savedTo(saved)}
          </p>
          {plan && (
            <button type="button" className="sm-btn quiet" disabled={busy} onClick={onReveal}>
              {h.reveal[plan.file_manager]}
            </button>
          )}
        </div>
      )}
      {error && (
        <p className="sm-alert signal" role="alert">
          {error}
        </p>
      )}
      {plan && (
        <>
          <p className="sm-caption">
            {before}
            {link(plan.support_url, "support", plan.support_email)}
            {after}
          </p>
          <p className="sm-caption">{link(plan.issues_url, "issues", h.issues)}</p>
          <p className="sm-caption">{link(plan.doc_url, "doc", h.whatsInside)}</p>
        </>
      )}
    </section>
  );
}

/** The Help section's state: the plan from Rust, the save (its dialog opens in Rust) and the reveal. */
function useDiagnostics() {
  const c = copy();
  const [plan, setPlan] = useState<DiagnosticsPlanDto | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api.diagnosticsPlan().then(setPlan, (e) => setError(errorText(e, c)));
  }, [c]);
  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (e) {
      setError(errorText(e, c));
    } finally {
      setBusy(false);
    }
  };
  return {
    plan,
    saved,
    busy,
    error,
    onSave: () =>
      void run(async () => {
        const path = await api.diagnosticsSave({
          locale: c.locale,
          languages: [...navigator.languages],
          user_agent: navigator.userAgent,
        });
        // null: the person cancelled the save dialog.
        if (path) setSaved(path);
      }),
    onReveal: () => void run(() => api.diagnosticsReveal()),
    onOpen: (link: DiagnosticsLink) => void run(() => api.diagnosticsOpen(link)),
  };
}

export function AboutPage() {
  const c = copy();
  const a = c.t.settings.about;
  const [update, setUpdate] = useUpdate();
  const [checked, setChecked] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  const diagnostics = useDiagnostics();

  /** Runs a button's command; `refocus` puts focus on the Updates heading afterwards. */
  const run = async (action: () => Promise<UpdateDto | void>, refocus = false) => {
    setBusy(true);
    setError(null);
    try {
      const answer = await action();
      if (answer) setUpdate(answer);
    } catch (e) {
      setError(errorText(e, c));
    } finally {
      setBusy(false);
      if (refocus) heading.current?.focus();
    }
  };

  return (
    <>
      <header className="tl-head">
        <h1 className="sm-display-m">{c.t.nav.about}</h1>
        <p className="sm-body sm-muted">{a.subtitle}</p>
      </header>
      <div className="sm-stack tl-settings">
        <p className="sm-body">{a.version(pkg.version)}</p>
        <UpdateSection
          update={update}
          checked={checked}
          busy={busy}
          error={error}
          onCheck={() =>
            void run(async () => {
              const answer = await api.updateCheckNow();
              setChecked(true);
              return answer;
            })
          }
          onInstall={() => void run(() => api.updateInstall(), true)}
          onLater={() => void run(() => api.updateLater(), true)}
          onSkip={() => void run(() => api.updateSkip(), true)}
          headingRef={heading}
        />
        <HelpSection {...diagnostics} />
      </div>
    </>
  );
}

// About (F9 nav, F11 updates): the version, and the update prompt with its three buttons. This page
// and one line in the tray panel are the only places an update is offered; never a takeover.
import { useState } from "react";
import { api } from "../api";
import type { UpdateDto } from "../api-types";
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
}: {
  update: UpdateDto | null;
  checked: boolean;
  busy: boolean;
  error: string | null;
  onCheck: () => void;
  onInstall: () => void;
  onLater: () => void;
  onSkip: () => void;
}) {
  const a = copy().t.settings.about;
  const view = updateView(update, checked);
  return (
    <section className="sm-stack tight" aria-labelledby="updates-heading">
      <h2 className="sm-label" id="updates-heading">
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

export function AboutPage() {
  const c = copy();
  const a = c.t.settings.about;
  const [update, setUpdate] = useUpdate();
  const [checked, setChecked] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const run = async (action: () => Promise<UpdateDto | void>) => {
    setBusy(true);
    setError(null);
    try {
      const answer = await action();
      if (answer) setUpdate(answer);
    } catch (e) {
      setError(errorText(e, c));
    } finally {
      setBusy(false);
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
          onInstall={() => void run(() => api.updateInstall())}
          onLater={() => void run(() => api.updateLater())}
          onSkip={() => void run(() => api.updateSkip())}
        />
      </div>
    </>
  );
}

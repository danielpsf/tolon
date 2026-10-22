// Help (L-071): the install guide and the help pages of this release, converted from docs/ when the UI was built
// (help/convert.ts). The text is English for now; the page around it speaks the person's language. Links between
// docs and anchors are plain `#/help/<section>[/<anchor>]` links; only a web address needs the system browser.
import { docsHash, sections } from "virtual:help";
import { useEffect, useRef, useState, type MouseEvent } from "react";
import { api } from "../api";
import { copy } from "../copy";
import { errorText } from "../view";

/** `#/help/ical-links/outlook` to `["ical-links", "outlook"]`; the second part is empty without an anchor. */
export function helpPathOf(hash: string): [string, string] {
  const [, , section = "", anchor = ""] = hash.replace(/^#/, "").split("/");
  return [section, anchor];
}

function useHelpPath(): [string, string] {
  const [path, setPath] = useState(() => helpPathOf(window.location.hash));
  useEffect(() => {
    const onChange = () => setPath(helpPathOf(window.location.hash));
    window.addEventListener("hashchange", onChange);
    return () => window.removeEventListener("hashchange", onChange);
  }, []);
  return path;
}

const LABELS = { install: "install", "ical-links": "icalLinks", diagnostics: "diagnostics" } as const;

export function HelpPage() {
  const c = copy();
  const h = c.t.help;
  const [wanted, anchor] = useHelpPath();
  const current = sections.find((s) => s.id === wanted) ?? sections[0];
  const article = useRef<HTMLElement>(null);
  const arrived = useRef(false);
  const [error, setError] = useState<string | null>(null);

  // An anchor scrolls to its heading; a new section starts at the top. Focus follows, so a keyboard
  // or screen reader person is not left on a link that is gone.
  useEffect(() => {
    const target = anchor ? document.getElementById(`help-${anchor}`) : null;
    if (target) {
      target.scrollIntoView();
      target.focus({ preventScroll: true });
    } else if (arrived.current) {
      window.scrollTo(0, 0);
      article.current?.focus({ preventScroll: true });
    }
    arrived.current = true;
  }, [current?.id, anchor]);

  // The one thing that must never happen: a link navigating the webview away from Tolon.
  const onClick = (e: MouseEvent<HTMLElement>) => {
    const link = (e.target as Element).closest("a");
    if (!link) return;
    const href = link.getAttribute("href") ?? "";
    if (link.hasAttribute("data-external")) {
      e.preventDefault();
      setError(null);
      api.appOpenUrl(href).catch((err: unknown) => setError(errorText(err, c)));
    } else if (!href.startsWith("#/help/")) {
      e.preventDefault();
    }
  };

  if (!current) return null;
  return (
    <>
      <header className="tl-head">
        <h1 className="sm-display-m">{c.t.nav.help}</h1>
        <p className="sm-body sm-muted">{h.subtitle}</p>
      </header>
      <div className="sm-stack tl-settings">
        <nav aria-label={h.sectionsLabel}>
          <div className="sm-tabs">
            {sections.map((s) => {
              const label = s.id in LABELS ? h[LABELS[s.id as keyof typeof LABELS]] : s.title;
              return (
                <a
                  key={s.id}
                  href={`#/help/${s.id}`}
                  className={s.id === current.id ? "sm-tab active" : "sm-tab"}
                  aria-current={s.id === current.id ? "page" : undefined}
                  lang={s.id in LABELS ? undefined : "en"}
                >
                  {label}
                </a>
              );
            })}
          </div>
        </nav>
        {error && (
          <p className="sm-alert signal" role="alert">
            {error}
          </p>
        )}
        {/* English text inside any page direction. The HTML comes from our own docs at build time, escaped and
            checked there; nothing here is typed by a person or fetched. */}
        <article
          ref={article}
          className="sm-prose tl-help"
          lang="en"
          dir="ltr"
          tabIndex={-1}
          data-docs-hash={docsHash}
          onClick={onClick}
          onAuxClick={onClick}
          dangerouslySetInnerHTML={{ __html: current.html }}
        />
      </div>
    </>
  );
}

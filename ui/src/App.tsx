import { useEffect, useState } from "react";
import { copy } from "./copy";
import { CalendarsPage } from "./pages/CalendarsPage";
import { startCalendarsFeed } from "./store";
import pkg from "../package.json";

const PAGES = [
  { id: "calendars", key: "calendars" },
  { id: "alarms", key: "alarms" },
  { id: "quiet-hours", key: "quietHours" },
  { id: "appearance", key: "appearance" },
  { id: "startup", key: "startup" },
  { id: "about", key: "about" },
] as const;

type PageId = (typeof PAGES)[number]["id"];

/** `#/alarms` to "alarms"; anything unknown is Calendars. */
function routeOf(hash: string): PageId {
  const id = hash.replace(/^#\/?/, "");
  return PAGES.find((p) => p.id === id)?.id ?? "calendars";
}

function useRoute(): PageId {
  const [route, setRoute] = useState(() => routeOf(window.location.hash));
  useEffect(() => {
    const onChange = () => setRoute(routeOf(window.location.hash));
    window.addEventListener("hashchange", onChange);
    return () => window.removeEventListener("hashchange", onChange);
  }, []);
  return route;
}

function Bell() {
  return (
    <svg viewBox="0 0 28 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
      <path d="M6 15a8 8 0 0 1 16 0v1H6z" />
      <path d="M3 11a11 11 0 0 1 2-5M25 11a11 11 0 0 0-2-5" />
      <path d="M14 21v.5" />
    </svg>
  );
}

export function App() {
  const { t } = copy();
  const route = useRoute();
  useEffect(() => {
    document.title = t.app.windowTitle;
  }, [t]);
  useEffect(startCalendarsFeed, []);
  const page = PAGES.find((p) => p.id === route) ?? PAGES[0];

  return (
    <div className="tl-shell">
      <aside className="tl-side">
        <div className="tl-brand">
          <Bell />
          <span>{t.app.windowTitle}</span>
        </div>
        <nav className="sm-nav" aria-label={t.nav.label}>
          {PAGES.map((p) => (
            <a key={p.id} href={`#/${p.id}`} aria-current={p.id === route ? "page" : undefined}>
              {t.nav[p.key]}
            </a>
          ))}
        </nav>
        <span className="sm-caption tl-version">v{pkg.version}</span>
      </aside>
      <main className="tl-main" aria-label={t.nav[page.key]}>
        {route === "calendars" ? (
          <CalendarsPage />
        ) : (
          <>
            <h1 className="sm-display-m">{t.nav[page.key]}</h1>
            <p className="sm-body sm-muted">{t.nav.comingSoon}</p>
          </>
        )}
      </main>
    </div>
  );
}

import { useEffect, useState } from "react";
import { Bell } from "./Bell";
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

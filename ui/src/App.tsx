import { useEffect, useState } from "react";
import { PeekMark } from "./PeekMark";
import { copy } from "./copy";
import { startFirstRunWatch, useFirstRun } from "./first-run";
import { AboutPage } from "./pages/AboutPage";
import { CalendarsPage } from "./pages/CalendarsPage";
import { FirstRunPage } from "./pages/FirstRunPage";
import { HelpPage } from "./pages/HelpPage";
import {
  AlarmsPage,
  AppearancePage,
  LanguagePage,
  QuietHoursPage,
  RulesPage,
  StartupPage,
} from "./pages/SettingsPages";
import { startTrayTooltip } from "./panel";
import { startCalendarsFeed, useSettings } from "./store";
import pkg from "../package.json";

// One topic per page (AC9.2).
export const PAGES = [
  { id: "calendars", key: "calendars", Page: CalendarsPage },
  { id: "alarms", key: "alarms", Page: AlarmsPage },
  { id: "quiet-hours", key: "quietHours", Page: QuietHoursPage },
  { id: "rules", key: "rules", Page: RulesPage },
  { id: "appearance", key: "appearance", Page: AppearancePage },
  { id: "language", key: "language", Page: LanguagePage },
  { id: "startup", key: "startup", Page: StartupPage },
  { id: "help", key: "help", Page: HelpPage },
  { id: "about", key: "about", Page: AboutPage },
] as const;

type PageId = (typeof PAGES)[number]["id"];

/** `#/alarms` to "alarms", `#/help/ical-links` to "help" (the rest of the path is the page's own); anything unknown is Calendars. */
function routeOf(hash: string): PageId {
  const id = hash.replace(/^#\/?/, "").split("/")[0];
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
  // A new language re-renders every page from the new catalogue (copy() reads the active one).
  useSettings((s) => s.settings?.locale);
  const { t } = copy();
  const route = useRoute();
  useEffect(() => {
    document.title = t.app.windowTitle;
  }, [t]);
  useEffect(startCalendarsFeed, []);
  // The settings window stays loaded while hidden (closing it only hides it, S7.2) and writes the tray tooltip.
  useEffect(() => startTrayTooltip(), []);
  useEffect(startFirstRunWatch, []);
  const firstRun = useFirstRun((s) => s.shows);
  const page = PAGES.find((p) => p.id === route) ?? PAGES[0];

  // Paper until the calendar list says whether this is a first run (a few milliseconds).
  if (firstRun === null) return null;
  if (firstRun) return <FirstRunPage />;
  return (
    <div className="tl-shell">
      <aside className="tl-side">
        <div className="tl-brand">
          <PeekMark />
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
        <page.Page />
      </main>
    </div>
  );
}

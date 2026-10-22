// Entry of `interrupt.html`: the full-screen window the app opens per screen (docs/architecture.md, "Scheduler").
import "@fontsource/inter/latin-400.css";
import "@fontsource/inter/latin-500.css";
import "@fontsource/inter/latin-600.css";
import "@fontsource/inter-tight/latin-500.css";
import "@fontsource/inter-tight/latin-600.css";
import "./fraunces.css";
import "swiss-moss/css/swiss-moss.css";
import "./styles/local.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { InterruptPage } from "./pages/InterruptPage";
import { startSettingsFeed, useSettings } from "./store";

// The interrupt follows the appearance and language settings too (AC9.3). It waits at most 500 ms for
// them: a late answer still applies, but a slow store must never hold the alarm back.
const settingsLoaded = Promise.race([
  startSettingsFeed(new URLSearchParams(window.location.search).get("lang")),
  new Promise((resolve) => setTimeout(resolve, 500)),
]);

/** Renders the page again when the language changes, so every string comes from the new catalogue. */
function Root() {
  useSettings((s) => s.settings?.locale);
  return <InterruptPage />;
}

const root = document.getElementById("root");
if (root) {
  void settingsLoaded.finally(() =>
    createRoot(root).render(
      <StrictMode>
        <Root />
      </StrictMode>,
    ),
  );
}

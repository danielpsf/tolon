// Entry of `panel.html`: the tray panel window (F7, mockup 02).
import "@fontsource/inter/latin-400.css";
import "@fontsource/inter/latin-500.css";
import "@fontsource/inter/latin-600.css";
import "@fontsource/inter-tight/latin-500.css";
import "@fontsource/inter-tight/latin-600.css";
import "swiss-moss/css/swiss-moss.css";
import "./styles/local.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { PanelPage } from "./pages/PanelPage";
import { startSettingsFeed, useSettings } from "./store";

const settingsLoaded = startSettingsFeed(new URLSearchParams(window.location.search).get("lang"));

/** Renders the panel again when the language changes, so every string comes from the new catalogue. */
function Root() {
  useSettings((s) => s.settings?.locale);
  return <PanelPage />;
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

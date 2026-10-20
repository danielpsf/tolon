import "@fontsource/inter/latin-400.css";
import "@fontsource/inter/latin-500.css";
import "@fontsource/inter/latin-600.css";
import "@fontsource/inter-tight/latin-500.css";
import "@fontsource/inter-tight/latin-600.css";
import "swiss-moss/css/swiss-moss.css";
import "./styles/local.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { startSettingsFeed } from "./store";

// `?lang=pt-PT` stands in for the system language (dev, tests) while the language setting follows the system.
const settingsLoaded = startSettingsFeed(new URLSearchParams(window.location.search).get("lang"));

const root = document.getElementById("root");
if (root) {
  void settingsLoaded.finally(() =>
    createRoot(root).render(
      <StrictMode>
        <App />
      </StrictMode>,
    ),
  );
}

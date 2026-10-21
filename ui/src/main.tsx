import "@fontsource/inter/latin-400.css";
import "@fontsource/inter/latin-500.css";
import "@fontsource/inter/latin-600.css";
import "@fontsource/inter-tight/latin-500.css";
import "@fontsource/inter-tight/latin-600.css";
// Peek's display face (L-050), Latin and Latin Extended: other scripts keep the :lang() stacks in local.css.
import "@fontsource/fraunces/latin-400.css";
import "@fontsource/fraunces/latin-600.css";
import "@fontsource/fraunces/latin-700.css";
import "@fontsource/fraunces/latin-ext-400.css";
import "@fontsource/fraunces/latin-ext-600.css";
import "@fontsource/fraunces/latin-ext-700.css";
import "swiss-moss/css/swiss-moss.css";
import "./styles/local.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { listenForHide } from "./first-run";
import { startSettingsFeed } from "./store";

// Before the first load: Rust may show the window, and the person close it, before the page renders.
listenForHide();
// `?lang=pt-PT` stands in for the system language (dev, tests) while the language setting follows the system.
// The settings window is the one that hands Rust the notification's wording (L-038); the other windows may not call it.
const settingsLoaded = startSettingsFeed(new URLSearchParams(window.location.search).get("lang"), true);

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

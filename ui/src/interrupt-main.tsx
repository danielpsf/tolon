// Entry of `interrupt.html`: the full-screen window the app opens per screen (docs/architecture.md, "Scheduler").
import "@fontsource/inter/latin-400.css";
import "@fontsource/inter/latin-500.css";
import "@fontsource/inter/latin-600.css";
import "@fontsource/inter-tight/latin-500.css";
import "@fontsource/inter-tight/latin-600.css";
import "swiss-moss/css/swiss-moss.css";
import "./styles/local.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { initCopy } from "./copy";
import { InterruptPage } from "./pages/InterruptPage";
import { followSystemTheme } from "./theme";

initCopy(new URLSearchParams(window.location.search).get("lang"));
followSystemTheme();

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <InterruptPage />
    </StrictMode>,
  );
}

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
import { initCopy } from "./copy";
import { followSystemTheme } from "./theme";

// `?lang=pt-PT` overrides the system language (dev, tests, and later the language setting).
initCopy(new URLSearchParams(window.location.search).get("lang"));
followSystemTheme();

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}

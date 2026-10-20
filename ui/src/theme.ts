import type { Appearance } from "./api-types";

let stopFollowing: (() => void) | undefined;

/** `data-theme` on <html> from the appearance setting (AC9.3): fixed for light and dark, the system's for auto. */
export function applyAppearance(appearance: Appearance): void {
  stopFollowing?.();
  stopFollowing = undefined;
  const root = document.documentElement;
  if (appearance === "light" || appearance === "dark") {
    root.dataset.theme = appearance;
    return;
  }
  const query = window.matchMedia("(prefers-color-scheme: dark)");
  const apply = () => {
    root.dataset.theme = query.matches ? "dark" : "light";
  };
  apply();
  query.addEventListener("change", apply);
  stopFollowing = () => query.removeEventListener("change", apply);
}

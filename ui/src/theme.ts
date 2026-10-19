// `data-theme` on <html> follows the system until the appearance setting exists (docs/architecture.md, "UI").
export function followSystemTheme(): () => void {
  const query = window.matchMedia("(prefers-color-scheme: dark)");
  const apply = () => {
    document.documentElement.dataset.theme = query.matches ? "dark" : "light";
  };
  apply();
  query.addEventListener("change", apply);
  return () => query.removeEventListener("change", apply);
}

import { useEffect } from "react";
import { copy } from "./copy";

// Empty on purpose: the Swiss Moss body rule paints the paper background.
export function App() {
  const { t } = copy();
  useEffect(() => {
    document.title = t.app.windowTitle;
  }, [t]);
  return <main aria-label={t.app.windowTitle} />;
}

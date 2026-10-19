import { copy } from "./copy";

document.title = copy.windowTitle;

// Empty on purpose: the Swiss Moss body rule paints the paper background.
export function App() {
  return <main aria-label={copy.windowTitle} />;
}

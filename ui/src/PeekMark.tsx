import { useId } from "react";
import { PREVIEW_TOKENS } from "./reminder-colors";

/**
 * Tolon's mark (L-052, decision "Peek is Tolon's mark everywhere"): the moss rounded square with the hill,
 * two eyes and two cheeks, after the app and tray icon. Inline SVG, decorative, no motion. It draws with the
 * light Swiss Moss values in both appearances (`data-tokens`, like the Peek hill), so the eyes stay paper and
 * the pupils ink; the outline sits outside that group and takes the window's own tokens (local.css, `.tl-mark`).
 */
export function PeekMark() {
  const clip = `tl-mark-${useId().replace(/:/g, "")}`;
  return (
    <svg className="tl-mark" viewBox="0 0 128 128" aria-hidden="true" focusable="false">
      <style>{PREVIEW_TOKENS}</style>
      <clipPath id={clip}>
        <rect width="128" height="128" rx="28" />
      </clipPath>
      <g data-tokens="light">
        <rect className="tl-mark-body" width="128" height="128" rx="28" />
        <path className="tl-mark-hill" clipPath={`url(#${clip})`} d="M8 140C8 98 30 76 64 76S120 98 120 140Z" />
        <ellipse className="tl-mark-eye" cx="45" cy="57" rx="19" ry="21" />
        <ellipse className="tl-mark-eye" cx="84" cy="59" rx="18" ry="19" />
        <circle className="tl-mark-pupil" cx="49" cy="55" r="11" />
        <circle className="tl-mark-pupil" cx="81" cy="57" r="10" />
        <circle className="tl-mark-eye" cx="53" cy="50" r="3" />
        <circle className="tl-mark-eye" cx="84" cy="52" r="2.5" />
        <circle className="tl-mark-cheek" cx="30" cy="90" r="10" />
        <circle className="tl-mark-cheek" cx="99" cy="90" r="10" />
      </g>
      <rect className="tl-mark-edge" width="128" height="128" rx="28" />
    </svg>
  );
}

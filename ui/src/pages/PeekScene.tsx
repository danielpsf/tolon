import type { InterruptColor } from "../api-types";

/**
 * Peek's reminder colour (L-050): Default is Peek's own deep moss; every choice draws with Swiss Moss's
 * light values (`data-tokens="light"`), so the hill and Join keep their colour on a dark page.
 */
export const peekColor = (color: InterruptColor): InterruptColor => (color === "theme" ? "moss-deep" : color);

/**
 * The moss hill with the Peek character, after the poll's mockup (decision "The poll picks Peek"). Inline
 * SVG drawn with Swiss Moss tokens only (local.css, `.tl-peek`); decorative, so hidden from screen readers.
 * The coordinates are design pixels at 1280 wide; the art scales with the window's width. No motion.
 */
export function PeekScene({ color, say }: { color: InterruptColor; say: string }) {
  return (
    <div className="tl-peek" aria-hidden="true" data-color={peekColor(color)} data-tokens="light">
      <svg className="tl-peek-art" viewBox="0 0 1280 600" focusable="false">
        <ellipse className="tl-peek-mound" cx="205" cy="476" rx="215" ry="260" />
        <ellipse className="tl-peek-mound" cx="1075" cy="476" rx="215" ry="260" />
        <ellipse className="tl-peek-shade" cx="240" cy="500" rx="185" ry="204" />
        <ellipse className="tl-peek-shade" cx="1040" cy="500" rx="185" ry="204" />
        {/* The page bulges into the hill and points at Peek. */}
        <path className="tl-peek-page" d="M0 -2H1280V0Q640 106 0 0ZM676 44L640 78L604 44Z" />
        <g className="tl-peek-glow">
          <circle cx="310" cy="92" r="10" />
          <circle cx="210" cy="138" r="10" />
          <circle cx="1055" cy="152" r="10" />
        </g>
        <g className="tl-peek-fly">
          <circle cx="310" cy="92" r="3.5" />
          <circle cx="210" cy="138" r="3.5" />
          <circle cx="1055" cy="152" r="3.5" />
        </g>
        <path className="tl-peek-dome" d="M372 640C372 380 440 142 640 142S908 380 908 640Z" />
        <ellipse className="tl-peek-belly" cx="640" cy="420" rx="180" ry="105" />
        <ellipse className="tl-peek-eye" cx="547" cy="165" rx="83" ry="90" />
        <ellipse className="tl-peek-eye" cx="729" cy="175" rx="79" ry="80" />
        <circle className="tl-peek-pupil" cx="570" cy="161" r="47" />
        <circle className="tl-peek-pupil" cx="705" cy="168" r="41" />
        <circle className="tl-peek-shine" cx="590" cy="144" r="12" />
        <circle className="tl-peek-shine" cx="690" cy="154" r="9" />
        <circle className="tl-peek-cheek" cx="477" cy="270" r="36" />
        <circle className="tl-peek-cheek" cx="798" cy="270" r="36" />
        <path className="tl-peek-smile" d="M614 274Q640 292 666 274" />
      </svg>
      <p className="tl-peek-say">{say}</p>
    </div>
  );
}

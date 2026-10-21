// View logic for the interrupt page (F4): what to show and which command a key or a click runs.
// The schedule, the join link and the alarm state stay in Rust (docs/agents/ui.md).
import { api } from "./api";
import type { JoinDto, MeetingDto } from "./api-types";
import type { Copy } from "./copy";

/** The Snooze button and the Esc key (AC4.4). The Alarms setting will own the number later. */
export const SNOOZE_MINUTES = 5;

/** What Enter does for a meeting (AC5.2). */
export type PrimaryAction = "join" | "open_event" | "already_there";
export type Action = PrimaryAction | "snooze" | "dismiss";

/** A link joins, an event page opens, a meeting with neither is only marked as attended. */
export function primaryAction(join: JoinDto): PrimaryAction {
  if (join.kind === "link") return "join";
  return join.kind === "event_page" ? "open_event" : "already_there";
}

/**
 * The host to show next to the join button: only for a link that is not a known provider, so the
 * person sees where an unfamiliar link goes before opening it (decisions.md, "Join links are
 * treated as untrusted"). It stays punycode (`xn--`), the form the core validated.
 */
export function joinHost(join: JoinDto): string | null {
  return join.kind === "link" && !join.recognised ? join.host : null;
}

// Brand names, not translated. Same providers as `detect_join` in tolon-core.
const PROVIDERS: readonly (readonly [domain: string, name: string])[] = [
  ["meet.google.com", "Google Meet"],
  ["zoom.us", "Zoom"],
  ["zoomgov.com", "Zoom"],
  ["teams.microsoft.com", "Microsoft Teams"],
  ["teams.live.com", "Microsoft Teams"],
  ["webex.com", "Webex"],
  ["meet.jit.si", "Jitsi Meet"],
];

/** "Google Meet" for a known provider's host or one of its subdomains, else null. */
export function providerOf(host: string): string | null {
  const h = host.toLowerCase();
  return PROVIDERS.find(([domain]) => h === domain || h.endsWith(`.${domain}`))?.[1] ?? null;
}

export function primaryLabel(c: Copy, meeting: MeetingDto): string {
  const { interrupt: t } = c.t;
  switch (primaryAction(meeting.join)) {
    case "join":
      return joinHost(meeting.join) ? t.joinOn(joinHost(meeting.join) ?? "") : t.joinCall;
    case "open_event":
      return t.openEvent;
    case "already_there":
      return t.alreadyThere;
  }
}

/** The primary label split around the host, so the page can shorten a long host (D3): [before, host, after]. */
export function primaryParts(c: Copy, meeting: MeetingDto): [string, string, string] {
  const label = primaryLabel(c, meeting);
  const host = joinHost(meeting.join);
  const at = host ? label.lastIndexOf(host) : -1;
  return host && at >= 0 ? [label.slice(0, at), host, label.slice(at + host.length)] : [label, "", ""];
}

/** The display-xl title is 112 design pixels: fine to about 36 characters, then it steps down so a long title still fits. */
export function titleSize(title: string): "xl" | "long" | "longer" {
  const length = [...title].length;
  return length > 90 ? "longer" : length > 36 ? "long" : "xl";
}

/**
 * "14:00 to 14:45 on Google Meet with 8 people." An unrecognised host is named in full here, since the
 * button may cut its start (decision "Reminder colour (L-043)").
 */
export function detailsText(c: Copy, meeting: MeetingDto): string {
  const provider = meeting.join.kind === "link" ? (providerOf(meeting.join.host) ?? joinHost(meeting.join)) : null;
  const range = c.t.interrupt.timeRange(c.formatTime(Date.parse(meeting.start)), c.formatTime(Date.parse(meeting.end)));
  return c.t.interrupt.details(range, provider, meeting.people);
}

/**
 * The key hint line follows what Enter does for the first meeting. It names Space only while Space
 * would mean "I am already there" (`spaceActive`), not while a Snooze or Dismiss button has focus.
 */
export function hintText(c: Copy, primary: PrimaryAction, spaceActive = true): string {
  const { interrupt: t } = c.t;
  const keys = primary === "join" ? t.hintJoin : primary === "open_event" ? t.hintEvent : t.hintAlreadyThere;
  // zh-CN and ja end sentences with the full-width "。", which takes no space after it.
  return spaceActive ? `${keys}${keys.endsWith("。") ? "" : " "}${t.hintSpace}` : keys;
}

/** "Starts in 2 min" before the start, "Started 3 min ago" after it (AC4.7, S3.4). */
export function countdownText(c: Copy, start: number, now: number): string {
  return now < start ? c.startsIn(start, now) : c.startedAgo(start, now);
}

/** Calls `onTick` now and every second after (AC4.7). Returns the stop function. */
export function startClock(onTick: (now: number) => void, intervalMs = 1000): () => void {
  onTick(Date.now());
  const id = setInterval(() => onTick(Date.now()), intervalMs);
  return () => clearInterval(id);
}

/** What a key press asks for; `enter` is resolved against the focused control. */
export type KeyAction = "enter" | "snooze" | "dismiss" | "already_there";

type KeyLike = Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "altKey" | "repeat" | "isComposing"> &
  Partial<Pick<KeyboardEvent, "code">>;

/**
 * Enter, Esc, D and Space (AC4.4). D is the character or the physical D key (`KeyD`), so it works on
 * Arabic, Hebrew, Hindi and other layouts (docs/decisions.md, "Clock, digits and keys across locales").
 * Modified keys, auto-repeat and IME input are ignored.
 */
export function keyAction(e: KeyLike): KeyAction | null {
  if (e.repeat || e.isComposing || e.ctrlKey || e.metaKey || e.altKey) return null;
  switch (e.key) {
    case "Enter":
      return "enter";
    case "Escape":
      return "snooze";
    case " ":
    case "Spacebar":
      return "already_there";
    case "d":
    case "D":
      return "dismiss";
    default:
      // Not when KeyD typed another Latin letter: on Dvorak it is "e", and the D character above already dismisses.
      return e.code === "KeyD" && !/^[a-z]$/i.test(e.key) ? "dismiss" : null;
  }
}

/**
 * Whether the page takes `wanted` itself. Space is "I am already there" (AC4.4) only from the
 * primary button or with nothing focused; on Snooze or Dismiss it activates that button, as
 * it would anywhere, so it can never mark a meeting joined by accident.
 */
export function handlesKey(wanted: KeyAction, focused: Action | null): boolean {
  return !(wanted === "already_there" && (focused === "snooze" || focused === "dismiss"));
}

/** Tab order inside a group of `count` controls, wrapping at both ends: the focus trap. */
export function nextFocusIndex(current: number, count: number, backwards: boolean): number {
  if (count <= 0) return -1;
  if (current < 0) return backwards ? count - 1 : 0;
  return (current + (backwards ? -1 : 1) + count) % count;
}

/** Runs one action as its command. Snooze and dismiss cover the whole interrupt; the rest, one meeting. */
export function perform(action: Action, key: string): Promise<void> {
  switch (action) {
    case "snooze":
      return api.alarmsSnooze(key, SNOOZE_MINUTES);
    case "dismiss":
      return api.alarmsDismiss(key);
    case "already_there":
      return api.alarmsAlreadyThere(key);
    case "join":
    case "open_event":
      return api.alarmsJoin(key);
  }
}

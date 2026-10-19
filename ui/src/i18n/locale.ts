import { enGB } from "./en-GB";
import { enUS, type Messages } from "./en-US";
import { ptBR } from "./pt-BR";
import { ptPT } from "./pt-PT";

export const FALLBACK_LOCALE = "en-US";

export const catalogues = { "en-US": enUS, "en-GB": enGB, "pt-BR": ptBR, "pt-PT": ptPT } as const satisfies Record<
  string,
  Messages
>;

export type Locale = keyof typeof catalogues;

export const LOCALES = Object.keys(catalogues) as Locale[];

const BRITISH_REGIONS = new Set(["GB", "IE", "AU", "NZ"]);

/** One BCP 47 tag to a shipped locale, or null when the language is not shipped. */
function matchTag(tag: string): Locale | null {
  let language: string;
  let region: string | undefined;
  try {
    ({ language, region } = new Intl.Locale(tag.replace(/_/g, "-")));
  } catch {
    return null;
  }
  if (language === "en") return region && BRITISH_REGIONS.has(region) ? "en-GB" : "en-US";
  // The user is Brazilian: bare "pt" and every other pt-* region use the pt-BR catalogue.
  if (language === "pt") return region === "PT" ? "pt-PT" : "pt-BR";
  return null;
}

/**
 * Picks the catalogue. The override wins when it names a shipped language, then the system
 * list in preference order; the first tag that matches a language decides. Otherwise en-US.
 */
export function resolveLocale(languages: readonly string[], override?: string | null): Locale {
  for (const tag of [override, ...languages]) {
    const match = tag ? matchTag(tag) : null;
    if (match) return match;
  }
  return FALLBACK_LOCALE;
}

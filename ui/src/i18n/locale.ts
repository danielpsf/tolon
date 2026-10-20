import { ar } from "./ar";
import { de } from "./de";
import { enGB } from "./en-GB";
import { enUS, type Messages } from "./en-US";
import { es } from "./es";
import { he } from "./he";
import { hi } from "./hi";
import { ja } from "./ja";
import { ptBR } from "./pt-BR";
import { ptPT } from "./pt-PT";
import { tr } from "./tr";
import { zhCN } from "./zh-CN";

export const FALLBACK_LOCALE = "en-US";

export const catalogues = {
  "en-US": enUS,
  "en-GB": enGB,
  "pt-BR": ptBR,
  "pt-PT": ptPT,
  de,
  es,
  ar,
  tr,
  hi,
  "zh-CN": zhCN,
  ja,
  he,
} as const satisfies Record<string, Messages>;

export type Locale = keyof typeof catalogues;

export const LOCALES = Object.keys(catalogues) as Locale[];

/** Each language in its own words, for the language setting. Never translated. */
export const NATIVE_NAMES: Record<Locale, string> = {
  "en-US": "English (US)",
  "en-GB": "English (UK)",
  "pt-BR": "Português (Brasil)",
  "pt-PT": "Português (Portugal)",
  de: "Deutsch",
  es: "Español",
  ar: "العربية",
  tr: "Türkçe",
  hi: "हिन्दी",
  "zh-CN": "简体中文",
  ja: "日本語",
  he: "עברית",
};

// docs/decisions.md: pt-AO falls to pt-PT, en-IE to en-GB. Portuguese outside Brazil follows pt-PT.
const PT_PT_REGIONS = new Set(["PT", "AO", "MZ", "CV", "GW", "ST", "TL", "MO"]);
const EN_US_REGIONS = new Set(["US", "CA", "PH"]);
// docs/decisions.md, "Locale list for beta 1": one catalogue per language, any region. Every zh-* is Simplified for now.
const BY_LANGUAGE = new Map<string, Locale>([
  ["de", "de"],
  ["es", "es"],
  ["ar", "ar"],
  ["tr", "tr"],
  ["hi", "hi"],
  ["ja", "ja"],
  ["zh", "zh-CN"],
  ["he", "he"],
  // Legacy code for Hebrew. Intl.Locale canonicalises it to "he"; kept for engines that do not.
  ["iw", "he"],
]);

/** Locales written right to left: `<html dir="rtl">`. */
export const RTL_LOCALES: ReadonlySet<Locale> = new Set<Locale>(["ar", "he"]);

/** One BCP 47 tag to a shipped locale, or null when the language is not shipped. */
function matchTag(tag: string): Locale | null {
  let language: string;
  let region: string | undefined;
  try {
    ({ language, region } = new Intl.Locale(tag.replace(/_/g, "-")));
  } catch {
    return null;
  }
  if (language === "en") return !region || EN_US_REGIONS.has(region) ? "en-US" : "en-GB";
  // Bare "pt" and BR stay pt-BR (the user is Brazilian).
  if (language === "pt") return region && PT_PT_REGIONS.has(region) ? "pt-PT" : "pt-BR";
  return BY_LANGUAGE.get(language) ?? null;
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

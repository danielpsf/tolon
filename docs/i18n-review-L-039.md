# Translation review for L-039

The catalogues `de`, `es`, `ar`, `tr`, `hi`, `zh-CN`, `ja` and `he` were drafted by the implementing agent (Claude Opus 5.5) and checked by a second model. This file logs every judge call, its token use and the decision on each issue. A native speaker still has to review each catalogue before a release.

## Method

- Judge: `gemini-3.8-flash` through the maintainer's LiteLLM proxy (`/chat/completions`), temperature 0, `max_tokens` 8000 (de round 1) and 16000 (all other calls). The model reasons before it answers, so most of the completion tokens are reasoning.
- One locale per call. Each prompt held the en-US source (`ui/src/i18n/en-US.ts`), the translated catalogue and the copy rules: no exclamation marks or emoji, sentence case where the script has case, "Tolon" never translated, buttons say what happens, the same meaning as the source. It also held the language rules: de "du"; es neutral "tú"; ar Arabic punctuation (، ؟) and six plural forms; tr "sen"; hi "आप" and the danda; zh-CN and ja full-width punctuation; he gender-neutral instructions (action nouns, "אפשר" with an infinitive) and the plural forms one, two and other.
- The judge answered in strict JSON: `{"issues":[{"key","problem","suggestion"}],"verdict":"ok|fix"}`.
- At most two rounds per locale. Round 2 ran on the first seven locales after the round 1 fixes, because one consistency fix (`nav.about`) touched all seven catalogues. Hebrew was added later (decision "Hebrew joins the locale list"); its round 1 found no issue, so it needed no round 2.
- Only UI copy and the TypeScript around it was sent. No keys, tokens or calendar data.

## Calls and tokens

| Locale | Round | Prompt | Completion (reasoning) | Total | Seconds | Issues | Verdict |
|---|---|---|---|---|---|---|---|
| de | 1 | 3444 | 7688 (7679) | 11132 | 56.9 | 0 | ok |
| de | 2 | 3503 | 6725 (6716) | 10228 | 33.2 | 0 | ok |
| es | 1 | 3424 | 12491 (12444) | 15915 | 69.4 | 1 | fix |
| es | 2 | 3483 | 2879 (2870) | 6362 | 14.3 | 0 | ok |
| ar | 1 | 3997 | 7922 (7873) | 11919 | 64.1 | 1 | fix |
| ar | 2 | 4056 | 6968 (6959) | 11024 | 50.5 | 0 | ok |
| tr | 1 | 3426 | 13633 (13552) | 17059 | 95.4 | 1 | fix |
| tr | 2 | 3488 | 2908 (2899) | 6396 | 29.3 | 0 | ok |
| hi | 1 | 3508 | 7522 (7468) | 11030 | 50.5 | 1 | fix |
| hi | 2 | 3566 | 8349 (8340) | 11915 | 44.3 | 0 | ok |
| zh-CN | 1 | 3285 | 8736 (8689) | 12021 | 48.5 | 1 | fix |
| zh-CN | 2 | 3344 | 6269 (6260) | 9613 | 46.6 | 0 | ok |
| ja | 1 | 3364 | 7607 (7598) | 10971 | 41.9 | 0 | ok |
| ja | 2 | 3423 | 8890 (8881) | 12313 | 72.5 | 0 | ok |
| he | 1 | 4030 | 11687 (11678) | 15717 | 81.4 | 0 | ok |
| All (15 calls) | | 53341 | 120274 | 173615 | | 5 | |

## Issues and decisions

Round 1 raised 5 issues, all in the first seven locales, and all 5 were accepted. Round 2 raised none. Hebrew round 1 raised none.

| Locale | Key | Judge's problem | Suggestion | Decision |
|---|---|---|---|---|
| es | `nav.about` | "Acerca de Tolon" adds the product name, which the source "About" does not have. | "Acerca de" | Accepted: the nav label must match the source; the page title is the same key. |
| ar | `nav.about` | "حول Tolon" adds the product name. | "حول" | Accepted: same reason as es. |
| zh-CN | `nav.about` | "关于 Tolon" adds the product name. | "关于" | Accepted: same reason as es. |
| tr | `interrupt.dismissConsequence` | "susar" is intransitive and reads oddly with a reminder as its subject; the transitive "susturur" matches the source. | "Kapatmak bu hatırlatıcıyı bu bilgisayarda susturur." | Accepted: it also mirrors the source structure ("Dismissing silences ..."). |
| hi | `nav.appearance` | "दिखावट" means show or pretence; the usual UI word for Appearance is "रूप-रंग". | "रूप-रंग" | Accepted: "दिखावट" can carry the sense of showing off. |

Applied by the implementer for consistency, without a judge issue: `nav.about` in de ("Über Tolon" to "Info"), hi ("Tolon के बारे में" to "परिचय") and ja ("Tolon について" to "情報") now leave out the product name as well. Round 2 checked them.

## Choices recorded with this review

The orchestrator recorded the clock, digit and key rules in `docs/decisions.md`, "Clock, digits and keys across locales". As implemented:

- Digits follow `Intl` defaults; `numberingSystem` is not pinned. On current engines (CLDR 46 and later; Node 22.23 and Node 24 ship ICU 78.3 with CLDR 48) `ar` formats numbers with Latin digits; older ICU builds use Arabic-Indic digits. HTTP status codes are codes and always print in ASCII digits ("فشلت المزامنة: 404"). Tests normalise Arabic-Indic digits, so they pass on both.
- 12-hour locales (en-US, ar, hi) show the hour without a leading zero ("2:05 م", "2:05 pm"). 24-hour locales, he included, keep two digits ("09:05"). `formatTime` asks `Intl` whether the locale uses a 12-hour clock.
- Relative-time tests pin each sentence but take the duration ("1 Min.", "1分钟", "1 דק׳") from `Intl` at runtime, since unit labels change between CLDR versions.
- Arabic and Hebrew wrap names, hosts and keywords from a feed in Unicode first-strong isolates (U+2068 ... U+2069), so "Team (EU)" keeps its order inside a right-to-left sentence.
- Key names stay in Latin script in every language (Esc, Enter, D); only Space is translated. D also matches the physical key (`KeyD`), so it works on Arabic, Hebrew, Hindi and other layouts.
- Key hint sentences are joined with a space, except after the full-width "。" in zh-CN and ja.
- Turkish uses informal "sen", to match "du" and "tú".

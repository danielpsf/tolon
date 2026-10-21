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

## L-017

The settings copy of L-017 (46 new strings: two nav labels, the Alarms, Quiet hours, Rules, Appearance, Language and Startup pages, and four `invalid:<field>` errors) was drafted by the implementing agent (Claude Opus 5.5) in all 12 catalogues and checked by the same judge. The prompts for de, tr, hi, zh-CN and ja also held the L-040 copy nits (`nav.about` in ja and hi, `relative.startsNow` and `interrupt.snooze` in ja, `ignoreMatching` in de, `timeRange` in tr, `snooze` in zh-CN). en-US is the source and was not judged.

### Method

- Judge: `gemini-3.8-flash` through the maintainer's LiteLLM proxy (`/chat/completions`), temperature 0, `max_tokens` 16000.
- One locale per call. Each prompt held only the new strings as key, en-US source and translation, a short description of each page, the copy rules and the language rules of the L-039 method (de "du", es "tú", pt-BR "você", pt-PT "ecrã", ar punctuation, tr "sen", hi "आप" and the danda, zh-CN and ja full-width punctuation, he gender-neutral instructions, en-GB spelling).
- Strict JSON answers. Round 2 ran on every locale with an accepted round 1 fix; en-GB had none.

### Calls and tokens

| Locale | Round | Prompt | Completion (reasoning) | Total | Seconds | Issues | Verdict |
|---|---|---|---|---|---|---|---|
| en-GB | 1 | 2161 | 3179 (3092) | 5340 | 17.8 | 1 | fix |
| de | 1 | 2181 | 3650 (3425) | 5831 | 20.4 | 3 | fix |
| de | 2 | 2186 | 4106 (3936) | 6292 | 36.6 | 2 | fix |
| es | 1 | 2122 | 5752 (5399) | 7874 | 32.9 | 4 | fix |
| es | 2 | 2129 | 5272 (5030) | 7401 | 41.7 | 3 | fix |
| pt-BR | 1 | 2239 | 5569 (5309) | 7808 | 52.8 | 3 | fix |
| pt-BR | 2 | 2239 | 3508 (3487) | 5747 | 46.3 | 0 | ok |
| pt-PT | 1 | 2258 | 4946 (4754) | 7204 | 27.1 | 2 | fix |
| pt-PT | 2 | 2264 | 2654 (2633) | 4918 | 14.7 | 0 | ok |
| ar | 1 | 2188 | 3391 (3184) | 5579 | 18.7 | 2 | fix |
| ar | 2 | 2198 | 4818 (4683) | 7016 | 39.6 | 1 | fix |
| tr | 1 | 2242 | 5260 (5060) | 7502 | 43.4 | 2 | fix |
| tr | 2 | 2243 | 4104 (4011) | 6347 | 25.7 | 1 | fix |
| hi | 1 | 2178 | 3038 (2804) | 5216 | 18.2 | 4 | fix |
| hi | 2 | 2178 | 3236 (3143) | 5414 | 24.9 | 1 | fix |
| zh-CN | 1 | 2210 | 5429 (5324) | 7639 | 48.5 | 1 | fix |
| zh-CN | 2 | 2211 | 5223 (5131) | 7434 | 37.6 | 1 | fix |
| ja | 1 | 2188 | 8474 (8153) | 10662 | 61.1 | 4 | fix |
| ja | 2 | 2190 | 11259 (11103) | 13449 | 65.5 | 2 | fix |
| he | 1 | 2299 | 8325 (8072) | 10624 | 40.3 | 3 | fix |
| he | 2 | 2300 | 3977 (3968) | 6277 | 21.8 | 0 | ok |
| All (21 calls) | | 46404 | 105170 | 151574 | | 40 | |

### Issues and decisions

40 issues: 31 accepted, 9 rejected. The two-round cap is reached; the round 2 fixes were applied without a third call.

Rejected:

| Locale | Round | Key | Judge's point | Why rejected |
|---|---|---|---|---|
| en-GB, es, pt-BR, ja | 1 | `settings.rules.removeKeyword` | The `(word: string)` annotation was dropped. | Not a copy issue: every catalogue is typed `Messages`, so the parameter type is inferred; `tsc` is green. |
| es, zh-CN | 2 | `settings.rules.removeKeyword` | Same. | Same. |
| hi | 1, 2 | `nav.about` | "के बारे में" is a dangling postposition; suggests "जानकारी". | The orchestrator chose "के बारे में" in L-040. Left for the native review. |
| es | 2 | `settings.alarms.soundChime` | "Carillón" is misspelled. | The RAE spelling is "carillón". |

Accepted, round 1:

| Locale | Key | Before | After |
|---|---|---|---|
| ar | `quiet.weekends` | عطلة نهاية الأسبوع هادئة كلها | هدوء طوال عطلة نهاية الأسبوع |
| ar | `invalid.keywords` | 50 كلمة على الأكثر، وكل كلمة حتى 64 حرفًا. | استخدم 50 كلمة كحد أقصى، بما لا يتجاوز 64 حرفًا لكل كلمة. |
| es | `alarms.repeat` | Repetir cada 30 s hasta descartar | Repetir cada 30 s hasta que se descarte |
| es | `rules.keywordsHint` | No importan las mayúsculas. | No se distingue entre mayúsculas y minúsculas. |
| es | `invalid.time` | Escribe una hora con horas y minutos. | Introduce una hora en formato de horas y minutos. |
| he | `quiet.subtitle` | ... את מה שמתקרב. | ... את האירועים הקרובים. |
| he | `startup.subtitle` | תתפספס (colloquial) | תוחמץ |
| he | `invalid.lead_minutes` | של 1 עד 60 דקות | בין 1 ל-60 דקות |
| hi | `alarms.leadSuffix` | मिनट पहले, शुरुआत से | मिनट (शुरुआत से पहले) |
| hi | `appearance.light`, `appearance.dark` | हल्की, गहरी | लाइट, डार्क |
| pt-BR | `alarms.repeat` | até dispensar | até ser dispensado |
| pt-BR | `rules.noAttendeesHint` | que você cria para si | que você mesmo cria |
| pt-PT | `alarms.repeat` | Repetir a cada 30 s até dispensar | Repetir de 30 em 30 s até ser dispensado |
| pt-PT | `invalid.time` | Introduza uma hora em horas e minutos. | Introduza a hora no formato de horas e minutos. |
| tr | `alarms.leadSuffix` | dakika önce, başlangıçtan | dakika (başlangıçtan önce) |
| tr | `appearance.subtitle` | bu ayarı izler | bu ayara uyar |
| zh-CN | `quiet.weekendsHint` | 周末全天保持安静。 | 周末全天保持勿扰。 |
| de | `alarms.subtitle` | welcher Ton spielt | welcher Ton abgespielt wird |
| de | `rules.declinedHint` | denen du abgesagt hast | die du abgelehnt hast |
| de | `startup.nextBuild` | Wirkt ab dem nächsten Build. | Wird ab dem nächsten Build wirksam. |
| ja | `alarms.takeOver` | 画面の表示 | 全画面表示 |
| ja | `rules.keywordPlaceholder`, `rules.keywordsHint` | 語を追加 | キーワードを追加 (the label and the error use キーワード too) |

Accepted, round 2:

| Locale | Key | Before | After |
|---|---|---|---|
| ar | `alarms.leadSuffix` | دقائق قبل البدء | دقيقة قبل البدء (right after numbers 11 to 60) |
| de | `alarms.repeat` | bis verworfen | bis er verworfen wird |
| de | `quiet.weekends` | Wochenenden ganz ruhig | Ganzes Wochenende ruhig |
| es | `rules.noAttendeesHint` | que te pones tú | que programas tú mismo |
| ja | `alarms.leadLabel` | 開始前の分数 | 開始何分前 |
| ja | `quiet.enabled` | サイレント時間を使う | サイレント時間帯を使う |
| tr | `alarms.takeOver` | Ekranı kaplama | Ekranı kapla |

The L-040 nits raised no issue except hi `nav.about` (rejected above). A native review of every catalogue is still required before release.

### L-017 review fixes

After the L-017 review, the implementer changed or added these strings in all 12 catalogues, and the same judge checked them:

- `alarms.play` now says what happens ("Play the sound").
- `startup.notAvailable` replaces `startup.nextBuild` ("Start at login is not available yet.").
- `alarms.leadSuffix` is now a function of the minutes, so it agrees with the number in the field. The judge saw the forms for 1, 2, 3, 11 and 60.
- The de keyword sentence puts the list after a colon.
- `quiet.weekends` names Saturday and Sunday with `Intl.DateTimeFormat` long weekday names, per the decision "All-day events, weekends and window checks".

en-US is the source and was not judged. The prompt sent rendered strings only, and told the judge that the number is already shown as digits before `leadSuffix`. Round 2 covered every locale, because `quiet.weekends` was added between the two rounds.

| Locale | Round | Prompt | Completion (reasoning) | Total | Seconds | Issues | Verdict |
|---|---|---|---|---|---|---|---|
| en-GB | 1 | 603 | 359 (338) | 962 | 2.9 | 0 | ok |
| en-GB | 2 | 705 | 749 (728) | 1454 | 5.0 | 0 | ok |
| de | 1 | 595 | 719 (698) | 1314 | 21.5 | 0 | ok |
| de | 2 | 697 | 891 (870) | 1588 | 6.4 | 0 | ok |
| es | 1 | 601 | 1256 (1235) | 1857 | 16.0 | 0 | ok |
| es | 2 | 704 | 877 (856) | 1581 | 6.5 | 0 | ok |
| pt-BR | 1 | 618 | 595 (586) | 1213 | 13.2 | 0 | ok |
| pt-BR | 2 | 725 | 1058 (1037) | 1783 | 7.3 | 0 | ok |
| pt-PT | 1 | 631 | 1076 (1055) | 1707 | 7.4 | 0 | ok |
| pt-PT | 2 | 738 | 1094 (1073) | 1832 | 9.3 | 0 | ok |
| ar | 1 | 612 | 8999 (8882) | 9611 | 76.0 | 1 | fix |
| ar | 2 | 717 | 8074 (7979) | 8791 | 51.7 | 1 | fix |
| tr | 1 | 618 | 1427 (1406) | 2045 | 13.1 | 0 | ok |
| tr | 2 | 724 | 3513 (3492) | 4237 | 24.3 | 0 | ok |
| hi | 1 | 624 | 589 (580) | 1213 | 6.2 | 0 | ok |
| hi | 2 | 725 | 1468 (1383) | 2193 | 13.7 | 1 | fix |
| zh-CN | 1 | 619 | 1299 (1278) | 1918 | 11.7 | 0 | ok |
| zh-CN | 2 | 724 | 2168 (2147) | 2892 | 14.6 | 0 | ok |
| ja | 1 | 592 | 8814 (8311) | 9406 | 76.7 | 5 | fix |
| ja | 2 | 691 | 1748 (1727) | 2439 | 11.7 | 0 | ok |
| he | 1 | 658 | 873 (864) | 1531 | 8.2 | 0 | ok |
| he | 2 | 765 | 1280 (1271) | 2045 | 9.1 | 0 | ok |
| All (22 calls) | | 14686 | 48926 | 63612 | | 8 | |

Decisions:

| Locale | Round | Key | Judge's point | Decision |
|---|---|---|---|---|
| ja | 1 | `alarms.leadSuffix` (5 issues, one per count) | "分前に表示" adds "show" and drops "the start". | Accepted: "分前". |
| ar | 1 | `alarms.leadSuffix(2)` | The dual "دقيقتان" after the digit 2 is redundant; use the singular. | Applied, then reverted after round 2. |
| ar | 2 | `alarms.leadSuffix(2)` | After the digit 2, Arabic needs the dual "دقيقتان". | Accepted. The two rounds contradict each other. The dual is what the orchestrator asked for, so the final forms are دقيقة (1, 11 to 60), دقيقتان (2) and دقائق (3 to 10). The native review should confirm. |
| hi | 2 | `quiet.weekends` | No finite verb. | Accepted: "... शांत रहते हैं". Applied after round 2, with no third call (two-round cap). |

The Hebrew dual is the plain plural "דקות" after the digit, not "שתי דקות" as the review suggested, because "שתי" is the word "two" and the number is already shown in the field. The judge raised no issue on Hebrew.

## L-015

The tray copy of L-015 (19 new strings: the `panel` section, `settings.startup.requiresApproval` and `settings.startup.unavailable` in place of `notAvailable`, and `errors.command.login_item`) was drafted by the implementing agent (Claude Opus 5.5) in all 12 catalogues and checked by the same judge. en-US is the source and was not judged.

### Method

- Judge: `gemini-3.8-flash` through the maintainer's LiteLLM proxy (`/chat/completions`), temperature 0, `max_tokens` 16000, JSON response format.
- One locale per call. Each prompt held the en-US and the locale's TypeScript for the new keys only, a description of where each string shows (tray panel, tooltip, Startup page, login item error), the copy rules and the language rules of the L-039 method.
- Round 2 ran on every locale with an accepted round 1 fix; en-GB and pt-BR had none. Two rounds at most.

### Calls and tokens

| Locale | Round | Prompt | Completion (reasoning) | Total | Seconds | Issues | Verdict |
|---|---|---|---|---|---|---|---|
| en-GB | 1 | 1037 | 810 (792) | 1847 | 7.0 | 0 | ok |
| de | 1 | 1047 | 2605 (2422) | 3652 | 18.8 | 3 | fix |
| de | 2 | 1053 | 2482 (2341) | 3535 | 18.0 | 2 | fix |
| es | 1 | 1053 | 2775 (2598) | 3828 | 20.3 | 2 | fix |
| es | 2 | 1062 | 2523 (2379) | 3585 | 18.2 | 2 | fix |
| pt-BR | 1 | 1086 | 1886 (1868) | 2972 | 15.0 | 0 | ok |
| pt-PT | 1 | 1079 | 4881 (4787) | 5960 | 34.1 | 1 | fix |
| pt-PT | 2 | 1086 | 2038 (2020) | 3124 | 10.8 | 0 | ok |
| ar | 1 | 1175 | 6105 (5800) | 7280 | 32.9 | 4 | fix |
| ar | 2 | 1183 | 9252 (8957) | 10435 | 65.9 | 4 | fix |
| tr | 1 | 1085 | 2043 (1952) | 3128 | 11.3 | 1 | fix |
| tr | 2 | 1093 | 2036 (2018) | 3129 | 14.1 | 0 | ok |
| hi | 1 | 1069 | 2526 (2438) | 3595 | 13.3 | 1 | fix |
| hi | 2 | 1079 | 3630 (3544) | 4709 | 26.2 | 1 | fix |
| zh-CN | 1 | 1063 | 3635 (3547) | 4698 | 25.1 | 1 | fix |
| zh-CN | 2 | 1069 | 2160 (2142) | 3229 | 11.0 | 0 | ok |
| ja | 1 | 1064 | 4271 (4087) | 5335 | 29.8 | 2 | fix |
| ja | 2 | 1069 | 2870 (2852) | 3939 | 20.8 | 0 | ok |
| he | 1 | 1167 | 2649 (2463) | 3816 | 19.5 | 2 | fix |
| he | 2 | 1180 | 5102 (4841) | 6282 | 28.6 | 2 | fix |
| All (20 calls) | | 21799 | 66279 | 88078 | | 28 | |

### Issues and decisions

28 issues: 20 accepted, 8 rejected. The round 2 fixes were applied without a third call (two-round cap).

Accepted:

| Locale | Round | Key | Change |
|---|---|---|---|
| tr, de, es, hi, ar, pt-PT, he, zh-CN, ja | 1 | `settings.startup.requiresApproval` | The draft dropped "so it starts when you log in"; added in each. |
| de | 1 | `panel.nextIn` | No comma: "Als Nächstes in …". |
| es | 1 | `errors.command.login_item` | "El inicio al iniciar sesión" repeats itself; now "Esta opción solo funciona con una copia instalada de Tolon." (own wording, not the judge's "Abrir al iniciar sesión", which is not the switch label). |
| he | 1 | `panel.ignored` | "בהתעלמות" to the noun "התעלמות". |
| ja | 1 | `panel.summary` | "通知" to "アラーム", the word the rest of the catalogue uses. |
| de | 2 | `panel.later`, `errors.command.login_item` | "Später am Tag"; "funktioniert nur mit einer installierten Version von Tolon". |
| es | 2 | `panel.later`, `panel.pausedUntil` | "Hoy más tarde"; "En pausa hasta …" without "las", which is wrong for 1:00. |
| hi | 2 | `panel.allDay` | Nominative "पूरा दिन" as a label. |
| he | 2 | `settings.startup.requiresApproval`, `panel.summary` | macOS's Hebrew name "פריטי פתיחה"; "שעות שקטות לא פעילות". |

Rejected:

| Locale | Round | Key | Judge's point | Why rejected |
|---|---|---|---|---|
| de | 1 | `panel.later` | "Später heute" is literal; suggests "Heute später". | Round 2 asked again with a better form, "Später am Tag", which was taken. |
| ar | 1, 2 | `panel.label`, `settings.startup.requiresApproval`, `settings.startup.unavailable`, `errors.command.login_item` (7 issues) | Wrap the name Tolon in `iso()`. | No ar string wraps Tolon (L-039): it is one Latin word without punctuation, so the bidi algorithm keeps it in place; `iso()` is for feed and system text such as titles, hosts and keywords, which the new strings do wrap. |

Choices recorded with this review: the tooltip puts "Paused until HH:MM" and the meeting on two lines (a line break needs no per-language separator), and `panel.summary` reuses `interrupt.timeRange` for the quiet hours. Native review is still required before release, as for every catalogue.

### L-015 review fixes

Two new strings, `panel.dismissConsequence` (the one-line consequence next to the panel's Dismiss, N6) and `panel.snoozeEnded` (a panel snooze that would end after the meeting's window, N7), drafted by the implementing agent (Claude Opus 5.5) in all 12 catalogues and checked by the same judge and method. The tooltip no longer takes the title through `iso()` in ar and he: the page now wraps the title in FSI and PDI for every locale (N2).

| Locale | Round | Prompt | Completion (reasoning) | Total | Seconds | Issues | Verdict |
|---|---|---|---|---|---|---|---|
| en-GB | 1 | 415 | 343 (331) | 758 | 2.7 | 0 | ok |
| de | 1 | 414 | 635 (617) | 1049 | 4.6 | 0 | ok |
| es | 1 | 419 | 664 (646) | 1083 | 4.7 | 0 | ok |
| pt-BR | 1 | 436 | 751 (739) | 1187 | 6.5 | 0 | ok |
| pt-PT | 1 | 432 | 731 (713) | 1163 | 5.4 | 0 | ok |
| ar | 1 | 437 | 771 (672) | 1208 | 7.2 | 1 | fix |
| tr | 1 | 421 | 695 (677) | 1116 | 6.0 | 0 | ok |
| hi | 1 | 424 | 499 (492) | 923 | 3.4 | 0 | ok |
| zh-CN | 1 | 429 | 1058 (1040) | 1487 | 6.0 | 0 | ok |
| ja | 1 | 430 | 1650 (1535) | 2080 | 10.9 | 1 | fix |
| ja | 2 | 429 | 975 (963) | 1404 | 11.6 | 0 | ok |
| he | 1 | 443 | 1180 (1162) | 1623 | 10.5 | 0 | ok |
| All (12 calls) | | 5129 | 9952 | 15081 | | 2 | |

Decisions:

| Locale | Round | Key | Judge's point | Decision |
|---|---|---|---|---|
| ja | 1 | `panel.snoozeEnded` | "これで終わりです" is colloquial for UI copy; align with "リマインダー". | Accepted: "その頃には会議が始まっているため、Tolon は再度リマインドしません。" Round 2 ok. |
| ar | 1 | `panel.snoozeEnded` | Wrap the name Tolon in `iso()`. | Rejected, as in the first L-015 round: no ar string wraps the product name. |

## L-038

The fallback notification copy of L-038 (3 new strings in a `notification` section: `body(time)`, `many(count)` and `manyOver`; the title of one meeting is the meeting itself, the test title and the two buttons reuse `interrupt.testTitle`, `interrupt.snooze` and `interrupt.joinCall`) was drafted by the implementing agent (Claude Sonnet 5.5) in all 12 catalogues and checked by the same judge. en-US is the source and was not judged.

### Method

- Judge: `gemini-3.8-flash` through the maintainer's LiteLLM proxy (`/chat/completions`), temperature 0, `max_tokens` 16000, JSON response format.
- One locale per call. Each prompt held the en-US and the locale's TypeScript for the new keys, the reused keys for context, where the strings show (a system notification: title for 2 to 20 meetings, title for more, body with the Intl-formatted local time), the copy rules and the language rules of the L-039 method.
- Round 2 ran only on the locales with an accepted round 1 fix (hi, he). Two rounds at most.

### Calls and tokens

| Locale | Round | Prompt | Completion (reasoning) | Total | Seconds | Issues | Verdict |
|---|---|---|---|---|---|---|---|
| en-GB | 1 | 613 | 292 (274) | 905 | 2.6 | 0 | ok |
| pt-BR | 1 | 629 | 703 (694) | 1332 | 5.7 | 0 | ok |
| pt-PT | 1 | 625 | 674 (656) | 1299 | 5.5 | 0 | ok |
| de | 1 | 615 | 1100 (1082) | 1715 | 6.6 | 0 | ok |
| es | 1 | 621 | 754 (736) | 1375 | 5.8 | 0 | ok |
| ar | 1 | 833 | 1519 (1501) | 2352 | 14.0 | 0 | ok |
| tr | 1 | 617 | 430 (421) | 1047 | 3.6 | 0 | ok |
| hi | 1 | 624 | 1628 (1520) | 2252 | 14.1 | 1 | fix |
| zh-CN | 1 | 612 | 458 (440) | 1070 | 3.3 | 0 | ok |
| ja | 1 | 620 | 889 (880) | 1509 | 7.8 | 0 | ok |
| he | 1 | 751 | 3850 (3698) | 4601 | 32.0 | 1 | fix |
| hi | 2 | 625 | 2009 (1991) | 2634 | 11.3 | 0 | ok |
| he | 2 | 748 | 5471 (5462) | 6219 | 35.6 | 0 | ok |
| All (13 calls) | | 8533 | 19777 | 28310 | | 2 | |

### Issues and decisions

2 issues, both accepted; the round 2 calls found none.

| Locale | Round | Key | Judge's point | Decision |
|---|---|---|---|---|
| hi | 1 | `notification.body` | "घेरना" means to surround or encircle, not to cover a screen. | Accepted: "कवर नहीं कर सका". Round 2 ok. The same verb stays in the earlier hi `takeOver` label ("स्क्रीन घेरें"), which no judge round flagged; left for the native review. |
| he | 1 | `notification.body` | "הפגישה מתחילה" (the meeting starts) clashes with a title for several meetings; use a neutral action noun. | Accepted: "התחלה ב-{time}". Round 2 ok. |

Choices recorded with this review: the body is shared by one meeting and by a group, so no locale names the number of meetings in it. The other locales were left as drafted: ok in round 1. A group above 20 meetings gets a title without a number (`manyOver`), so no plural form is needed beyond the 19 written out. Native review is still required before release.

### L-038 review fix: `notification.untitled`

One more string, `notification.untitled` (the title of a meeting that has none), in all 12 catalogues, drafted by Claude Sonnet 5.5 and checked with the same judge and method as above: 11 calls in one round (en-US is the source), no issues, no second round.

| Locale | Round | Prompt | Completion (reasoning) | Total | Seconds | Issues | Verdict |
|---|---|---|---|---|---|---|---|
| en-GB | 1 | 157 | 145 (136) | 302 | 2.8 | 0 | ok |
| pt-BR | 1 | 164 | 187 (178) | 351 | 3.3 | 0 | ok |
| pt-PT | 1 | 159 | 187 (178) | 346 | 3.8 | 0 | ok |
| de | 1 | 158 | 272 (263) | 430 | 2.3 | 0 | ok |
| es | 1 | 162 | 183 (174) | 345 | 2.9 | 0 | ok |
| ar | 1 | 184 | 169 (160) | 353 | 2.0 | 0 | ok |
| tr | 1 | 160 | 134 (125) | 294 | 1.8 | 0 | ok |
| hi | 1 | 164 | 230 (221) | 394 | 2.5 | 0 | ok |
| zh-CN | 1 | 162 | 190 (181) | 352 | 2.8 | 0 | ok |
| ja | 1 | 160 | 267 (258) | 427 | 3.3 | 0 | ok |
| he | 1 | 188 | 188 (179) | 376 | 2.2 | 0 | ok |
| All (11 calls) | | 1818 | 2152 | 3970 | | 0 | |

## L-018

The first-run copy of L-018 (14 new strings in a `firstRun` section: headline, lead, three step titles and bodies with two variants of the tray step, the two buttons, the preview caption, the sample meeting title and the footer; the preview's buttons, time range and details reuse `interrupt.*`) was drafted by the implementing agent (Claude Opus 5.5) in all 12 catalogues and checked by the same judge. en-US is the source and was not judged; en-GB is the same text.

### Method

- Judge: `gemini-3.8-flash` through the maintainer's LiteLLM proxy (`/chat/completions`), temperature 0, `max_tokens` 16000, JSON response format.
- One locale per call. Each prompt held the en-US and the locale's `firstRun` section, the neighbouring strings of the locale for consistent terms (`testTitle`, `joinCall`, `dismiss`, `hintJoin`, `pasteIcalUrl`, `addCalendar`, `urlHint`), where the strings show (the first-run window, and that `trayMenuBar` is macOS and `traySystemTray` Windows and Linux), the copy rules and the language rules of the L-039 method.
- Round 2 ran only on the locales with an accepted round 1 fix (pt-BR, pt-PT, es, zh-CN, ja). Two rounds at most.

### Calls and tokens

| Locale | Round | Prompt | Completion (reasoning) | Total | Seconds | Issues | Verdict |
|---|---|---|---|---|---|---|---|
| en-GB | 1 | 911 | 888 (870) | 1799 | 7.7 | 0 | ok |
| pt-BR | 1 | 955 | 3309 (3197) | 4264 | 33.3 | 1 | fix |
| pt-PT | 1 | 974 | 2674 (2555) | 3648 | 13.4 | 1 | fix |
| de | 1 | 965 | 936 (927) | 1901 | 7.5 | 0 | ok |
| es | 1 | 957 | 1403 (1316) | 2360 | 13.7 | 1 | fix |
| ar | 1 | 1000 | 1251 (1242) | 2251 | 9.7 | 0 | ok |
| tr | 1 | 961 | 2267 (2249) | 3228 | 18.6 | 0 | ok |
| hi | 1 | 978 | 1482 (1473) | 2460 | 8.8 | 0 | ok |
| zh-CN | 1 | 939 | 4814 (4707) | 5753 | 42.4 | 1 | fix |
| ja | 1 | 968 | 3705 (3464) | 4673 | 32.9 | 2 | fix |
| he | 1 | 1076 | 2226 (2208) | 3302 | 24.0 | 0 | ok |
| pt-BR | 2 | 956 | 1813 (1795) | 2769 | 9.6 | 0 | ok |
| pt-PT | 2 | 975 | 1513 (1495) | 2488 | 12.2 | 0 | ok |
| es | 2 | 956 | 1572 (1563) | 2528 | 14.8 | 0 | ok |
| zh-CN | 2 | 938 | 2925 (2798) | 3863 | 15.3 | 1 | fix |
| ja | 2 | 969 | 2484 (2367) | 3453 | 19.9 | 1 | fix |
| All (16 calls) | | 15478 | 35262 | 50740 | | 8 | |

### Issues and decisions

8 issues: 6 accepted (1 in part), 2 rejected (the same point twice).

| Locale | Round | Key | Judge's point | Decision |
|---|---|---|---|---|
| pt-BR | 1 | `firstRun.footer` | "é código aberto" is a calque; Portuguese says "é de código aberto". | Accepted. Round 2 ok. |
| pt-PT | 1 | `firstRun.footer` | Same point as pt-BR. | Accepted. Round 2 ok. |
| es | 1 | `firstRun.addTitle` | "Añade" clashes with the catalogue's "Agregar calendario"; neutral Spanish prefers "agregar". | Accepted: "Agrega un calendario". The implementer also changed `trayBody` to "cuando agregues" for the same reason. Round 2 ok. The older `keywordPlaceholder` and `keywordsHint` still say "añadir"; left for the native review. |
| zh-CN | 1 | `firstRun.tryAlarm` | "试用测试提醒" doubles "试" and reads like a product trial. | Accepted: "体验测试提醒". |
| ja | 1 | `firstRun.lead` | "見守り" suggests caring for a person; and "画面を表示します" loses "takes over the screen". | In part: "見守り" became "チェックし". "画面を表示します" stays, because every ja string for the takeover uses it (`takeOver` "全画面表示", `atLoginHint`, the Rules and Quiet hours subtitles); changing one string would split the term. Left for the native review. |
| ja | 1 | `firstRun.trayBody` | Two nouns and a verb phrase in one list read as "確認を開く". | Accepted: "今日の会議を確認したり、アラームを一時停止したり、設定を開いたりできます。" |
| zh-CN | 2 | `firstRun.lead` | "关注" reads as a social "follow"; the second sentence is a literal translation. | Accepted without a third round (two rounds at most): "Tolon 会监测你的日历，在会议开始前接管屏幕。数据不会发送至任何地方。" Not re-checked by the judge. |
| ja | 2 | `firstRun.lead` | "画面を表示します" again; suggests "画面を占有します". | Rejected, as in round 1: the catalogue's term for the takeover. Native review to decide for every ja string at once. |

Choices recorded with this review: the tray step names the menu bar (macOS) or the system tray (Windows, Linux); pt-PT says "área de notificação", German "Infobereich" (ar follows in the review fixes below). The sample meeting title in the preview is translated, as a person would name such a meeting. "Apache 2.0" keeps its dot in zh-CN and ja; the full-width punctuation test now ignores a dot between digits. Native review is still required before release.

### L-018 review fixes

Two new strings for the "Skip for now" button of decision "First run (L-018)", `firstRun.skip` and `firstRun.skipConsequence` ("Tolon does nothing until you add a calendar."), in all 12 catalogues, plus two words the review asked for: he `firstRun.tryTitle` "ניסיון" to "לנסות" (an infinitive, like a step title), ar `firstRun.traySystemTray` "علبة النظام" to "منطقة الإعلام" (the Windows term for the notification area). Drafted by Claude Opus 5.5 and checked with the same judge and method; each prompt held the locale's whole `firstRun` section for context and only the changed keys for review. One round: no issues, so no second round.

| Locale | Round | Prompt | Completion (reasoning) | Total | Seconds | Issues | Verdict |
|---|---|---|---|---|---|---|---|
| en-GB | 1 | 590 | 225 (216) | 815 | 3.0 | 0 | ok |
| pt-BR | 1 | 633 | 401 (383) | 1034 | 4.1 | 0 | ok |
| pt-PT | 1 | 641 | 367 (349) | 1008 | 4.5 | 0 | ok |
| de | 1 | 645 | 356 (338) | 1001 | 3.5 | 0 | ok |
| es | 1 | 641 | 298 (289) | 939 | 3.1 | 0 | ok |
| ar | 1 | 708 | 1244 (1235) | 1952 | 8.8 | 0 | ok |
| tr | 1 | 642 | 385 (367) | 1027 | 3.3 | 0 | ok |
| hi | 1 | 650 | 287 (269) | 937 | 3.3 | 0 | ok |
| zh-CN | 1 | 608 | 612 (604) | 1220 | 5.4 | 0 | ok |
| ja | 1 | 633 | 369 (351) | 1002 | 3.8 | 0 | ok |
| he | 1 | 759 | 694 (676) | 1453 | 5.7 | 0 | ok |
| All (11 calls) | | 7150 | 5238 | 12388 | | 0 | |

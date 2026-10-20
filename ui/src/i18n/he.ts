import type { Messages } from "./en-US";

const plural = new Intl.PluralRules("he");
const number = new Intl.NumberFormat("he");

type Forms = Record<"one" | "two" | "other", (n: string) => string>;

/** Hebrew has three plural forms in CLDR: one, two and other. */
const counted = (count: number, forms: Forms) => {
  const form = plural.select(count);
  return (form === "one" || form === "two" ? forms[form] : forms.other)(number.format(count));
};

/**
 * First-strong isolates around text from a feed or the system (names, hosts, keywords), so a Latin
 * name that ends in punctuation, "Team (EU)", keeps its order inside a right-to-left sentence.
 */
const iso = (text: string) => `⁨${text}⁩`;

// Hebrew, right to left. Instructions avoid gendered imperatives: buttons are action nouns
// ("הוספת יומן"), sentences use "אפשר" with an infinitive. Every he-* tag and the legacy iw read it.
export const he: Messages = {
  app: { windowTitle: "Tolon" },
  nav: {
    label: "הגדרות",
    calendars: "יומנים",
    alarms: "התראות",
    quietHours: "שעות שקטות",
    appearance: "מראה",
    startup: "הפעלה",
    about: "אודות",
    comingSoon: "יגיע בגרסה מאוחרת יותר.",
  },
  calendars: {
    title: "יומנים",
    subtitle: "Tolon קורא את היומנים שלך ואף פעם לא כותב בהם.",
    googleName: (account) => `Google: ${iso(account)}`,
    kindGoogle: "יומן Google, מחובר",
    kindIcs: "מינוי iCal",
    kindOutlook: "יומן Outlook שפורסם",
    toggleLabel: (name) => `הפעלת היומן ${iso(name)}`,
    addCalendar: "הוספת יומן",
    pasteIcalUrl: "הדבקת כתובת URL של iCal",
    ignoreHeading: "התעלמות",
    ignoreAllDay: "אירועים של יום שלם",
    ignoreDeclined: "אירועים שדחיתי",
    ignoreSolo: "אירועים בלי אנשים נוספים",
    ignoreMatching: (keywords) => `אירועים שתואמים את: ${iso(keywords)}`,
    changesSave: "השינויים נשמרים מיד.",
    nameWithKind: (name, kind) => `${iso(name)}, ${kind}`,
    removeLabel: (name) => `הסרת היומן ${iso(name)}`,
    removeTitle: (name) => `להסיר את ${iso(name)}?`,
    removeConsequence: "Tolon יפסיק לקרוא את היומן הזה. היומן עצמו לא ישתנה.",
    remove: "הסרה",
    removeConfirm: "הסרת היומן",
    cancel: "ביטול",
    urlPlaceholder: "https://calendar.example.com/team.ics",
    urlHint: "Tolon שומר את הקישור במחשב הזה ורק קורא ממנו.",
    adding: "היומן מתווסף",
    empty: "אין עדיין יומנים. אפשר להדביק כתובת URL של iCal כדי להוסיף יומן.",
  },
  interrupt: {
    region: "התראת פגישה",
    testTitle: "התראת בדיקה",
    joinCall: "הצטרפות לשיחה",
    joinOn: (host) => `הצטרפות דרך ${iso(host)}`,
    openEvent: "פתיחת האירוע",
    alreadyThere: "אני כבר בפגישה",
    snooze: (minutes) =>
      `דחייה ב${counted(minutes, {
        one: () => "דקה אחת",
        two: () => "שתי דקות",
        other: (n) => `-${n} דקות`,
      })}`,
    dismiss: "סגירה",
    dismissConsequence: "סגירה משתיקה את התזכורת הזו במחשב הזה.",
    hintJoin: "Esc לדחייה, Enter להצטרפות, D לסגירה.",
    hintEvent: "Esc לדחייה, Enter לפתיחת האירוע, D לסגירה.",
    hintAlreadyThere: "Esc לדחייה, Enter לסימון שאני כבר בפגישה, D לסגירה.",
    hintSpace: "רווח לסימון שאני כבר בפגישה.",
    timeRange: (start, end) => `${start} עד ${end}`,
    details: (range, provider, people) =>
      `${range}${provider ? ` ב-${iso(provider)}` : ""}${
        people > 0
          ? `, עם ${counted(people, {
              one: () => "אדם אחד",
              two: () => "שני אנשים",
              other: (n) => `${n} אנשים`,
            })}`
          : ""
      }.`,
  },
  relative: {
    neverSynced: "עדיין לא סונכרן",
    syncedJustNow: "סונכרן הרגע",
    syncedAgo: (duration) => `סונכרן לפני ${duration}`,
    startsNow: "מתחילה עכשיו",
    startsIn: (duration) => `מתחילה בעוד ${duration}`,
    startedJustNow: "התחילה הרגע",
    startedAgo: (duration) => `התחילה לפני ${duration}`,
  },
  syncLog: {
    skipped: (count) =>
      counted(count, {
        one: () => "רכיב אחד דולג",
        two: () => "שני רכיבים דולגו",
        other: (n) => `${n} רכיבים דולגו`,
      }),
  },
  errors: {
    sync: {
      http: (status) => `הסנכרון נכשל: ${status}`,
      httpHint: (status, hint) => `הסנכרון נכשל: ${status}, ${hint}`,
      timeout: "הסנכרון נכשל: היומן לא ענה בזמן.",
      connect: "הסנכרון נכשל: Tolon לא הצליח להתחבר לשרת.",
      not_calendar: "הסנכרון נכשל: הקישור הזה אינו יומן iCal.",
      too_large: "הסנכרון נכשל: היומן גדול מדי לקריאה.",
      network: "הסנכרון נכשל: אין חיבור לרשת.",
      unsupported_scheme: "הסנכרון נכשל: רק קישורי https ו-webcal עובדים.",
      invalid_url: "הסנכרון נכשל: זו לא כתובת URL תקינה.",
      unknown: "הסנכרון נכשל.",
    },
    httpHints: {
      unauthorized: "פג תוקף הקישור",
      forbidden: "הגישה נדחתה",
      tooManyRequests: "יותר מדי בקשות",
      server: "שגיאת שרת",
    },
    command: {
      unsupported_scheme: "Tolon יכול להוסיף רק קישורי https או webcal.",
      invalid_url: "זו לא כתובת URL תקינה של יומן.",
      not_found: "היומן הזה כבר לא קיים.",
      no_url: "ליומן הזה אין קישור לסנכרון.",
      not_ready: "Tolon עדיין עולה. אפשר לנסות שוב בעוד רגע.",
      db_error: "Tolon לא הצליח לשמור את השינוי. אפשר לנסות שוב.",
      internal: "משהו השתבש בתוך Tolon. אפשר לנסות שוב.",
    },
  },
};

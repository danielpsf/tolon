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
    rules: "כללים",
    appearance: "מראה",
    language: "שפה",
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
  settings: {
    sameAsSystem: "כמו במערכת",
    alarms: {
      subtitle: "מתי להשתלט על המסך ואיזה צליל להשמיע.",
      takeOver: "השתלטות על המסך",
      leadSuffix: (minutes) => (plural.select(minutes) === "one" ? "דקה לפני ההתחלה" : "דקות לפני ההתחלה"),
      againAtStart: "שוב בהתחלה אם לא נסגר",
      sound: "צליל",
      soundBell: "פעמון רך",
      soundChime: "צלצול",
      soundNone: "ללא",
      play: "השמעת הצליל",
      repeat: "חזרה כל 30 שניות עד לסגירה",
      keyboard: "מקלדת",
      spaceKey: "רווח",
    },
    quiet: {
      subtitle: "בשעות האלה אף התראה לא משתלטת על המסך. Tolon עדיין מציג את האירועים הקרובים.",
      enabled: "שימוש בשעות שקטות",
      from: "התחלה",
      to: "סיום",
      weekends: (saturday, sunday) => `סופי שבוע (${saturday} ו${sunday}) שקטים`,
      weekendsHint: "סופי השבוע נשארים שקטים כל היום.",
    },
    rules: {
      subtitle: "אירועים שאף פעם לא משתלטים על המסך.",
      allDayHint: "חגים, ימי הולדת וימים מחוץ למשרד.",
      declinedHint: "הזמנות שנדחו.",
      cancelled: "אירועים שבוטלו",
      cancelledHint: "אירועים שהמארגן ביטל אבל השאיר ביומן.",
      noAttendeesHint: "אירועים בלי מוזמנים, כמו תזכורות אישיות.",
      keywords: "אירועים עם המילים האלה בכותרת",
      keywordsHint: "אין הבדל בין אותיות גדולות לקטנות. אפשר להקיש Enter או פסיק כדי להוסיף מילה.",
      keywordPlaceholder: "הוספת מילה",
      removeKeyword: (word) => `הסרת ${iso(word)}`,
    },
    appearance: {
      subtitle: "גם מסך ההתראה פועל לפי ההגדרה הזו.",
      theme: "ערכת נושא",
      light: "בהיר",
      dark: "כהה",
    },
    language: {
      subtitle: "השפה של כל החלונות של Tolon.",
      label: "שפה",
    },
    startup: {
      subtitle: "Tolon יכול לעלות מעצמו, כך שאף פגישה לא תוחמץ אחרי הפעלה מחדש.",
      atLogin: "הפעלת Tolon בכניסה למערכת",
      atLoginHint: "Tolon ממתין ברקע ומשתלט על המסך רק לפני פגישה.",
      requiresApproval: "אפשר לאשר את Tolon בהגדרות המערכת, תחת פריטי פתיחה, כדי שיופעל בעת ההתחברות.",
      unavailable: "Tolon לא הצליח לקרוא את ההגדרה הזו מהמערכת.",
    },
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
  panel: {
    label: "היום ב-Tolon",
    nextIn: (duration) => `הבאה בעוד ${duration}`,
    noMore: "אין עוד פגישות היום.",
    later: "בהמשך היום",
    pausedUntil: (time) => `מושהה עד ${time}`,
    pause: "השהיה לשעה",
    resume: "חידוש ההתראות",
    settings: "הגדרות",
    quit: "יציאה",
    ignored: "התעלמות",
    allDay: "כל היום",
    noLink: "אין קישור",
    eventPage: "דף האירוע",
    summary: (minutes, quiet) =>
      `התראה ${counted(minutes, {
        one: () => "דקה אחת",
        two: () => "שתי דקות",
        other: (n) => `${n} דקות`,
      })} לפני. ${quiet ? `שעות שקטות: ${iso(quiet)}.` : "שעות שקטות לא פעילות."}`,
    dismissConsequence: "סגירה משתיקה את כל התזכורות לפגישה הזו במחשב הזה.",
    snoozeEnded: "עד אז הפגישה כבר תתחיל, ולכן Tolon לא יזכיר שוב.",
    tooltip: (title, duration) => `${title} בעוד ${duration}`,
    tooltipNow: (title) => `עכשיו: ${title}`,
  },
  firstRun: {
    title: "לא מפספסים יותר אף פגישה.",
    lead: "Tolon עוקב אחרי היומנים שלך ומשתלט על המסך לפני שפגישה מתחילה. שום דבר לא נשלח לשום מקום.",
    addTitle: "הוספת יומן",
    addBody: "אפשר להדביק את קישור ה-iCal הפרטי מכל יומן. Tolon רק קורא אותו.",
    tryTitle: "לנסות",
    tryBody: "התראת בדיקה מראה בדיוק מה יקרה. Esc לדחייה, Enter להצטרפות.",
    trayMenuBar: "Tolon נמצא בשורת התפריטים",
    traySystemTray: "Tolon נמצא במגש המערכת",
    trayBody: "החלון הזה ייסגר אחרי הוספת יומן. אפשר ללחוץ על הסמל של Tolon כדי לראות את הפגישות של היום, להשהות התראות או לפתוח את ההגדרות.",
    pasteIcal: "הדבקת קישור iCal",
    tryAlarm: "הפעלת התראת בדיקה",
    previewLabel: "תצוגה מקדימה של ההתראה",
    sampleTitle: "סנכרון שבועי של צוות הפלטפורמה",
    footer: "Tolon הוא קוד פתוח ברישיון Apache 2.0. בלי חשבון, בלי טלמטריה.",
    skip: "דילוג בינתיים",
    skipConsequence: "Tolon לא עושה דבר עד שמוסיפים יומן.",
  },
  notification: {
    body: (time) => `התחלה ב-${time}. Tolon לא הצליח לכסות את המסך.`,
    many: (count) => counted(count, {
      one: () => "פגישה אחת עומדת להתחיל",
      two: () => "שתי פגישות עומדות להתחיל",
      other: (n) => `${n} פגישות עומדות להתחיל`,
    }),
    manyOver: "כמה פגישות עומדות להתחיל",
    untitled: "פגישה ללא כותרת",
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
    invalid: {
      lead_minutes: "אפשר לבחור זמן מראש בין 1 ל-60 דקות.",
      time: "אפשר להזין שעה בשעות ובדקות.",
      keywords: "עד 50 מילים, כל אחת עד 64 תווים.",
      other: "Tolon לא הצליח לשמור את הערך הזה. אפשר לנסות ערך אחר.",
    },
    command: {
      unsupported_scheme: "Tolon יכול להוסיף רק קישורי https או webcal.",
      invalid_url: "זו לא כתובת URL תקינה של יומן.",
      not_found: "היומן הזה כבר לא קיים.",
      no_url: "ליומן הזה אין קישור לסנכרון.",
      not_ready: "Tolon עדיין עולה. אפשר לנסות שוב בעוד רגע.",
      db_error: "Tolon לא הצליח לשמור את השינוי. אפשר לנסות שוב.",
      internal: "משהו השתבש בתוך Tolon. אפשר לנסות שוב.",
      login_item: "המערכת לא אפשרה ל-Tolon לשנות את זה. הפעלה בכניסה למערכת עובדת רק בעותק מותקן של Tolon.",
    },
  },
};

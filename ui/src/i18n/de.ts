import type { Messages } from "./en-US";

const plural = new Intl.PluralRules("de");
const number = new Intl.NumberFormat("de");

// German, informal "du". Every de-* tag (de-AT, de-CH ...) reads this catalogue.
export const de: Messages = {
  app: { windowTitle: "Tolon" },
  nav: {
    label: "Einstellungen",
    calendars: "Kalender",
    alarms: "Alarme",
    quietHours: "Ruhezeiten",
    appearance: "Darstellung",
    startup: "Systemstart",
    about: "Info",
    comingSoon: "Kommt in einer späteren Version.",
  },
  calendars: {
    title: "Kalender",
    subtitle: "Tolon liest deine Kalender und schreibt nie etwas hinein.",
    googleName: (account) => `Google: ${account}`,
    kindGoogle: "Google Kalender, angemeldet",
    kindIcs: "iCal-Abo",
    kindOutlook: "Veröffentlichter Outlook-Kalender",
    toggleLabel: (name) => `Kalender ${name} aktivieren`,
    addCalendar: "Kalender hinzufügen",
    pasteIcalUrl: "iCal-URL einfügen",
    ignoreHeading: "Ignorieren",
    ignoreAllDay: "Ganztägige Termine",
    ignoreDeclined: "Termine, die ich abgelehnt habe",
    ignoreSolo: "Termine ohne andere Personen",
    ignoreMatching: (keywords) => `Termine, die zu ${keywords} passen`,
    changesSave: "Änderungen werden sofort gespeichert.",
    nameWithKind: (name, kind) => `${name}, ${kind}`,
    removeLabel: (name) => `Kalender ${name} entfernen`,
    removeTitle: (name) => `${name} entfernen?`,
    removeConsequence: "Tolon liest diesen Kalender nicht mehr. Der Kalender selbst bleibt unverändert.",
    remove: "Entfernen",
    removeConfirm: "Kalender entfernen",
    cancel: "Abbrechen",
    urlPlaceholder: "https://calendar.example.com/team.ics",
    urlHint: "Tolon speichert den Link auf diesem Computer und liest nur daraus.",
    adding: "Kalender wird hinzugefügt",
    empty: "Noch keine Kalender. Füge eine iCal-URL ein, um einen hinzuzufügen.",
  },
  interrupt: {
    region: "Meeting-Alarm",
    testTitle: "Testalarm",
    joinCall: "Am Anruf teilnehmen",
    joinOn: (host) => `Über ${host} teilnehmen`,
    openEvent: "Termin öffnen",
    alreadyThere: "Ich bin schon dabei",
    snooze: (minutes) => `${number.format(minutes)} Min. schlummern`,
    dismiss: "Verwerfen",
    dismissConsequence: "Verwerfen schaltet diese Erinnerung auf diesem Computer stumm.",
    hintJoin: "Esc: schlummern. Enter: teilnehmen. D: verwerfen.",
    hintEvent: "Esc: schlummern. Enter: Termin öffnen. D: verwerfen.",
    hintAlreadyThere: "Esc: schlummern. Enter: Ich bin schon dabei. D: verwerfen.",
    hintSpace: "Leertaste: Ich bin schon dabei.",
    timeRange: (start, end) => `${start} bis ${end}`,
    details: (range, provider, people) =>
      `${range}${provider ? ` über ${provider}` : ""}${
        people > 0 ? ` mit ${number.format(people)} ${plural.select(people) === "one" ? "Person" : "Personen"}` : ""
      }.`,
  },
  relative: {
    neverSynced: "Noch nicht synchronisiert",
    syncedJustNow: "Gerade synchronisiert",
    syncedAgo: (duration) => `Vor ${duration} synchronisiert`,
    startsNow: "Beginnt jetzt",
    startsIn: (duration) => `Beginnt in ${duration}`,
    startedJustNow: "Hat gerade begonnen",
    startedAgo: (duration) => `Hat vor ${duration} begonnen`,
  },
  syncLog: {
    skipped: (count) =>
      `${number.format(count)} ${plural.select(count) === "one" ? "Komponente" : "Komponenten"} übersprungen`,
  },
  errors: {
    sync: {
      http: (status) => `Synchronisierung fehlgeschlagen: ${status}`,
      httpHint: (status, hint) => `Synchronisierung fehlgeschlagen: ${status}, ${hint}`,
      timeout: "Synchronisierung fehlgeschlagen: Der Kalender hat zu lange nicht geantwortet.",
      connect: "Synchronisierung fehlgeschlagen: Tolon konnte keine Verbindung zum Server herstellen.",
      not_calendar: "Synchronisierung fehlgeschlagen: Dieser Link ist kein iCal-Kalender.",
      too_large: "Synchronisierung fehlgeschlagen: Der Kalender ist zu groß zum Lesen.",
      network: "Synchronisierung fehlgeschlagen: Es besteht keine Netzwerkverbindung.",
      unsupported_scheme: "Synchronisierung fehlgeschlagen: Nur https- und webcal-Links funktionieren.",
      invalid_url: "Synchronisierung fehlgeschlagen: Das ist keine gültige URL.",
      unknown: "Synchronisierung fehlgeschlagen.",
    },
    httpHints: {
      unauthorized: "Link abgelaufen",
      forbidden: "Zugriff verweigert",
      tooManyRequests: "zu viele Anfragen",
      server: "Serverfehler",
    },
    command: {
      unsupported_scheme: "Tolon kann nur https- oder webcal-Links hinzufügen.",
      invalid_url: "Das ist keine gültige Kalender-URL.",
      not_found: "Diesen Kalender gibt es nicht mehr.",
      no_url: "Dieser Kalender hat keinen Link zum Synchronisieren.",
      not_ready: "Tolon startet noch. Versuch es gleich noch einmal.",
      db_error: "Tolon konnte die Änderung nicht speichern. Versuch es noch einmal.",
      internal: "In Tolon ist etwas schiefgelaufen. Versuch es noch einmal.",
    },
  },
};

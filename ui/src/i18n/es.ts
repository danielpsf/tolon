import type { Messages } from "./en-US";

const plural = new Intl.PluralRules("es");
const number = new Intl.NumberFormat("es");

// Neutral international Spanish with "tú". Every es-* tag (es-ES, es-MX, es-419 ...) reads this catalogue.
export const es: Messages = {
  app: { windowTitle: "Tolon" },
  nav: {
    label: "Ajustes",
    calendars: "Calendarios",
    alarms: "Alarmas",
    quietHours: "Horas de silencio",
    appearance: "Apariencia",
    startup: "Inicio",
    about: "Acerca de",
    comingSoon: "Disponible en una versión futura.",
  },
  calendars: {
    title: "Calendarios",
    subtitle: "Tolon lee tus calendarios y nunca escribe en ellos.",
    googleName: (account) => `Google: ${account}`,
    kindGoogle: "Google Calendar, sesión iniciada",
    kindIcs: "Suscripción iCal",
    kindOutlook: "Calendario de Outlook publicado",
    toggleLabel: (name) => `Activar el calendario ${name}`,
    addCalendar: "Agregar calendario",
    pasteIcalUrl: "Pegar una URL de iCal",
    ignoreHeading: "Ignorar",
    ignoreAllDay: "Eventos de todo el día",
    ignoreDeclined: "Eventos que rechacé",
    ignoreSolo: "Eventos sin otras personas",
    ignoreMatching: (keywords) => `Eventos que coinciden con: ${keywords}`,
    changesSave: "Los cambios se guardan al instante.",
    nameWithKind: (name, kind) => `${name}, ${kind}`,
    removeLabel: (name) => `Quitar el calendario ${name}`,
    removeTitle: (name) => `¿Quitar ${name}?`,
    removeConsequence: "Tolon deja de leer este calendario. El calendario en sí no cambia.",
    remove: "Quitar",
    removeConfirm: "Quitar calendario",
    cancel: "Cancelar",
    urlPlaceholder: "https://calendar.example.com/team.ics",
    urlHint: "Tolon guarda el enlace en este equipo y solo lee de él.",
    adding: "Agregando calendario",
    empty: "Aún no hay calendarios. Pega una URL de iCal para agregar uno.",
  },
  interrupt: {
    region: "Alarma de reunión",
    testTitle: "Alarma de prueba",
    joinCall: "Unirse a la llamada",
    joinOn: (host) => `Unirse en ${host}`,
    openEvent: "Abrir el evento",
    alreadyThere: "Ya estoy ahí",
    snooze: (minutes) => `Posponer ${number.format(minutes)} min`,
    dismiss: "Descartar",
    dismissConsequence: "Descartar silencia este recordatorio en este equipo.",
    hintJoin: "Esc: posponer. Enter: unirse. D: descartar.",
    hintEvent: "Esc: posponer. Enter: abrir el evento. D: descartar.",
    hintAlreadyThere: "Esc: posponer. Enter: ya estoy ahí. D: descartar.",
    hintSpace: "Espacio: ya estoy ahí.",
    timeRange: (start, end) => `${start} a ${end}`,
    details: (range, provider, people) =>
      `${range}${provider ? ` en ${provider}` : ""}${
        people > 0 ? ` con ${number.format(people)} ${plural.select(people) === "one" ? "persona" : "personas"}` : ""
      }.`,
  },
  relative: {
    neverSynced: "Aún sin sincronizar",
    syncedJustNow: "Sincronizado hace un momento",
    syncedAgo: (duration) => `Sincronizado hace ${duration}`,
    startsNow: "Empieza ahora",
    startsIn: (duration) => `Empieza en ${duration}`,
    startedJustNow: "Empezó hace un momento",
    startedAgo: (duration) => `Empezó hace ${duration}`,
  },
  syncLog: {
    skipped: (count) =>
      `${number.format(count)} ${plural.select(count) === "one" ? "componente omitido" : "componentes omitidos"}`,
  },
  errors: {
    sync: {
      http: (status) => `Falló la sincronización: ${status}`,
      httpHint: (status, hint) => `Falló la sincronización: ${status}, ${hint}`,
      timeout: "Falló la sincronización: el calendario tardó demasiado en responder.",
      connect: "Falló la sincronización: Tolon no pudo conectarse al servidor.",
      not_calendar: "Falló la sincronización: ese enlace no es un calendario iCal.",
      too_large: "Falló la sincronización: el calendario es demasiado grande para leerlo.",
      network: "Falló la sincronización: no hay conexión de red.",
      unsupported_scheme: "Falló la sincronización: solo funcionan los enlaces https y webcal.",
      invalid_url: "Falló la sincronización: esa URL no es válida.",
      unknown: "Falló la sincronización.",
    },
    httpHints: {
      unauthorized: "enlace caducado",
      forbidden: "acceso denegado",
      tooManyRequests: "demasiadas solicitudes",
      server: "error del servidor",
    },
    command: {
      unsupported_scheme: "Tolon solo puede agregar enlaces https o webcal.",
      invalid_url: "Esa URL de calendario no es válida.",
      not_found: "Ese calendario ya no existe.",
      no_url: "Este calendario no tiene un enlace para sincronizar.",
      not_ready: "Tolon aún se está iniciando. Inténtalo de nuevo en un momento.",
      db_error: "Tolon no pudo guardar ese cambio. Inténtalo de nuevo.",
      internal: "Algo salió mal dentro de Tolon. Inténtalo de nuevo.",
    },
  },
};

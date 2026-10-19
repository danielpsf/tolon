import type { Messages } from "./en-US";

const plural = new Intl.PluralRules("pt-PT");
const number = new Intl.NumberFormat("pt-PT");

// European Portuguese. A separate catalogue from pt-BR: vocabulary, "ligação" for link, "guardar" for save.
export const ptPT: Messages = {
  app: { windowTitle: "Tolon" },
  nav: {
    label: "Definições",
    calendars: "Calendários",
    alarms: "Alarmes",
    quietHours: "Horário de silêncio",
    appearance: "Aspeto",
    startup: "Arranque",
    about: "Acerca de",
  },
  calendars: {
    title: "Calendários",
    subtitle: "O Tolon lê os seus calendários e nunca escreve neles.",
    googleName: (account) => `Google: ${account}`,
    kindGoogle: "Google Agenda, sessão iniciada",
    kindIcs: "Subscrição iCal",
    kindOutlook: "Calendário do Outlook publicado",
    toggleLabel: (name) => `Ativar o calendário ${name}`,
    addCalendar: "Adicionar calendário",
    pasteIcalUrl: "Colar um URL iCal",
    ignoreHeading: "Ignorar",
    ignoreAllDay: "Eventos de dia inteiro",
    ignoreDeclined: "Eventos que recusei",
    ignoreSolo: "Eventos sem outras pessoas",
    ignoreMatching: (keywords) => `Eventos que correspondem a: ${keywords}`,
    changesSave: "As alterações são guardadas automaticamente.",
  },
  relative: {
    neverSynced: "Ainda não sincronizado",
    syncedJustNow: "Sincronizado agora mesmo",
    syncedAgo: (duration) => `Sincronizado há ${duration}`,
    startsNow: "Começa agora",
    startsIn: (duration) => `Começa dentro de ${duration}`,
    startedJustNow: "Começou agora mesmo",
    startedAgo: (duration) => `Começou há ${duration}`,
  },
  syncLog: {
    skipped: (count) =>
      `${number.format(count)} ${plural.select(count) === "one" ? "componente ignorado" : "componentes ignorados"}`,
  },
  errors: {
    sync: {
      http: (status) => `A sincronização falhou: ${status}`,
      httpHint: (status, hint) => `A sincronização falhou: ${status}, ${hint}`,
      timeout: "A sincronização falhou: o calendário demorou demasiado tempo a responder.",
      connect: "A sincronização falhou: o Tolon não conseguiu ligar ao servidor.",
      not_calendar: "A sincronização falhou: essa ligação não é um calendário iCal.",
      too_large: "A sincronização falhou: o calendário é demasiado grande para ser lido.",
      network: "A sincronização falhou: não há ligação à rede.",
      unsupported_scheme: "A sincronização falhou: só as ligações https e webcal funcionam.",
      invalid_url: "A sincronização falhou: esse URL não é válido.",
      unknown: "A sincronização falhou.",
    },
    httpHints: {
      unauthorized: "endereço expirado",
      forbidden: "acesso negado",
      tooManyRequests: "demasiados pedidos",
      server: "erro do servidor",
    },
    command: {
      unsupported_scheme: "O Tolon só adiciona ligações https ou webcal.",
      invalid_url: "Esse URL de calendário não é válido.",
      not_found: "Esse calendário já não existe.",
      no_url: "Este calendário não tem uma ligação para sincronizar.",
      not_ready: "O Tolon ainda está a iniciar. Tente novamente dentro de instantes.",
      db_error: "O Tolon não conseguiu guardar a alteração. Tente novamente.",
      internal: "Ocorreu um erro interno no Tolon. Tente novamente.",
    },
  },
};

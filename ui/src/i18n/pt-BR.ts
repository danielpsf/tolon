import type { Messages } from "./en-US";

const plural = new Intl.PluralRules("pt-BR");
const number = new Intl.NumberFormat("pt-BR");

// Brazilian Portuguese, informal "você". Also the fallback for every pt-* except pt-PT.
export const ptBR: Messages = {
  app: { windowTitle: "Tolon" },
  nav: {
    label: "Configurações",
    calendars: "Calendários",
    alarms: "Alarmes",
    quietHours: "Horário de silêncio",
    appearance: "Aparência",
    startup: "Inicialização",
    about: "Sobre",
    comingSoon: "Disponível em uma versão futura.",
  },
  calendars: {
    title: "Calendários",
    subtitle: "O Tolon lê seus calendários e nunca escreve neles.",
    googleName: (account) => `Google: ${account}`,
    kindGoogle: "Google Agenda, conectado",
    kindIcs: "Assinatura iCal",
    kindOutlook: "Calendário publicado do Outlook",
    toggleLabel: (name) => `Ativar o calendário ${name}`,
    addCalendar: "Adicionar calendário",
    pasteIcalUrl: "Colar uma URL iCal",
    ignoreHeading: "Ignorar",
    ignoreAllDay: "Eventos de dia inteiro",
    ignoreDeclined: "Eventos que recusei",
    ignoreSolo: "Eventos sem outras pessoas",
    ignoreMatching: (keywords) => `Eventos que correspondem a: ${keywords}`,
    changesSave: "As alterações são salvas automaticamente.",
    nameWithKind: (name, kind) => `${name}, ${kind}`,
    removeLabel: (name) => `Remover o calendário ${name}`,
    removeTitle: (name) => `Remover ${name}?`,
    removeConsequence: "O Tolon deixa de ler este calendário. O calendário em si não é alterado.",
    remove: "Remover",
    removeConfirm: "Remover calendário",
    cancel: "Cancelar",
    urlPlaceholder: "https://calendar.example.com/team.ics",
    urlHint: "O Tolon guarda o link neste computador e só lê o calendário.",
    adding: "Adicionando calendário",
    empty: "Nenhum calendário ainda. Cole uma URL iCal para adicionar um.",
  },
  relative: {
    neverSynced: "Ainda não sincronizado",
    syncedJustNow: "Sincronizado agora mesmo",
    syncedAgo: (duration) => `Sincronizado há ${duration}`,
    startsNow: "Começa agora",
    startsIn: (duration) => `Começa em ${duration}`,
    startedJustNow: "Começou agora mesmo",
    startedAgo: (duration) => `Começou há ${duration}`,
  },
  syncLog: {
    skipped: (count) =>
      `${number.format(count)} ${plural.select(count) === "one" ? "componente ignorado" : "componentes ignorados"}`,
  },
  errors: {
    sync: {
      http: (status) => `Falha na sincronização: ${status}`,
      httpHint: (status, hint) => `Falha na sincronização: ${status}, ${hint}`,
      timeout: "Falha na sincronização: o calendário demorou demais para responder.",
      connect: "Falha na sincronização: o Tolon não conseguiu conectar ao servidor.",
      not_calendar: "Falha na sincronização: esse link não é um calendário iCal.",
      too_large: "Falha na sincronização: o calendário é grande demais para ser lido.",
      network: "Falha na sincronização: não há conexão de rede.",
      unsupported_scheme: "Falha na sincronização: só links https e webcal funcionam.",
      invalid_url: "Falha na sincronização: essa URL não é válida.",
      unknown: "Falha na sincronização.",
    },
    httpHints: {
      unauthorized: "link expirado",
      forbidden: "acesso negado",
      tooManyRequests: "muitas solicitações",
      server: "erro no servidor",
    },
    command: {
      unsupported_scheme: "O Tolon só adiciona links https ou webcal.",
      invalid_url: "Essa URL de calendário não é válida.",
      not_found: "Esse calendário não existe mais.",
      no_url: "Este calendário não tem um link para sincronizar.",
      not_ready: "O Tolon ainda está iniciando. Tente de novo em instantes.",
      db_error: "O Tolon não conseguiu salvar a alteração. Tente de novo.",
      internal: "Algo deu errado dentro do Tolon. Tente de novo.",
    },
  },
};

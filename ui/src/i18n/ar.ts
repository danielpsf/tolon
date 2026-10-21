import type { Messages } from "./en-US";

const plural = new Intl.PluralRules("ar");
const number = new Intl.NumberFormat("ar");

type Forms = Record<Intl.LDMLPluralRule, (n: string) => string>;

/** Arabic has six plural forms: zero, one, two, few (3-10), many (11-99) and other (100, 101 ...). */
const counted = (count: number, forms: Forms) => forms[plural.select(count)](number.format(count));

/**
 * First-strong isolates around text from a feed or the system (names, hosts, keywords), so a Latin
 * name that ends in punctuation, "Team (EU)", keeps its order inside a right-to-left sentence.
 */
const iso = (text: string) => `⁨${text}⁩`;

// Modern Standard Arabic, right to left, Arabic punctuation (، ؟). Every ar-* tag reads this catalogue.
export const ar: Messages = {
  app: { windowTitle: "Tolon" },
  nav: {
    label: "الإعدادات",
    calendars: "التقويمات",
    alarms: "التنبيهات",
    quietHours: "ساعات الهدوء",
    rules: "القواعد",
    appearance: "المظهر",
    language: "اللغة",
    startup: "بدء التشغيل",
    about: "حول",
    comingSoon: "سيتوفر في إصدار لاحق.",
  },
  calendars: {
    title: "التقويمات",
    subtitle: "يقرأ Tolon تقويماتك ولا يكتب فيها أبدًا.",
    googleName: (account) => `Google: ${iso(account)}`,
    kindGoogle: "تقويم Google، تم تسجيل الدخول",
    kindIcs: "اشتراك iCal",
    kindOutlook: "تقويم Outlook منشور",
    toggleLabel: (name) => `تفعيل التقويم ${iso(name)}`,
    addCalendar: "إضافة تقويم",
    pasteIcalUrl: "لصق عنوان URL لتقويم iCal",
    ignoreHeading: "تجاهل",
    ignoreAllDay: "الأحداث التي تستغرق اليوم كله",
    ignoreDeclined: "الأحداث التي رفضتها",
    ignoreSolo: "الأحداث التي لا تضم أشخاصًا آخرين",
    ignoreMatching: (keywords) => `الأحداث المطابقة لـ: ${iso(keywords)}`,
    changesSave: "تُحفظ التغييرات فور إجرائها.",
    nameWithKind: (name, kind) => `${iso(name)}، ${kind}`,
    removeLabel: (name) => `إزالة التقويم ${iso(name)}`,
    removeTitle: (name) => `إزالة ${iso(name)}؟`,
    removeConsequence: "سيتوقف Tolon عن قراءة هذا التقويم. لن يتغير التقويم نفسه.",
    remove: "إزالة",
    removeConfirm: "إزالة التقويم",
    cancel: "إلغاء",
    urlPlaceholder: "https://calendar.example.com/team.ics",
    urlHint: "يحفظ Tolon الرابط على هذا الكمبيوتر ويقرأ منه فقط.",
    adding: "جارٍ إضافة التقويم",
    empty: "لا توجد تقويمات بعد. الصق عنوان URL لتقويم iCal لإضافة تقويم.",
  },
  settings: {
    sameAsSystem: "مثل النظام",
    alarms: {
      subtitle: "متى يملأ التنبيه الشاشة، وأي صوت يُشغَّل.",
      takeOver: "ملء الشاشة",
      leadSuffix: (minutes) =>
        counted(minutes, {
          zero: () => "دقيقة قبل البدء",
          one: () => "دقيقة قبل البدء",
          two: () => "دقيقتان قبل البدء",
          few: () => "دقائق قبل البدء",
          many: () => "دقيقة قبل البدء",
          other: () => "دقيقة قبل البدء",
        }),
      againAtStart: "مرة أخرى عند البدء إن لم يُغلق",
      sound: "الصوت",
      soundBell: "جرس هادئ",
      soundChime: "رنين",
      soundNone: "بلا صوت",
      play: "تشغيل الصوت",
      repeat: "التكرار كل 30 ثانية حتى الإغلاق",
      keyboard: "لوحة المفاتيح",
      spaceKey: "المسافة",
    },
    quiet: {
      subtitle: "لا يملأ أي تنبيه الشاشة في هذه الساعات، ويواصل Tolon عرض ما هو قادم.",
      enabled: "تفعيل ساعات الهدوء",
      from: "من",
      to: "إلى",
      weekends: (saturday, sunday) => `عطلة نهاية الأسبوع (${saturday} و${sunday}) هادئة`,
      weekendsHint: "تبقى عطلة نهاية الأسبوع هادئة طوال اليوم.",
    },
    rules: {
      subtitle: "أحداث لا تملأ الشاشة أبدًا.",
      allDayHint: "العطلات وأعياد الميلاد وأيام الغياب عن المكتب.",
      declinedHint: "الدعوات التي رفضتها.",
      cancelled: "الأحداث الملغاة",
      cancelledHint: "أحداث ألغاها المنظم لكنها بقيت في التقويم.",
      noAttendeesHint: "أحداث بلا مدعوين، مثل التذكيرات التي تضعها لنفسك.",
      keywords: "الأحداث التي تحتوي عناوينها على هذه الكلمات",
      keywordsHint: "لا فرق بين الأحرف الكبيرة والصغيرة. اضغط Enter أو الفاصلة لإضافة كلمة.",
      keywordPlaceholder: "إضافة كلمة",
      removeKeyword: (word) => `إزالة ${iso(word)}`,
    },
    appearance: {
      subtitle: "تتبع شاشة التنبيه هذا الإعداد أيضًا.",
      theme: "السمة",
      light: "فاتح",
      dark: "داكن",
    },
    language: {
      subtitle: "لغة جميع نوافذ Tolon.",
      label: "اللغة",
    },
    startup: {
      subtitle: "يمكن أن يبدأ Tolon تلقائيًا، فلا يفوتك اجتماع بعد إعادة التشغيل.",
      atLogin: "تشغيل Tolon عند تسجيل الدخول",
      atLoginHint: "ينتظر Tolon في الخلفية ولا يملأ الشاشة إلا قبل الاجتماع.",
      requiresApproval: "اسمح لـ Tolon في إعدادات النظام، ضمن عناصر تسجيل الدخول، حتى يبدأ عند تسجيل الدخول.",
      unavailable: "تعذّر على Tolon قراءة هذا الإعداد من النظام.",
    },
  },
  interrupt: {
    region: "تنبيه الاجتماع",
    testTitle: "تنبيه تجريبي",
    joinCall: "الانضمام إلى المكالمة",
    joinOn: (host) => `الانضمام عبر ${iso(host)}`,
    openEvent: "فتح الحدث",
    alreadyThere: "أنا في الاجتماع بالفعل",
    snooze: (minutes) =>
      `تأجيل ${counted(minutes, {
        zero: (n) => `${n} دقيقة`,
        one: () => "دقيقة واحدة",
        two: () => "دقيقتين",
        few: (n) => `${n} دقائق`,
        many: (n) => `${n} دقيقة`,
        other: (n) => `${n} دقيقة`,
      })}`,
    dismiss: "إغلاق",
    dismissConsequence: "يؤدي الإغلاق إلى إسكات هذا التذكير على هذا الكمبيوتر.",
    hintJoin: "Esc للتأجيل، و Enter للانضمام، و D للإغلاق.",
    hintEvent: "Esc للتأجيل، و Enter لفتح الحدث، و D للإغلاق.",
    hintAlreadyThere: "Esc للتأجيل، و Enter لتأكيد أنك في الاجتماع، و D للإغلاق.",
    hintSpace: "مفتاح المسافة لتأكيد أنك في الاجتماع.",
    timeRange: (start, end) => `من ${start} إلى ${end}`,
    details: (range, provider, people) =>
      `${range}${provider ? ` عبر ${iso(provider)}` : ""}${
        people > 0
          ? ` ${counted(people, {
              zero: (n) => `مع ${n} شخص`,
              one: () => "مع شخص واحد",
              two: () => "مع شخصين",
              few: (n) => `مع ${n} أشخاص`,
              many: (n) => `مع ${n} شخصًا`,
              other: (n) => `مع ${n} شخص`,
            })}`
          : ""
      }.`,
  },
  relative: {
    neverSynced: "لم تتم المزامنة بعد",
    syncedJustNow: "تمت المزامنة الآن",
    syncedAgo: (duration) => `تمت المزامنة منذ ${duration}`,
    startsNow: "يبدأ الآن",
    startsIn: (duration) => `يبدأ بعد ${duration}`,
    startedJustNow: "بدأ للتو",
    startedAgo: (duration) => `بدأ منذ ${duration}`,
  },
  syncLog: {
    skipped: (count) =>
      counted(count, {
        zero: () => "لم يتم تخطي أي مكوّن",
        one: () => "تم تخطي مكوّن واحد",
        two: () => "تم تخطي مكوّنين",
        few: (n) => `تم تخطي ${n} مكوّنات`,
        many: (n) => `تم تخطي ${n} مكوّنًا`,
        other: (n) => `تم تخطي ${n} مكوّن`,
      }),
  },
  errors: {
    sync: {
      http: (status) => `فشلت المزامنة: ${status}`,
      httpHint: (status, hint) => `فشلت المزامنة: ${status}، ${hint}`,
      timeout: "فشلت المزامنة: استغرق التقويم وقتًا طويلًا للرد.",
      connect: "فشلت المزامنة: تعذّر على Tolon الاتصال بالخادم.",
      not_calendar: "فشلت المزامنة: هذا الرابط ليس تقويم iCal.",
      too_large: "فشلت المزامنة: التقويم أكبر من أن تتم قراءته.",
      network: "فشلت المزامنة: لا يوجد اتصال بالشبكة.",
      unsupported_scheme: "فشلت المزامنة: لا تعمل إلا روابط https و webcal.",
      invalid_url: "فشلت المزامنة: هذا ليس عنوان URL صالحًا.",
      unknown: "فشلت المزامنة.",
    },
    httpHints: {
      unauthorized: "انتهت صلاحية الرابط",
      forbidden: "تم رفض الوصول",
      tooManyRequests: "طلبات كثيرة جدًا",
      server: "خطأ في الخادم",
    },
    invalid: {
      lead_minutes: "اختر مدة من 1 إلى 60 دقيقة.",
      time: "أدخل الوقت بالساعات والدقائق.",
      keywords: "استخدم 50 كلمة كحد أقصى، بما لا يتجاوز 64 حرفًا لكل كلمة.",
      other: "تعذر على Tolon حفظ هذه القيمة. جرّب قيمة أخرى.",
    },
    command: {
      unsupported_scheme: "لا يضيف Tolon إلا روابط https أو webcal.",
      invalid_url: "هذا ليس عنوان URL صالحًا لتقويم.",
      not_found: "لم يعد هذا التقويم موجودًا.",
      no_url: "لا يحتوي هذا التقويم على رابط للمزامنة.",
      not_ready: "لا يزال Tolon قيد البدء. حاول مرة أخرى بعد لحظة.",
      db_error: "تعذّر على Tolon حفظ هذا التغيير. حاول مرة أخرى.",
      internal: "حدث خطأ ما داخل Tolon. حاول مرة أخرى.",
      login_item: "لم يسمح النظام لـ Tolon بتغيير ذلك. لا يعمل التشغيل عند تسجيل الدخول إلا في نسخة مثبتة من Tolon.",
    },
  },
};

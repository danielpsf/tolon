//! The settings contract: defaults, validation codes, all-or-nothing saves and persistence.

use std::sync::Mutex;

use serde_json::json;

use super::*;
use crate::errors::command_error;

type TestResult = anyhow::Result<()>;

fn db() -> anyhow::Result<Db> {
    Ok(Mutex::new(Store::open_in_memory()?))
}

/// The `invalid:` code `apply` gives `patch`, or `None` when it is saved.
fn refused(db: &Db, patch: Value) -> Option<&'static str> {
    apply(db, &patch).err().map(|e| command_error(&e).code)
}

fn defaults() -> SettingsDto {
    SettingsDto {
        lead_minutes: 2,
        again_at_start: false,
        sound: SoundChoice::Bell,
        sound_repeat: true,
        quiet: QuietDto {
            enabled: false,
            start: "18:00".into(),
            end: "09:00".into(),
            weekends: true,
        },
        ignore: IgnoreDto {
            all_day: true,
            declined: true,
            cancelled: true,
            no_attendees: false,
            keywords: vec!["Focus".into(), "Lunch".into(), "OOO".into()],
        },
        appearance: Appearance::Auto,
        interrupt_color: InterruptColor::Theme,
        locale: None,
        start_at_login: false,
        start_at_login_status: None,
    }
}

#[test]
fn defaults_follow_the_contract() -> TestResult {
    assert_eq!(get(&db()?)?, defaults());
    Ok(())
}

#[test]
fn the_dto_serialises_with_the_contract_field_names() -> TestResult {
    let value = serde_json::to_value(defaults())?;
    assert_eq!(
        value,
        json!({
            "lead_minutes": 2, "again_at_start": false, "sound": "bell", "sound_repeat": true,
            "quiet": { "enabled": false, "start": "18:00", "end": "09:00", "weekends": true },
            "ignore": { "all_day": true, "declined": true, "cancelled": true, "no_attendees": false,
                        "keywords": ["Focus", "Lunch", "OOO"] },
            "appearance": "auto", "interrupt_color": "theme", "locale": null,
            "start_at_login": false,
            "start_at_login_status": null
        })
    );
    Ok(())
}

fn everything() -> Value {
    json!({
        "lead_minutes": 10, "again_at_start": true, "sound": "chime", "sound_repeat": false,
        "quiet": { "enabled": true, "start": "22:30", "end": "07:15", "weekends": false },
        "ignore": { "all_day": false, "declined": false, "cancelled": false, "no_attendees": true,
                    "keywords": [" Standup ", "", "1:1"] },
        "appearance": "dark", "interrupt_color": "moss-deep", "locale": "pt-BR",
        "start_at_login": true
    })
}

fn everything_dto() -> SettingsDto {
    SettingsDto {
        lead_minutes: 10,
        again_at_start: true,
        sound: SoundChoice::Chime,
        sound_repeat: false,
        quiet: QuietDto {
            enabled: true,
            start: "22:30".into(),
            end: "07:15".into(),
            weekends: false,
        },
        ignore: IgnoreDto {
            all_day: false,
            declined: false,
            cancelled: false,
            no_attendees: true,
            keywords: vec!["Standup".into(), "1:1".into()],
        },
        appearance: Appearance::Dark,
        interrupt_color: InterruptColor::MossDeep,
        locale: Some("pt-BR".into()),
        start_at_login: true,
        start_at_login_status: None,
    }
}

#[test]
fn ac9_1_settings_survive_restart() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("tolon.db");
    {
        let db: Db = Mutex::new(Store::open(&path)?);
        assert_eq!(apply(&db, &everything())?, everything_dto());
    }
    let db: Db = Mutex::new(Store::open(&path)?);
    assert_eq!(get(&db)?, everything_dto());
    Ok(())
}

#[test]
fn ac9_1_each_change_saves_on_its_own_and_leaves_the_rest() -> TestResult {
    let db = db()?;
    apply(&db, &json!({ "quiet": { "enabled": true } }))?;
    let after = apply(&db, &json!({ "ignore": { "all_day": false } }))?;
    let mut want = defaults();
    want.quiet.enabled = true;
    want.ignore.all_day = false;
    assert_eq!(after, want);
    assert_eq!(get(&db)?, want);
    Ok(())
}

#[test]
fn storage_keeps_the_existing_keys_and_values() -> TestResult {
    let db = db()?;
    apply(&db, &everything())?;
    let all = pipeline::lock(&db).settings().all()?;
    assert_eq!(all[LEAD_MINUTES], json!(10));
    assert_eq!(all[SOUND], json!("chime"));
    assert_eq!(all[IGNORE_KEYWORDS], json!("Standup, 1:1"));
    assert_eq!(all[IGNORE_ALL_DAY], json!(false));
    assert_eq!(all[QUIET_START], json!("22:30"));
    assert_eq!(all[LOCALE], json!("pt-BR"));
    assert_eq!(all[INTERRUPT_COLOR], json!("moss-deep"));
    apply(&db, &json!({ "sound": "bell", "locale": null }))?;
    let all = pipeline::lock(&db).settings().all()?;
    assert_eq!(
        all[SOUND],
        json!("soft_bell"),
        "the scheduler's stored name"
    );
    assert_eq!(all[LOCALE], Value::Null);
    assert_eq!(get(&db)?.locale, None);
    Ok(())
}

#[test]
fn ac9_5_lead_time_accepts_1_to_60_minutes() -> TestResult {
    let db = db()?;
    for ok in [1, 60] {
        assert_eq!(apply(&db, &json!({ "lead_minutes": ok }))?.lead_minutes, ok);
    }
    for bad in [json!(0), json!(61), json!(-1), json!(1.5), json!("5")] {
        assert_eq!(
            refused(&db, json!({ "lead_minutes": bad })),
            Some("invalid:lead_minutes"),
            "{bad}"
        );
    }
    // "Again at the start" is its own switch: it leaves the lead time alone.
    let s = apply(&db, &json!({ "again_at_start": true }))?;
    assert_eq!((s.lead_minutes, s.again_at_start), (60, true));
    Ok(())
}

#[test]
fn ac6_3_the_preview_sounds_map_to_the_bundled_ones() -> TestResult {
    for (name, sound) in [
        ("bell", Sound::SoftBell),
        ("chime", Sound::Chime),
        ("none", Sound::None),
    ] {
        let choice: SoundChoice = serde_json::from_value(json!(name))?;
        assert_eq!(Sound::from(choice), sound);
        assert_eq!(SoundChoice::from(sound), choice);
    }
    assert!(serde_json::from_value::<SoundChoice>(json!("soft_bell")).is_err());
    Ok(())
}

#[test]
fn times_must_be_hh_mm() -> TestResult {
    let db = db()?;
    for bad in [
        "9:00", "24:00", "09:60", "ab:cd", "09-00", "+9:00", "09:00 ", "", "0900",
    ] {
        assert_eq!(
            refused(&db, json!({ "quiet": { "start": bad } })),
            Some("invalid:quiet.start"),
            "{bad:?}"
        );
        assert_eq!(
            refused(&db, json!({ "quiet": { "end": bad } })),
            Some("invalid:quiet.end"),
            "{bad:?}"
        );
    }
    assert_eq!(
        refused(&db, json!({ "quiet": { "start": 900 } })),
        Some("invalid:quiet.start")
    );
    let s = apply(
        &db,
        &json!({ "quiet": { "start": "00:00", "end": "23:59" } }),
    )?;
    assert_eq!(
        (s.quiet.start.as_str(), s.quiet.end.as_str()),
        ("00:00", "23:59")
    );
    Ok(())
}

#[test]
fn keywords_are_trimmed_and_bounded() -> TestResult {
    let db = db()?;
    let fifty: Vec<String> = (0..50).map(|i| format!("k{i}")).collect();
    assert_eq!(
        apply(&db, &json!({ "ignore": { "keywords": fifty } }))?
            .ignore
            .keywords
            .len(),
        50
    );
    let mut fifty_one = fifty.clone();
    fifty_one.push("one more".into());
    let long = "x".repeat(65);
    let longest = "é".repeat(64);
    for bad in [
        json!(fifty_one),
        json!([long]),
        json!(["a,b"]),
        json!("Focus"),
        json!([1]),
    ] {
        assert_eq!(
            refused(&db, json!({ "ignore": { "keywords": bad } })),
            Some("invalid:ignore.keywords"),
            "{bad}"
        );
    }
    // 64 characters, not bytes; blanks only is an empty list, and nothing is ignored by title.
    let s = apply(&db, &json!({ "ignore": { "keywords": [longest] } }))?;
    assert_eq!(s.ignore.keywords, [longest]);
    let s = apply(&db, &json!({ "ignore": { "keywords": ["  ", ""] } }))?;
    assert!(s.ignore.keywords.is_empty());
    assert_eq!(s.rules().keywords, "");
    Ok(())
}

#[test]
fn locale_is_a_shipped_catalogue_or_null() -> TestResult {
    let db = db()?;
    for tag in LOCALES {
        assert_eq!(
            apply(&db, &json!({ "locale": tag }))?.locale.as_deref(),
            Some(tag)
        );
    }
    for bad in [
        json!("fr"),
        json!("pt"),
        json!("EN-US"),
        json!("zh-TW"),
        json!(7),
    ] {
        assert_eq!(
            refused(&db, json!({ "locale": bad })),
            Some("invalid:locale"),
            "{bad}"
        );
    }
    assert_eq!(apply(&db, &json!({ "locale": null }))?.locale, None);
    // A locale missing from the patch is left alone.
    apply(&db, &json!({ "locale": "ja" }))?;
    assert_eq!(apply(&db, &json!({}))?.locale.as_deref(), Some("ja"));
    Ok(())
}

#[test]
fn a_field_of_the_wrong_type_or_value_names_itself() -> TestResult {
    let db = db()?;
    let cases = [
        (json!({ "sound": "soft_bell" }), "invalid:sound"),
        (json!({ "sound_repeat": "yes" }), "invalid:sound_repeat"),
        (json!({ "appearance": "blue" }), "invalid:appearance"),
        (json!({ "start_at_login": 1 }), "invalid:start_at_login"),
        (
            json!({ "quiet": { "weekends": "no" } }),
            "invalid:quiet.weekends",
        ),
        (json!({ "quiet": "on" }), "invalid:quiet"),
        (
            json!({ "ignore": { "declined": 0 } }),
            "invalid:ignore.declined",
        ),
        (json!({ "theme": "dark" }), "invalid:patch"),
        (json!({ "quiet": { "from": "18:00" } }), "invalid:patch"),
        (json!(["lead_minutes", 5]), "invalid:patch"),
        (json!(null), "invalid:patch"),
    ];
    for (patch, code) in cases {
        assert_eq!(refused(&db, patch.clone()), Some(code), "{patch}");
    }
    Ok(())
}

#[test]
fn a_refused_patch_saves_nothing() -> TestResult {
    let db = db()?;
    let patch = json!({ "lead_minutes": 5, "ignore": { "all_day": false, "keywords": ["a,b"] } });
    assert_eq!(refused(&db, patch), Some("invalid:ignore.keywords"));
    assert_eq!(get(&db)?, defaults());
    Ok(())
}

#[test]
fn stored_values_that_do_not_read_fall_back_to_the_defaults() -> TestResult {
    let db = db()?;
    {
        let store = pipeline::lock(&db);
        let s = store.settings();
        s.set(QUIET_START, &json!("25:00"))?;
        s.set(QUIET_END, &json!(9))?;
        s.set(APPEARANCE, &json!("blue"))?;
        s.set(INTERRUPT_COLOR, &json!("blue"))?;
        s.set(LOCALE, &json!("fr"))?;
        s.set(LEAD_MINUTES, &json!(500))?;
        s.set(QUIET_ENABLED, &json!("yes"))?;
    }
    let mut want = defaults();
    want.lead_minutes = 60;
    assert_eq!(get(&db)?, want);
    Ok(())
}

#[test]
fn quiet_hours_reach_the_core_only_while_enabled() -> TestResult {
    let db = db()?;
    assert_eq!(get(&db)?.quiet_hours(Tz::UTC), None);
    let s = apply(&db, &json!({ "quiet": { "enabled": true } }))?;
    let q = s
        .quiet_hours(Tz::Europe__Berlin)
        .ok_or_else(|| anyhow::anyhow!("quiet hours are on"))?;
    assert_eq!(
        (q.start, q.end, q.weekends, q.tz),
        (
            NaiveTime::from_hms_opt(18, 0, 0).unwrap_or_default(),
            NaiveTime::from_hms_opt(9, 0, 0).unwrap_or_default(),
            true,
            Tz::Europe__Berlin
        )
    );
    Ok(())
}

#[test]
fn nested_patches_may_be_complete_or_partial() -> TestResult {
    let db = db()?;
    // Complete, as the settings pages send them.
    let complete = json!({
        "quiet": { "enabled": true, "start": "20:00", "end": "08:00", "weekends": false },
        "ignore": { "all_day": false, "declined": true, "cancelled": true, "no_attendees": true,
                    "keywords": ["Focus"] }
    });
    let s = apply(&db, &complete)?;
    assert_eq!(
        s.quiet,
        QuietDto {
            enabled: true,
            start: "20:00".into(),
            end: "08:00".into(),
            weekends: false
        }
    );
    assert_eq!(
        s.ignore,
        IgnoreDto {
            all_day: false,
            declined: true,
            cancelled: true,
            no_attendees: true,
            keywords: vec!["Focus".into()]
        }
    );
    // Partial: one nested field changes, the others stay.
    let s = apply(
        &db,
        &json!({ "quiet": { "weekends": true }, "ignore": { "declined": false } }),
    )?;
    assert_eq!(
        (s.quiet.enabled, s.quiet.start.as_str(), s.quiet.weekends),
        (true, "20:00", true)
    );
    assert_eq!(
        (s.ignore.all_day, s.ignore.declined, s.ignore.no_attendees),
        (false, false, true)
    );
    // An empty nested object changes nothing.
    assert_eq!(apply(&db, &json!({ "quiet": {}, "ignore": {} }))?, s);
    Ok(())
}

/// AC9.4: the settings window sees the login item as the OS reports it, never the stored flag.
#[test]
fn ac9_4_the_switch_shows_what_the_os_reports() -> TestResult {
    let stored = apply(&db()?, &json!({ "start_at_login": true }))?;
    assert!(stored.start_at_login, "the preference is saved");
    for (os, on, status) in [
        (Ok(StartAtLogin::Enabled), true, "enabled"),
        (Ok(StartAtLogin::Disabled), false, "disabled"),
        (
            Ok(StartAtLogin::RequiresApproval),
            false,
            "requires_approval",
        ),
        (Err(PlatformError::Unsupported), false, "unavailable"),
    ] {
        let dto = with_login(stored.clone(), os);
        assert_eq!(
            (dto.start_at_login, dto.start_at_login_status),
            (on, Some(status))
        );
    }
    Ok(())
}

/// AC9.4: a change reaches the login item and the answer is read back from it. The stub keeps
/// one flag in memory, so no real login item is registered on the machine running the tests.
#[test]
fn ac9_4_set_login_registers_and_reads_back() -> TestResult {
    let stub = tolon_platform::StubPlatform;
    assert_eq!(set_login(&stub, true)?, StartAtLogin::Enabled);
    assert_eq!(set_login(&stub, false)?, StartAtLogin::Disabled);
    Ok(())
}

/// S7.1, AC7.3: a pause is saved with its end, survives a reopened store, ends by itself, and
/// Resume ends it at once. It is not a setting: `settings_set` cannot write it.
#[test]
fn s7_1_pause_is_saved_and_ends_on_time() -> TestResult {
    let db = db()?;
    let now = DateTime::parse_from_rfc3339("2026-10-20T13:40:00Z")?.with_timezone(&Utc);
    let until = pause(&db, 60, now)?;
    assert_eq!(until, now + Duration::hours(1));
    let store = pipeline::lock(&db);
    assert_eq!(
        store.settings().get(PAUSED_UNTIL)?,
        Some(json!("2026-10-20T14:40:00Z"))
    );
    assert_eq!(paused_until(&store, now)?, Some(until));
    assert_eq!(
        paused_until(&store, until - Duration::seconds(1))?,
        Some(until)
    );
    assert_eq!(paused_until(&store, until)?, None, "resumes automatically");
    drop(store);
    resume(&db)?;
    assert_eq!(paused_until(&pipeline::lock(&db), now)?, None);
    for minutes in [0, 61] {
        let code = pause(&db, minutes, now)
            .err()
            .map(|e| command_error(&e).code);
        assert_eq!(code, Some("invalid:minutes"), "{minutes}");
    }
    assert_eq!(
        refused(&db, json!({ "paused_until": "2099-01-01T00:00:00Z" })),
        Some("invalid:patch")
    );
    Ok(())
}

/// L-043: the eight reminder colours of the decision, stored under `interrupt_color`; anything else
/// is refused by name and saves nothing.
#[test]
fn interrupt_color_is_one_of_the_eight_pairs() -> TestResult {
    let db = db()?;
    for name in [
        "theme",
        "moss",
        "moss-deep",
        "ink",
        "paper",
        "info",
        "signal",
        "danger",
    ] {
        let s = apply(&db, &json!({ "interrupt_color": name }))?;
        assert_eq!(serde_json::to_value(s.interrupt_color)?, json!(name));
        let stored = pipeline::lock(&db).settings().get(INTERRUPT_COLOR)?;
        assert_eq!(stored, Some(json!(name)));
    }
    for bad in [
        json!("Moss"),
        json!("moss_deep"),
        json!("#4A7A52"),
        json!("red"),
        json!(""),
        json!(3),
        json!({ "bg": "moss" }),
    ] {
        let patch = json!({ "lead_minutes": 9, "interrupt_color": bad });
        assert_eq!(
            refused(&db, patch),
            Some("invalid:interrupt_color"),
            "{bad}"
        );
    }
    let s = get(&db)?;
    assert_eq!(
        (s.interrupt_color, s.lead_minutes),
        (InterruptColor::Danger, 2),
        "a refused colour saves nothing"
    );
    // A null plain field means "unchanged" (decision "Quiet hours and settings details").
    let s = apply(&db, &json!({ "interrupt_color": null }))?;
    assert_eq!(s.interrupt_color, InterruptColor::Danger);
    Ok(())
}

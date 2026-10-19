//! `TZID` parameter to IANA zone.

use chrono_tz::Tz;

/// Windows zone names as Outlook writes them, mapped to the CLDR default IANA zone.
/// shortcut: common zones only; a name missing here reads as floating time, add the row when a feed needs it.
const WINDOWS: &[(&str, &str)] = &[
    ("UTC", "UTC"),
    ("GMT Standard Time", "Europe/London"),
    ("Greenwich Standard Time", "Atlantic/Reykjavik"),
    ("W. Europe Standard Time", "Europe/Berlin"),
    ("Central Europe Standard Time", "Europe/Budapest"),
    ("Central European Standard Time", "Europe/Warsaw"),
    ("Romance Standard Time", "Europe/Paris"),
    ("E. Europe Standard Time", "Europe/Chisinau"),
    ("GTB Standard Time", "Europe/Bucharest"),
    ("FLE Standard Time", "Europe/Kiev"),
    ("Russian Standard Time", "Europe/Moscow"),
    ("Turkey Standard Time", "Europe/Istanbul"),
    ("Israel Standard Time", "Asia/Jerusalem"),
    ("Egypt Standard Time", "Africa/Cairo"),
    ("South Africa Standard Time", "Africa/Johannesburg"),
    ("Arab Standard Time", "Asia/Riyadh"),
    ("India Standard Time", "Asia/Kolkata"),
    ("China Standard Time", "Asia/Shanghai"),
    ("Singapore Standard Time", "Asia/Singapore"),
    ("Tokyo Standard Time", "Asia/Tokyo"),
    ("Korea Standard Time", "Asia/Seoul"),
    ("W. Australia Standard Time", "Australia/Perth"),
    ("AUS Eastern Standard Time", "Australia/Sydney"),
    ("New Zealand Standard Time", "Pacific/Auckland"),
    ("Hawaiian Standard Time", "Pacific/Honolulu"),
    ("Alaskan Standard Time", "America/Anchorage"),
    ("Pacific Standard Time", "America/Los_Angeles"),
    ("US Mountain Standard Time", "America/Phoenix"),
    ("Mountain Standard Time", "America/Denver"),
    ("Central Standard Time", "America/Chicago"),
    ("Canada Central Standard Time", "America/Regina"),
    ("Mexico Standard Time", "America/Mexico_City"),
    ("Eastern Standard Time", "America/New_York"),
    ("SA Pacific Standard Time", "America/Bogota"),
    ("Atlantic Standard Time", "America/Halifax"),
    ("Newfoundland Standard Time", "America/St_Johns"),
    ("E. South America Standard Time", "America/Sao_Paulo"),
    ("Argentina Standard Time", "America/Buenos_Aires"),
];

/// IANA name, a path-prefixed IANA name (`/mozilla.org/20050126_1/Europe/Berlin`), or a Windows name.
pub fn lookup(tzid: &str) -> Option<Tz> {
    let tzid = tzid.trim();
    if let Ok(tz) = tzid.parse::<Tz>() {
        return Some(tz);
    }
    let mut rest = tzid.trim_start_matches('/');
    while let Some((_, tail)) = rest.split_once('/') {
        if let Ok(tz) = tail.parse::<Tz>() {
            return Some(tz);
        }
        rest = tail;
    }
    WINDOWS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(tzid))
        .and_then(|(_, iana)| iana.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iana_prefixed_and_windows_names_resolve() {
        assert_eq!(lookup("Europe/Berlin"), Some(Tz::Europe__Berlin));
        assert_eq!(
            lookup("/mozilla.org/20050126_1/Europe/Berlin"),
            Some(Tz::Europe__Berlin)
        );
        assert_eq!(lookup("W. Europe Standard Time"), Some(Tz::Europe__Berlin));
        assert_eq!(lookup("Customized Time Zone"), None);
    }

    #[test]
    fn every_windows_row_names_a_real_zone() {
        for (name, iana) in WINDOWS {
            assert!(iana.parse::<Tz>().is_ok(), "{name} -> {iana}");
        }
    }
}

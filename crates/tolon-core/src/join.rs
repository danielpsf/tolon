//! Join-link detection (F5, AC5.1, AC5.2). Pure: the same event always gives the same target.

use crate::model::{Event, JoinTarget};

/// Hosts whose links count in `description`. A host matches itself and every subdomain.
const PROVIDER_HOSTS: &[&str] = &[
    "meet.google.com",
    "zoom.us",
    "zoomgov.com",
    "teams.microsoft.com",
    "teams.live.com",
    "webex.com",
    "meet.jit.si",
];

/// Where to join an event, in the AC5.1 order: the provider's conference data, then the first
/// link in `location` (any `https://` host counts there), then the first provider link in
/// `description`. With no link, the event page if the source has one (AC5.2), else `None`.
/// Only `https://` links are ever returned.
pub fn detect_join(event: &Event) -> JoinTarget {
    if let Some(link) = event
        .conference
        .as_deref()
        .and_then(|t| links_in(t).into_iter().next())
    {
        return target(link, true);
    }
    if let Some(link) = event
        .location
        .as_deref()
        .and_then(|t| links_in(t).into_iter().next())
    {
        return target(link.clone(), is_provider(&link));
    }
    if let Some(link) = event
        .description
        .as_deref()
        .and_then(|t| links_in(t).into_iter().find(|l| is_provider(l)))
    {
        return target(link, true);
    }
    match event.html_link.as_deref().map(str::trim) {
        Some(page) if !page.is_empty() => JoinTarget::EventPage(page.to_string()),
        _ => JoinTarget::None,
    }
}

/// `link` has passed `host_of`, so the host is always there.
fn target(url: String, recognised: bool) -> JoinTarget {
    let host = host_of(&url).unwrap_or_default();
    JoinTarget::Link {
        url,
        host,
        recognised,
    }
}

fn is_https(s: &str) -> bool {
    s.len() > "https://".len()
        && s.get(..8)
            .is_some_and(|p| p.eq_ignore_ascii_case("https://"))
}

/// Every `https://` link in `text`, in order. Understands plain text, `<https://...>`,
/// Outlook's `Join Microsoft Teams Meeting <https://...>` and HTML (`href="..."`, `&amp;`).
/// A link is a whole token between whitespace, quotes and angle brackets, so
/// `http://x/?u=https://y` yields nothing.
fn links_in(text: &str) -> Vec<String> {
    let text = ["&quot;", "&lt;", "&gt;", "&#39;", "&apos;"]
        .iter()
        .fold(capped(text).to_string(), |t, e| t.replace(e, " "))
        .replace("&amp;", "&");
    text.split(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | '`'))
        .map(|tok| tok.trim_start_matches(['(', '[', '{', '*']))
        .filter(|tok| is_https(tok))
        .map(trim_end)
        .filter(|l| is_https(l) && !l.chars().any(unsafe_char) && host_of(l).is_some())
        .map(str::to_string)
        .collect()
}

/// Drops sentence punctuation after a link, and a closing bracket only when the link has no
/// opener for it, so `https://x/a_(b)` keeps its `)` and `(see https://x/a).` loses it.
fn trim_end(mut s: &str) -> &str {
    const PAIRS: [(char, char); 3] = [('(', ')'), ('[', ']'), ('{', '}')];
    // Counted once and updated as we trim, so the whole pass is linear.
    let open = PAIRS.map(|(o, _)| s.matches(o).count());
    let mut close = PAIRS.map(|(_, c)| s.matches(c).count());
    while let Some(last) = s.chars().next_back() {
        if let Some(i) = PAIRS.iter().position(|&(_, c)| c == last) {
            if open[i] >= close[i] {
                break;
            }
            close[i] -= 1;
        } else if !matches!(last, '.' | ',' | ';' | ':' | '!' | '?' | '*') {
            break;
        }
        s = &s[..s.len() - last.len_utf8()];
    }
    s
}

/// shortcut: only the first 8 KB of a field is scanned, so a hostile feed cannot make a
/// join lookup expensive; raise it if real descriptions put the join link further down.
const MAX_SCAN: usize = 8 * 1024;

/// `text` cut to `MAX_SCAN` bytes, dropping a link the cut would split in half.
fn capped(text: &str) -> &str {
    if text.len() <= MAX_SCAN {
        return text;
    }
    let mut n = MAX_SCAN;
    while !text.is_char_boundary(n) {
        n -= 1;
    }
    let head = &text[..n];
    head.rfind(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | '`'))
        .map_or("", |i| &head[..i])
}

/// Control and invisible format characters: never part of a link we hand to a browser.
fn unsafe_char(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{ad}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{feff}')
}

/// The lower-cased host of an `https://` link, without port or trailing dot. Only
/// `[a-z0-9.-]` hosts with an optional digits-only port pass, so user info
/// (`meet.google.com@evil.example`), a backslash (`evil.example\.zoom.us`, which browsers read
/// as a path) and `evil.example:80.zoom.us` are all refused. Punycode stays as `xn--`.
fn host_of(link: &str) -> Option<String> {
    let authority = link.get(8..)?.split(['/', '?', '#']).next()?;
    let host = match authority.rsplit_once(':') {
        Some((h, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => h,
        _ => authority,
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let ok = host
        .split('.')
        .all(|l| !l.is_empty() && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'));
    ok.then_some(host)
}

fn is_provider(link: &str) -> bool {
    host_of(link).is_some_and(|h| {
        PROVIDER_HOSTS
            .iter()
            .any(|p| h == *p || h.strip_suffix(p).is_some_and(|s| s.ends_with('.')))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CalendarId, EventId, EventStatus, EventTime};
    use chrono::{DateTime, NaiveDate, Utc};

    fn event() -> Event {
        let t = EventTime::Date {
            date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap_or_default(),
        };
        Event {
            id: EventId("e".into()),
            calendar: CalendarId("c".into()),
            uid: "u".into(),
            title: "t".into(),
            start: t.clone(),
            end: t,
            all_day: false,
            rrule: None,
            exdates: vec![],
            overrides: vec![],
            status: EventStatus::Confirmed,
            my_response: None,
            attendees_count: 0,
            location: None,
            description: None,
            conference: None,
            html_link: None,
            updated: DateTime::<Utc>::UNIX_EPOCH,
        }
    }

    fn in_field(field: &str, text: &str) -> JoinTarget {
        let mut e = event();
        match field {
            "location" => e.location = Some(text.into()),
            _ => e.description = Some(text.into()),
        }
        detect_join(&e)
    }

    fn link(s: &str) -> JoinTarget {
        target(s.into(), is_provider(s))
    }

    #[test]
    fn s5_1_recognised_links() {
        for (field, url) in [
            ("location", "https://meet.google.com/abc-defg-hij"),
            ("description", "https://us02web.zoom.us/j/123456789?pwd=x"),
            (
                "location",
                "https://teams.microsoft.com/l/meetup-join/19%3Ameeting",
            ),
            ("description", "https://company.webex.com/meet/ana"),
        ] {
            assert_eq!(in_field(field, url), link(url), "{field}: {url}");
        }
    }

    #[test]
    fn s5_1_other_hosts_and_formats() {
        for url in [
            "https://teams.live.com/meet/9300",
            "https://meet.jit.si/Standup",
            "https://www.zoomgov.com/j/1",
            "https://zoom.us/j/1?pwd=a&uname=b",
        ] {
            assert_eq!(in_field("description", url), link(url));
        }
        assert_eq!(
            in_field(
                "description",
                "Join Microsoft Teams Meeting <https://teams.microsoft.com/l/meetup-join/x> Learn more"
            ),
            link("https://teams.microsoft.com/l/meetup-join/x")
        );
        assert_eq!(
            in_field(
                "description",
                r#"<p>Join: <a href="https://zoom.us/j/1?pwd=a&amp;uname=b">link</a></p>"#
            ),
            link("https://zoom.us/j/1?pwd=a&uname=b")
        );
        assert_eq!(
            in_field(
                "description",
                "Join with Google Meet: https://meet.google.com/abc-defg-hij\nOr dial: (US) +1 555\nMore phone numbers: https://tel.meet/abc-defg-hij?pin=1"
            ),
            link("https://meet.google.com/abc-defg-hij")
        );
        assert_eq!(
            in_field("location", "Room 4, https://example.com/room"),
            link("https://example.com/room")
        );
    }

    #[test]
    fn s5_2_no_link_gives_none() {
        let mut e = event();
        assert_eq!(detect_join(&e), JoinTarget::None);
        e.location = Some("Room 4.02, Example HQ".into());
        e.description = Some("Agenda: see the doc.".into());
        assert_eq!(detect_join(&e), JoinTarget::None);
    }

    #[test]
    fn ac5_2_event_page_fallback() {
        let mut e = event();
        e.html_link = Some("https://calendar.example.com/event?eid=1".into());
        assert_eq!(
            detect_join(&e),
            JoinTarget::EventPage("https://calendar.example.com/event?eid=1".into())
        );
        e.description = Some("https://meet.jit.si/x".into());
        assert_eq!(detect_join(&e), link("https://meet.jit.si/x"));
    }

    #[test]
    fn ac5_1_conference_beats_location_beats_description() {
        let mut e = event();
        e.conference = Some("https://meet.google.com/conf".into());
        e.location = Some("https://zoom.us/j/loc".into());
        e.description = Some("https://meet.jit.si/desc".into());
        assert_eq!(detect_join(&e), link("https://meet.google.com/conf"));
        e.conference = None;
        assert_eq!(detect_join(&e), link("https://zoom.us/j/loc"));
        e.location = Some("Room 4".into());
        assert_eq!(detect_join(&e), link("https://meet.jit.si/desc"));
    }

    #[test]
    fn hostile_inputs_are_refused() {
        for text in [
            "http://meet.google.com/abc",
            "javascript:alert(1)//https.zoom.us",
            "data:text/html,<a href=https://zoom.us/j/1>",
            "https://meet.google.com.evil.example/abc",
            "https://zoom.us@evil.example/j/1",
            "https://evilzoom.us/j/1",
            "see http://x/?u=https://zoom.us/j/1",
            r"https://evil.example\.zoom.us/j/1",
            "https://evil.example:80.zoom.us/j/1",
            "https://zoom.us\u{200b}/j/1",
            "https://zoom.us/j/1\u{0}",
        ] {
            assert_eq!(in_field("description", text), JoinTarget::None, "{text}");
        }
        let mut e = event();
        e.conference = Some("http://meet.google.com/abc".into());
        e.html_link = Some("https://calendar.example.com/e".into());
        assert_eq!(
            detect_join(&e),
            JoinTarget::EventPage("https://calendar.example.com/e".into())
        );
        for text in [
            r"https://evil.example\.zoom.us/j/1",
            "https://evil.example:80.zoom.us/j/1",
        ] {
            assert_eq!(in_field("location", text), JoinTarget::None, "{text}");
        }
        assert_eq!(
            in_field("location", "javascript:alert(1)"),
            JoinTarget::None
        );
        assert_eq!(in_field("location", "http://zoom.us/j/1"), JoinTarget::None);
    }

    #[test]
    fn trailing_punctuation_is_not_part_of_the_link() {
        assert_eq!(
            in_field("description", "(join at https://zoom.us/j/1?pwd=x)."),
            link("https://zoom.us/j/1?pwd=x")
        );
        assert_eq!(
            in_field("description", "Link: https://meet.jit.si/Room_(a), thanks!"),
            link("https://meet.jit.si/Room_(a)")
        );
    }

    #[test]
    fn hostile_input_is_fast() {
        let fast = std::time::Duration::from_millis(100);
        for tail in [")".repeat(200_000), ").".repeat(100_000)] {
            let text = format!("https://zoom.us/j/1{tail}");
            let t = std::time::Instant::now();
            assert_eq!(trim_end(&text), "https://zoom.us/j/1");
            for field in ["location", "description"] {
                assert_eq!(in_field(field, &text), JoinTarget::None, "over the cap");
            }
            assert!(t.elapsed() < fast, "{:?}", t.elapsed());
        }
        let text = format!("https://zoom.us/j/1{}", ")".repeat(MAX_SCAN - 100));
        let t = std::time::Instant::now();
        assert_eq!(in_field("description", &text), link("https://zoom.us/j/1"));
        assert!(t.elapsed() < fast, "{:?}", t.elapsed());
    }

    #[test]
    fn a_link_past_the_scan_cap_is_ignored() {
        let text = format!("{} https://zoom.us/j/1", "x ".repeat(MAX_SCAN));
        assert_eq!(in_field("description", &text), JoinTarget::None);
        let cut = format!("{} https://zoom.us/j/1", "x ".repeat(MAX_SCAN / 2 - 8));
        assert_eq!(in_field("description", &cut), JoinTarget::None);
    }

    #[test]
    fn conference_data_is_sanitised_like_any_link() {
        for c in [
            "https://meet.google.com@evil.example/x",
            "https://zoom.us/j/1\u{0}x",
            "https://zoom.us/j/1\u{200b}",
            r"https://evil.example\.zoom.us/j/1",
            "http://meet.google.com/x",
        ] {
            let mut e = event();
            e.conference = Some(c.into());
            assert_eq!(detect_join(&e), JoinTarget::None, "{c:?}");
        }
        let mut e = event();
        e.conference = Some("https://zoom.us/j/1 https://evil.example".into());
        assert_eq!(detect_join(&e), link("https://zoom.us/j/1"));
        e.conference =
            Some("https://meet.google.com@evil.example/x\nhttps://meet.google.com/ok".into());
        assert_eq!(detect_join(&e), link("https://meet.google.com/ok"));
    }

    #[test]
    fn an_http_location_falls_through_to_a_zoom_description() {
        let mut e = event();
        e.location = Some("http://zoom.us/j/1".into());
        e.description = Some("https://zoom.us/j/2".into());
        assert_eq!(detect_join(&e), link("https://zoom.us/j/2"));
    }

    #[test]
    fn the_link_carries_its_host_and_whether_it_was_recognised() {
        assert_eq!(
            in_field("location", "https://Room.Example.com:8443/x"),
            JoinTarget::Link {
                url: "https://Room.Example.com:8443/x".into(),
                host: "room.example.com".into(),
                recognised: false
            }
        );
        assert_eq!(
            in_field("description", "https://us02web.zoom.us/j/1"),
            JoinTarget::Link {
                url: "https://us02web.zoom.us/j/1".into(),
                host: "us02web.zoom.us".into(),
                recognised: true
            }
        );
        let mut e = event();
        e.conference = Some("https://xn--mnich-kva.example/x".into());
        assert_eq!(
            detect_join(&e),
            JoinTarget::Link {
                url: "https://xn--mnich-kva.example/x".into(),
                host: "xn--mnich-kva.example".into(),
                recognised: true
            }
        );
    }
}

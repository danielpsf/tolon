//! A local mock of Google's OAuth and Calendar endpoints. No test reaches Google.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use oauth2::{PkceCodeChallenge, PkceCodeVerifier};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tolon_core::{
    judge_window, CalendarId, EventStatus, EventTime, ExpandOptions, IgnoreRules, Response,
};

use super::map::{merge, Item};
use super::*;

const CODE: &str = "code-SECRET-4711";
const REFRESH: &str = "refresh-SECRET-1";
const ACCESS: &str = "access-SECRET-1";
const EMAIL: &str = "owner-SECRET@example.com";

#[derive(Debug, Clone)]
struct Req {
    method: String,
    path: String,
    query: HashMap<String, String>,
    form: HashMap<String, String>,
    auth: Option<String>,
}

type Shared = Arc<Mutex<HashMap<String, String>>>;
type Handler = Box<dyn Fn(&Req) -> (u16, Value) + Send + Sync>;

struct Mock {
    base: String,
    seen: Arc<Mutex<Vec<Req>>>,
}

impl Mock {
    fn seen(&self, path: &str) -> Vec<Req> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.path.ends_with(path))
            .cloned()
            .collect()
    }

    fn config(&self) -> GoogleConfig {
        GoogleConfig {
            client_id: "client-1.apps.googleusercontent.com".into(),
            client_secret: Some("not-really-secret".into()),
            auth_url: format!("{}/auth", self.base),
            token_url: format!("{}/token", self.base),
            revoke_url: format!("{}/revoke", self.base),
            api_base: format!("{}/calendar/v3", self.base),
            sign_in_wait: SIGN_IN_WAIT,
        }
    }

    fn client_waiting(&self, wait: std::time::Duration) -> GoogleClient {
        GoogleClient::new(GoogleConfig {
            sign_in_wait: wait,
            ..self.config()
        })
        .unwrap()
    }

    fn client(&self) -> GoogleClient {
        GoogleClient::new(self.config()).unwrap()
    }
}

fn pairs(s: &str) -> HashMap<String, String> {
    Url::parse(&format!("http://x/?{s}"))
        .map(|u| u.query_pairs().into_owned().collect())
        .unwrap_or_default()
}

/// Answers every request with `handler`, one connection at a time, for the life of the test.
async fn mock(handler: Handler) -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            let mut raw = Vec::new();
            let mut b = [0u8; 4096];
            let head_end = loop {
                if let Some(i) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                    break i + 4;
                }
                match sock.read(&mut b).await {
                    Ok(0) | Err(_) => break raw.len(),
                    Ok(n) => raw.extend_from_slice(&b[..n]),
                }
            };
            let head = String::from_utf8_lossy(&raw[..head_end]).into_owned();
            let header = |name: &str| {
                head.lines().find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.trim()
                        .eq_ignore_ascii_case(name)
                        .then(|| v.trim().to_owned())
                })
            };
            let len: usize = header("content-length")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            while raw.len() < head_end + len {
                match sock.read(&mut b).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => raw.extend_from_slice(&b[..n]),
                }
            }
            let mut line = head.lines().next().unwrap_or("").split(' ');
            let method = line.next().unwrap_or("").to_owned();
            let target = line.next().unwrap_or("/");
            let (path, query) = target.split_once('?').unwrap_or((target, ""));
            let req = Req {
                method,
                path: path.to_owned(),
                query: pairs(query),
                form: pairs(&String::from_utf8_lossy(&raw[head_end.min(raw.len())..])),
                auth: header("authorization"),
            };
            let (status, body) = handler(&req);
            log.lock().unwrap().push(req);
            let body = body.to_string();
            let resp = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
        }
    });
    Mock { base, seen }
}

fn token_ok() -> (u16, Value) {
    (
        200,
        json!({"access_token": ACCESS, "refresh_token": REFRESH, "expires_in": 3600, "token_type": "Bearer"}),
    )
}

fn calendar_list() -> (u16, Value) {
    (
        200,
        json!({"items": [
            {"id": EMAIL, "summary": EMAIL, "primary": true, "accessRole": "owner"},
            {"id": "team@group.calendar.google.com", "summary": "Team", "summaryOverride": "My team", "accessRole": "reader"},
            {"id": "busy@group.calendar.google.com", "summary": "Busy", "accessRole": "freeBusyReader"}
        ]}),
    )
}

/// Plays Google's part after the consent page: hits the loopback redirect, first with noise
/// (a favicon and a forged state) that must be ignored. Returns the auth URL's query.
fn browser(params: &'static [(&'static str, &'static str)]) -> (impl FnOnce(&str) -> bool, Shared) {
    let auth = Arc::new(Mutex::new(HashMap::new()));
    let keep = auth.clone();
    let open = move |url: &str| {
        let url = Url::parse(url).unwrap();
        let q: HashMap<String, String> = url.query_pairs().into_owned().collect();
        *keep.lock().unwrap() = q.clone();
        let keep_page = keep.clone();
        tokio::spawn(async move {
            let redirect = q["redirect_uri"].clone();
            // A preconnect that never sends a request: it must not hold up the redirect.
            let _idle = tokio::net::TcpStream::connect(redirect.trim_start_matches("http://"))
                .await
                .unwrap();
            let http = reqwest::Client::new();
            let _ = http.get(format!("{redirect}/favicon.ico")).send().await;
            let _ = http
                .get(format!("{redirect}/?state=forged&code=evil"))
                .send()
                .await;
            let mut back = Url::parse(&redirect).unwrap();
            back.query_pairs_mut().append_pair("state", &q["state"]);
            for (k, v) in params {
                back.query_pairs_mut().append_pair(k, v);
            }
            let page = http.get(back).send().await.unwrap().text().await.unwrap();
            keep_page.lock().unwrap().insert(PAGE_KEY.into(), page);
        });
        true
    };
    (open, auth)
}

const PAGE_KEY: &str = "<page>";

/// The page the browser was shown, once it arrived.
async fn shown_page(auth: &Shared) -> String {
    for _ in 0..100 {
        if let Some(page) = auth.lock().unwrap().get(PAGE_KEY) {
            return page.clone();
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("the browser never got a page");
}

const PAGE: DonePage<'static> = DonePage {
    title: "Signed in & done",
    body: "Go back to Tolon.",
};

#[tokio::test]
async fn ac13_1_pkce_s256_loopback_redirect_and_code_exchange() {
    let challenge = Arc::new(Mutex::new(String::new()));
    let seen_challenge = challenge.clone();
    let m = mock(Box::new(move |r| match r.path.as_str() {
        "/token" => {
            // The verifier must hash (S256) to the challenge the auth URL carried.
            let verifier = PkceCodeVerifier::new(r.form["code_verifier"].clone());
            let expected = PkceCodeChallenge::from_code_verifier_sha256(&verifier);
            assert_eq!(expected.as_str(), seen_challenge.lock().unwrap().as_str());
            assert_eq!(r.form["grant_type"], "authorization_code");
            assert_eq!(r.form["code"], CODE);
            assert!(r.form["redirect_uri"].starts_with("http://127.0.0.1:"));
            assert_eq!(r.form["client_id"], "client-1.apps.googleusercontent.com");
            token_ok()
        }
        "/calendar/v3/users/me/calendarList" => {
            assert_eq!(r.auth.as_deref(), Some(format!("Bearer {ACCESS}").as_str()));
            calendar_list()
        }
        _ => (404, json!({})),
    }))
    .await;
    let (open, auth) = browser(&[("code", CODE)]);
    let challenge_in = challenge.clone();
    let open = move |url: &str| {
        let q: HashMap<String, String> = Url::parse(url)
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect();
        *challenge_in.lock().unwrap() = q["code_challenge"].clone();
        open(url)
    };
    let started = std::time::Instant::now();
    let signed = m.client().sign_in(open, &PAGE).await.unwrap();
    // The idle preconnect did not make the redirect wait for READ_WAIT.
    assert!(started.elapsed() < READ_WAIT / 2, "{:?}", started.elapsed());
    assert!(shown_page(&auth)
        .await
        .contains("<h1>Signed in &amp; done</h1>"));

    let q = auth.lock().unwrap().clone();
    assert_eq!(q["code_challenge_method"], "S256");
    assert_eq!(q["response_type"], "code");
    assert!(q["redirect_uri"].starts_with("http://127.0.0.1:"));
    assert!(q["state"].len() >= 16);
    assert_eq!(signed.account, EMAIL);
    assert_eq!(signed.refresh_token, REFRESH);
    assert_eq!(
        signed.calendars,
        vec![
            RemoteCalendar {
                id: EMAIL.into(),
                name: EMAIL.into(),
                primary: true
            },
            RemoteCalendar {
                id: "team@group.calendar.google.com".into(),
                name: "My team".into(),
                primary: false
            },
        ]
    );
    assert_eq!(m.seen("/token").len(), 1);
}

#[tokio::test]
async fn ac13_2_auth_url_asks_for_calendar_readonly_only() {
    let m = mock(Box::new(|r| match r.path.as_str() {
        "/token" => token_ok(),
        _ => calendar_list(),
    }))
    .await;
    let (open, auth) = browser(&[("code", CODE)]);
    m.client().sign_in(open, &PAGE).await.unwrap();
    let q = auth.lock().unwrap().clone();
    assert_eq!(
        q["scope"],
        "https://www.googleapis.com/auth/calendar.readonly"
    );
    assert_eq!(SCOPE, q["scope"]);
}

#[tokio::test]
async fn ac13_1_refused_consent_is_denied_and_exchanges_nothing() {
    let m = mock(Box::new(|_| token_ok())).await;
    let (open, auth) = browser(&[("error", "access_denied")]);
    assert_eq!(
        m.client().sign_in(open, &PAGE).await.unwrap_err(),
        GoogleError::Denied
    );
    // No "signed in" page for a refusal.
    assert!(!shown_page(&auth).await.contains("Signed in"));
    assert!(m.seen("/token").is_empty());
}

#[tokio::test]
async fn ac13_1_a_browser_that_cannot_open_ends_the_sign_in() {
    let m = mock(Box::new(|_| token_ok())).await;
    assert_eq!(
        m.client().sign_in(|_| false, &PAGE).await.unwrap_err(),
        GoogleError::Browser
    );
}

/// An `open` that keeps the loopback address of the consent URL and sends nobody back.
fn silent_browser() -> (impl FnOnce(&str) -> bool, Arc<Mutex<String>>) {
    let addr = Arc::new(Mutex::new(String::new()));
    let keep = addr.clone();
    let open = move |url: &str| {
        let q: HashMap<String, String> = Url::parse(url)
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect();
        *keep.lock().unwrap() = q["redirect_uri"].trim_start_matches("http://").to_owned();
        true
    };
    (open, addr)
}

#[tokio::test]
async fn ac13_1_a_browser_that_never_comes_back_times_out_and_frees_the_port() {
    let m = mock(Box::new(|_| token_ok())).await;
    let (open, addr) = silent_browser();
    let started = std::time::Instant::now();
    let err = m
        .client_waiting(std::time::Duration::from_millis(200))
        .sign_in(open, &PAGE)
        .await
        .unwrap_err();
    assert_eq!(err, GoogleError::Timeout);
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert!(m.seen("/token").is_empty());
    let addr = addr.lock().unwrap().clone();
    assert!(
        TcpStream::connect(&addr).await.is_err(),
        "the port is closed"
    );
    // The default is five minutes (AC13.1).
    assert_eq!(SIGN_IN_WAIT, std::time::Duration::from_secs(300));
    assert_eq!(m.config().sign_in_wait, SIGN_IN_WAIT);
}

#[tokio::test]
async fn ac13_1_cancelling_a_waiting_sign_in_frees_the_loopback_port() {
    let m = mock(Box::new(|_| token_ok())).await;
    let (open, addr) = silent_browser();
    let client = m.client();
    let (cancel, cancelled) = tokio::sync::oneshot::channel::<()>();
    let waiting = async {
        // The same race as `calendars_google_sign_in`: the sign-in against a cancel signal.
        tokio::select! {
            r = client.sign_in(open, &PAGE) => r.map(|_| ()),
            _ = cancelled => Err(GoogleError::Denied),
        }
    };
    let (result, ()) = tokio::join!(waiting, async {
        while addr.lock().unwrap().is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let a = addr.lock().unwrap().clone();
        // Waiting: the port accepts.
        assert!(TcpStream::connect(&a).await.is_ok());
        cancel.send(()).unwrap();
    });
    assert_eq!(result, Err(GoogleError::Denied));
    let a = addr.lock().unwrap().clone();
    assert!(TcpStream::connect(&a).await.is_err(), "the port is closed");
    assert!(m.seen("/token").is_empty());
}

#[tokio::test]
async fn a_flood_of_silent_connections_is_capped_and_extras_are_dropped_at_once() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let waiting =
        tokio::spawn(async move { wait_for_code(&listener, "state-1234567890", &PAGE).await });
    let mut idle = Vec::new();
    for _ in 0..MAX_CONNECTIONS {
        idle.push(TcpStream::connect(addr).await.unwrap());
    }
    // The next connection is accepted and closed without a read: the client sees the end of the
    // stream long before READ_WAIT.
    let mut extra = TcpStream::connect(addr).await.unwrap();
    let mut byte = [0u8; 1];
    let end = tokio::time::timeout(READ_WAIT / 5, extra.read(&mut byte))
        .await
        .expect("the extra connection was held open");
    assert!(matches!(end, Ok(0) | Err(_)), "{end:?}");

    // The idle ones give their room back when they go; the redirect then goes through.
    drop(idle);
    let request = "GET /?state=state-1234567890&code=the-code HTTP/1.1\r\nHost: x\r\n\r\n";
    let mut answered = false;
    for _ in 0..100 {
        let mut s = TcpStream::connect(addr).await.unwrap();
        let _ = s.write_all(request.as_bytes()).await;
        let mut got = Vec::new();
        let _ = s.read_to_end(&mut got).await;
        if String::from_utf8_lossy(&got).starts_with("HTTP/1.1 200") {
            answered = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(answered, "no room came back");
    assert_eq!(waiting.await.unwrap(), Ok("the-code".to_owned()));
}

#[test]
fn the_state_is_compared_in_full_with_no_early_exit() {
    assert!(same(b"state-1234567890", b"state-1234567890"));
    assert!(same(b"", b""));
    // A difference in the first byte, the last byte, or the length.
    assert!(!same(b"Xtate-1234567890", b"state-1234567890"));
    assert!(!same(b"state-123456789X", b"state-1234567890"));
    assert!(!same(b"state-123456789", b"state-1234567890"));
    assert!(!same(b"state-12345678901", b"state-1234567890"));
    assert!(!same(b"", b"s"));

    let ok = redirect_result("GET /?state=s1&code=c HTTP/1.1\r\n\r\n", "s1");
    assert_eq!(ok, Some(Ok("c".to_owned())));
    for wrong in ["s2", "s", "s1x", ""] {
        let head = format!("GET /?state={wrong}&code=c HTTP/1.1\r\n\r\n");
        assert_eq!(redirect_result(&head, "s1"), None, "{wrong}");
    }
    assert_eq!(redirect_result("GET /?code=c HTTP/1.1\r\n\r\n", "s1"), None);
}

#[test]
fn debug_output_omits_the_address_titles_and_the_sync_token() {
    let cal = RemoteCalendar {
        id: EMAIL.into(),
        name: "Salary talks SECRET".into(),
        primary: true,
    };
    let listed = json!([event("id-SECRET", "Salary talk SECRET")]);
    let (events, _) = merge(Vec::new(), items(listed.clone()), &CalendarId("c".into()));
    assert_eq!(events[0].title, "Salary talk SECRET");
    let synced = Synced {
        events,
        sync_token: "sync-SECRET".into(),
        skipped: 2,
    };
    let item = items(json!([{
        "id": "id-SECRET", "summary": "Salary talk SECRET", "description": "d-SECRET",
        "location": "l-SECRET", "htmlLink": "https://x.example/SECRET", "status": "confirmed"
    }]));
    let text = format!("{cal:?} {synced:?} {:?}", item[0]);
    assert!(!text.contains("SECRET"), "{text}");
    assert!(text.contains("RemoteCalendar") && text.contains("Synced") && text.contains("Item"));
    assert!(text.contains("skipped: 2"), "{text}");
}

fn event(id: &str, summary: &str) -> Value {
    json!({"id": id, "status": "confirmed", "summary": summary,
           "start": {"dateTime": "2026-10-22T10:00:00+02:00", "timeZone": "Europe/Berlin"},
           "end": {"dateTime": "2026-10-22T10:30:00+02:00", "timeZone": "Europe/Berlin"}})
}

/// Events endpoint: full listing in two pages (token `s1`), `s1` is expired (410), the full
/// listing again gives `s2`, and the delta after `s2` deletes `b`.
fn events_api(r: &Req) -> (u16, Value) {
    match (
        r.query.get("syncToken").map(String::as_str),
        r.query.get("pageToken").map(String::as_str),
    ) {
        (None, None) => (
            200,
            json!({"items": [event("a", "A")], "nextPageToken": "p2"}),
        ),
        (None, Some("p2")) => (
            200,
            json!({"items": [event("b", "B")], "nextSyncToken": "s1"}),
        ),
        (Some("s1"), _) => (
            410,
            json!({"error": {"code": 410, "message": "Sync token is no longer valid"}}),
        ),
        (Some("s2"), _) => (
            200,
            json!({"items": [{"id": "b", "status": "cancelled"}, event("c", "C")], "nextSyncToken": "s3"}),
        ),
        _ => (400, json!({})),
    }
}

fn uids(events: &[Event]) -> Vec<&str> {
    events.iter().map(|e| e.uid.as_str()).collect()
}

#[tokio::test]
async fn ac13_4_pages_sync_token_delta_and_full_resync_on_410() {
    let m = mock(Box::new(|r| match r.path.as_str() {
        "/token" => token_ok(),
        p if p.ends_with("/events") => {
            assert_eq!(r.query["singleEvents"], "false");
            events_api(r)
        }
        _ => (404, json!({})),
    }))
    .await;
    let g = m.client();
    let cal = CalendarId("cal-1".into());
    let team = "team@group.calendar.google.com";

    let first = g.sync(REFRESH, team, None, vec![], &cal).await.unwrap();
    assert_eq!(uids(&first.events), ["a", "b"]);
    assert_eq!(first.sync_token, "s1");
    // The calendar id is one encoded path segment.
    let path = &m.seen("/events")[0].path;
    assert!(
        path == "/calendar/v3/calendars/team@group.calendar.google.com/events"
            || path == "/calendar/v3/calendars/team%40group.calendar.google.com/events",
        "{path}"
    );

    // s1 expired: the stored events are dropped and everything is listed again.
    let stale = vec![first.events[0].clone()];
    let mut second = g
        .sync(REFRESH, team, Some("s1"), stale, &cal)
        .await
        .unwrap();
    assert_eq!(uids(&second.events), ["a", "b"]);
    assert_eq!(second.sync_token, "s1");
    second.sync_token = "s2".into();

    let third = g
        .sync(REFRESH, team, Some("s2"), second.events, &cal)
        .await
        .unwrap();
    assert_eq!(uids(&third.events), ["a", "c"]);
    assert_eq!(third.sync_token, "s3");

    // Every page of a full listing starts one day back (HISTORY); a delta carries no `timeMin`
    // (Google refuses it next to a sync token), and the full listing after a 410 has it again.
    let seen = m.seen("/events");
    let time_min: Vec<bool> = seen
        .iter()
        .map(|r| r.query.contains_key("timeMin"))
        .collect();
    assert_eq!(time_min, [true, true, false, true, true, false]);
    let from = DateTime::parse_from_rfc3339(&seen[0].query["timeMin"]).unwrap();
    let back = Utc::now().signed_duration_since(from);
    assert!(
        (back - HISTORY).abs() < chrono::Duration::minutes(1),
        "{back}"
    );
    assert_eq!(seen[0].query["timeMin"], seen[1].query["timeMin"]);
}

#[tokio::test]
async fn ac13_3_refresh_token_buys_an_access_token_that_is_reused() {
    let m = mock(Box::new(|r| match r.path.as_str() {
        "/token" => {
            assert_eq!(r.form["grant_type"], "refresh_token");
            assert_eq!(r.form["refresh_token"], REFRESH);
            (
                200,
                json!({"access_token": ACCESS, "expires_in": 3600, "token_type": "Bearer"}),
            )
        }
        _ => {
            assert_eq!(r.auth.as_deref(), Some(format!("Bearer {ACCESS}").as_str()));
            calendar_list()
        }
    }))
    .await;
    let g = m.client();
    g.calendars(REFRESH).await.unwrap();
    g.calendars(REFRESH).await.unwrap();
    assert_eq!(m.seen("/token").len(), 1);
}

#[tokio::test]
async fn ac13_3_a_401_renews_the_access_token_once() {
    let calls = Arc::new(Mutex::new(0));
    let n = calls.clone();
    let m = mock(Box::new(move |r| match r.path.as_str() {
        "/token" => (
            200,
            json!({"access_token": ACCESS, "expires_in": 3600, "token_type": "Bearer"}),
        ),
        _ => {
            let mut n = n.lock().unwrap();
            *n += 1;
            if *n == 1 {
                (401, json!({}))
            } else {
                calendar_list()
            }
        }
    }))
    .await;
    m.client().calendars(REFRESH).await.unwrap();
    assert_eq!(m.seen("/token").len(), 2);
}

#[tokio::test]
async fn ac13_3_expired_refresh_token_asks_to_sign_in_again() {
    let m = mock(Box::new(|_| (400, json!({"error": "invalid_grant", "error_description": format!("Token {REFRESH} has been expired or revoked.")})))).await;
    let err = m
        .client()
        .sync(REFRESH, "primary", None, vec![], &CalendarId("c".into()))
        .await
        .unwrap_err();
    assert_eq!(err, GoogleError::Auth);
}

#[tokio::test]
async fn ac13_3_revoke_posts_the_refresh_token() {
    let m = mock(Box::new(|_| (200, json!({})))).await;
    m.client().revoke(REFRESH).await.unwrap();
    let revoke = m.seen("/revoke");
    assert_eq!(revoke.len(), 1);
    assert_eq!(revoke[0].method, "POST");
    assert_eq!(revoke[0].form["token"], REFRESH);
    // Already revoked (Google answers 400 invalid_token): nothing left to do.
    let gone = mock(Box::new(|_| (400, json!({"error": "invalid_token"})))).await;
    assert_eq!(gone.client().revoke(REFRESH).await, Ok(()));
}

#[tokio::test]
async fn ac13_3_tokens_codes_and_the_address_never_reach_logs_or_errors() {
    crate::test_log::captured();
    // A refresh that fails, an API error, a sign-in that fails after the code exchange.
    let m = mock(Box::new(|r| match r.path.as_str() {
        "/token" if r.form.get("grant_type").map(String::as_str) == Some("refresh_token") => (
            400,
            json!({"error": "invalid_grant", "error_description": format!("{REFRESH} {EMAIL}")}),
        ),
        "/token" => token_ok(),
        _ => (
            500,
            json!({"error": {"message": format!("{EMAIL} {ACCESS}")}}),
        ),
    }))
    .await;
    let g = m.client();
    let mut errors = vec![g
        .sync(REFRESH, EMAIL, Some("s"), vec![], &CalendarId("c".into()))
        .await
        .unwrap_err()];
    let (open, _) = browser(&[("code", CODE)]);
    errors.push(g.sign_in(open, &PAGE).await.unwrap_err());
    errors.push(g.calendars(REFRESH).await.unwrap_err());
    assert_eq!(
        errors,
        [
            GoogleError::Auth,
            GoogleError::Status(500),
            GoogleError::Status(500)
        ]
    );

    let signed = SignIn {
        account: EMAIL.into(),
        refresh_token: REFRESH.into(),
        calendars: vec![],
    };
    let text = format!("{} {errors:?} {signed:?}", captured_errors(&errors));
    let log = crate::test_log::captured();
    assert!(log.contains("google"), "{log}");
    for secret in [REFRESH, ACCESS, CODE, EMAIL, "SECRET"] {
        assert!(!log.contains(secret), "{secret} in log: {log}");
        assert!(!text.contains(secret), "{secret} in {text}");
    }
}

fn captured_errors(errors: &[GoogleError]) -> String {
    errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

// Mapping and merging, as `parse_ics` does for a feed.

fn items(v: Value) -> Vec<Item> {
    serde_json::from_value(v).unwrap()
}

fn cal() -> CalendarId {
    CalendarId("cal-1".into())
}

#[test]
fn ac13_5_declined_from_self_response_and_join_from_conference_data() {
    let (events, skipped) = merge(
        vec![],
        items(json!([
            {"id": "d", "summary": "Declined one",
             "start": {"dateTime": "2026-10-22T09:00:00Z"}, "end": {"dateTime": "2026-10-22T10:00:00Z"},
             "attendees": [{"email": "x@example.com", "responseStatus": "accepted"},
                           {"email": EMAIL, "self": true, "responseStatus": "declined"}],
             "hangoutLink": "https://meet.google.com/aaa-bbbb-ccc",
             "conferenceData": {"entryPoints": [
                 {"entryPointType": "phone", "uri": "tel:+1-555-0100"},
                 {"entryPointType": "video", "uri": "https://meet.google.com/abc-defg-hij"}]},
             "htmlLink": "https://www.google.com/calendar/event?eid=x"},
            {"id": "m", "summary": "Meet only",
             "start": {"dateTime": "2026-10-22T11:00:00Z"}, "end": {"dateTime": "2026-10-22T11:30:00Z"},
             "hangoutLink": "https://meet.google.com/xyz-abcd-efg"}
        ])),
        &cal(),
    );
    assert_eq!(skipped, 0);
    let d = &events[0];
    assert_eq!(d.my_response, Some(Response::Declined));
    assert_eq!(d.attendees_count, 2);
    assert_eq!(
        d.conference.as_deref(),
        Some("https://meet.google.com/abc-defg-hij")
    );
    assert_eq!(d.id.0, "cal-1:d");
    assert_eq!(
        events[1].conference.as_deref(),
        Some("https://meet.google.com/xyz-abcd-efg")
    );
    assert_eq!(events[1].my_response, None);

    // The declined rule (AC8.2) silences it like a declined ICS event.
    let from = DateTime::parse_from_rfc3339("2026-10-22T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let judged = judge_window(
        &events,
        from,
        from + chrono::Duration::days(1),
        chrono_tz::UTC,
        &ExpandOptions::default(),
        &IgnoreRules::default(),
    );
    let verdicts: Vec<_> = judged
        .iter()
        .map(|(o, v)| (o.title.as_str(), format!("{v:?}")))
        .collect();
    assert_eq!(verdicts[0].1, "Ignored(Declined)", "{verdicts:?}");
    assert_eq!(verdicts[1].1, "Keep");
}

#[test]
fn recurring_series_with_a_cancelled_and_a_moved_instance() {
    let series = json!({"id": "w", "summary": "Weekly",
        "start": {"dateTime": "2026-10-19T10:00:00+02:00", "timeZone": "Europe/Berlin"},
        "end": {"dateTime": "2026-10-19T10:30:00+02:00", "timeZone": "Europe/Berlin"},
        "recurrence": ["RRULE:FREQ=WEEKLY;BYDAY=MO"]});
    let cancelled = json!({"id": "w_20261026T080000Z", "status": "cancelled", "recurringEventId": "w",
        "originalStartTime": {"dateTime": "2026-10-26T09:00:00+01:00", "timeZone": "Europe/Berlin"}});
    let moved = json!({"id": "w_20261102T090000Z", "summary": "Weekly (moved)", "recurringEventId": "w",
        "originalStartTime": {"dateTime": "2026-11-02T10:00:00+01:00", "timeZone": "Europe/Berlin"},
        "start": {"dateTime": "2026-11-02T14:00:00+01:00", "timeZone": "Europe/Berlin"},
        "end": {"dateTime": "2026-11-02T14:30:00+01:00", "timeZone": "Europe/Berlin"}});
    // Instances before their series in the listing: still folded in.
    let (events, _) = merge(vec![], items(json!([cancelled, moved, series])), &cal());
    assert_eq!(uids(&events), ["w"]);
    let w = &events[0];
    assert_eq!(w.rrule.as_deref(), Some("FREQ=WEEKLY;BYDAY=MO"));
    assert!(matches!(w.start, EventTime::Zoned { tz, .. } if tz == chrono_tz::Europe::Berlin));
    assert_eq!(w.exdates.len(), 1);
    assert_eq!(w.overrides.len(), 1);
    assert_eq!(w.overrides[0].event.title, "Weekly (moved)");

    // A later delta renames the series only: the instances already applied stay.
    let renamed = json!({"id": "w", "summary": "Weekly sync",
        "start": {"dateTime": "2026-10-19T10:00:00+02:00", "timeZone": "Europe/Berlin"},
        "end": {"dateTime": "2026-10-19T10:30:00+02:00", "timeZone": "Europe/Berlin"},
        "recurrence": ["RRULE:FREQ=WEEKLY;BYDAY=MO"]});
    let (events, _) = merge(events, items(json!([renamed])), &cal());
    assert_eq!(events[0].title, "Weekly sync");
    assert_eq!((events[0].exdates.len(), events[0].overrides.len()), (1, 1));

    // The moved instance is restored to a normal one, then the series is deleted.
    let restored = json!({"id": "w_20261026T080000Z", "summary": "Weekly sync", "recurringEventId": "w",
        "originalStartTime": {"dateTime": "2026-10-26T09:00:00+01:00", "timeZone": "Europe/Berlin"},
        "start": {"dateTime": "2026-10-26T09:00:00+01:00", "timeZone": "Europe/Berlin"},
        "end": {"dateTime": "2026-10-26T09:30:00+01:00", "timeZone": "Europe/Berlin"}});
    let (events, _) = merge(events, items(json!([restored])), &cal());
    assert_eq!((events[0].exdates.len(), events[0].overrides.len()), (0, 2));
    let (events, _) = merge(
        events,
        items(json!([{"id": "w", "status": "cancelled"}])),
        &cal(),
    );
    assert!(events.is_empty());
}

#[test]
fn instance_without_its_series_stands_alone_and_goes_with_the_series() {
    let lone = json!({"id": "s_20261022T100000Z", "summary": "One invite", "recurringEventId": "s",
        "originalStartTime": {"dateTime": "2026-10-22T10:00:00Z"},
        "start": {"dateTime": "2026-10-22T10:00:00Z"}, "end": {"dateTime": "2026-10-22T11:00:00Z"}});
    let (events, _) = merge(vec![], items(json!([lone])), &cal());
    assert_eq!(uids(&events), ["s_20261022T100000Z"]);
    let (events, _) = merge(
        events,
        items(json!([{"id": "s", "status": "cancelled"}])),
        &cal(),
    );
    assert!(events.is_empty());
}

#[test]
fn all_day_cancelled_status_and_unreadable_items() {
    let (events, skipped) = merge(
        vec![],
        items(json!([
            {"id": "h", "summary": "Holiday", "status": "tentative", "start": {"date": "2026-12-24"}},
            {"id": "x", "summary": "No start"},
            {"id": "r", "summary": "Bad rule", "start": {"date": "2026-12-24"}, "recurrence": ["RRULE:FREQ=NEVER"]}
        ])),
        &cal(),
    );
    assert_eq!(skipped, 2);
    assert_eq!(uids(&events), ["h"]);
    assert!(events[0].all_day);
    assert_eq!(events[0].status, EventStatus::Tentative);
    assert_eq!(
        events[0].end,
        EventTime::Date {
            date: chrono::NaiveDate::from_ymd_opt(2026, 12, 25).unwrap()
        }
    );
}

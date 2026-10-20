//! Conditional ICS fetch. Every URL that reaches a log or an error goes through `redact_url`.

use std::time::Duration;

use reqwest::{header, redirect, StatusCode, Url};

const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_BODY: usize = 5 * 1024 * 1024;
const MAX_REDIRECTS: usize = 10;

// A release build cannot carry the loopback seam: `debug_assertions` is off there.
#[cfg(all(feature = "e2e-loopback", not(debug_assertions)))]
compile_error!("the e2e-loopback feature must never be built in release mode");

/// Keeps scheme and host only; path, query, userinfo and port are dropped.
pub fn redact_url(url: &str) -> String {
    match Url::parse(url.trim()) {
        Ok(u) => match u.host_str() {
            Some(host) => format!("{}://{host}", u.scheme()),
            None => "<redacted>".to_owned(),
        },
        Err(_) => "<invalid url>".to_owned(),
    }
}

/// Typed fetch failure. `Display` never contains the URL, so it is safe for the UI and logs.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FetchError {
    #[error("unsupported URL scheme")]
    UnsupportedScheme,
    #[error("invalid URL")]
    InvalidUrl,
    /// The UI shows "Sync failed: {status}".
    #[error("server answered {0}")]
    Status(u16),
    #[error("request timed out")]
    Timeout,
    #[error("could not connect")]
    Connect,
    #[error("response is larger than {limit} bytes")]
    TooLarge { limit: usize },
    #[error("response is not an iCalendar feed")]
    NotCalendar,
    #[error("network error")]
    Network,
}

impl FetchError {
    /// HTTP status when the failure was an HTTP answer.
    pub fn status(&self) -> Option<u16> {
        match self {
            FetchError::Status(s) => Some(*s),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchOutcome {
    Modified { body: String, etag: Option<String> },
    NotModified,
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FetchConfig {
    pub timeout: Duration,
    pub max_body_bytes: usize,
    /// Tests only: plain http is rejected in production.
    pub(crate) allow_http: bool,
}

impl Default for FetchConfig {
    fn default() -> Self {
        Self {
            timeout: TIMEOUT,
            max_body_bytes: MAX_BODY,
            allow_http: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct IcsFetcher {
    client: reqwest::Client,
    cfg: FetchConfig,
}

impl IcsFetcher {
    pub fn new() -> Result<Self, FetchError> {
        Self::with_config(FetchConfig::default())
    }

    pub fn with_config(cfg: FetchConfig) -> Result<Self, FetchError> {
        let allow_http = cfg.allow_http;
        // Redirects may only land on https (plain http only in tests), so a feed cannot downgrade.
        let policy = redirect::Policy::custom(move |a| {
            if follow_redirect(a.url(), a.previous().len(), allow_http) {
                a.follow()
            } else {
                a.stop()
            }
        });
        let client = reqwest::Client::builder()
            .timeout(cfg.timeout)
            .redirect(policy)
            .build()
            .map_err(|_| FetchError::Network)?;
        Ok(Self { client, cfg })
    }

    /// GET `url` with `If-None-Match: etag` when given; a 304 gives `NotModified`.
    pub async fn fetch(&self, url: &str, etag: Option<&str>) -> Result<FetchOutcome, FetchError> {
        let result = self.fetch_inner(url, etag).await;
        if let Err(e) = &result {
            tracing::warn!(
                url = %redact_url(url),
                status = e.status(),
                error = %e,
                "ics fetch failed"
            );
        }
        result
    }

    async fn fetch_inner(&self, url: &str, etag: Option<&str>) -> Result<FetchOutcome, FetchError> {
        let url = parse_url(url, self.cfg.allow_http)?;
        let mut req = self.client.get(url);
        if let Some(etag) = etag {
            req = req.header(header::IF_NONE_MATCH, etag);
        }
        let mut resp = req.send().await.map_err(map_err)?;

        let status = resp.status();
        if status == StatusCode::NOT_MODIFIED {
            return Ok(FetchOutcome::NotModified);
        }
        if !status.is_success() {
            return Err(FetchError::Status(status.as_u16()));
        }
        let limit = self.cfg.max_body_bytes;
        if resp.content_length().is_some_and(|n| n > limit as u64) {
            return Err(FetchError::TooLarge { limit });
        }
        let etag = resp
            .headers()
            .get(header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let is_calendar_type = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.trim().to_ascii_lowercase().starts_with("text/calendar"));

        // Cap enforced chunk by chunk, so an endless body is cut off early.
        let mut buf: Vec<u8> = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(map_err)? {
            if buf.len() + chunk.len() > limit {
                return Err(FetchError::TooLarge { limit });
            }
            buf.extend_from_slice(&chunk);
        }
        let body = String::from_utf8_lossy(&buf).into_owned();
        // AC1.1: text/calendar passes; other types (hosts often send text/plain) only if the body is a VCALENDAR.
        if !is_calendar_type && !body.trim_start().starts_with("BEGIN:VCALENDAR") {
            return Err(FetchError::NotCalendar);
        }
        Ok(FetchOutcome::Modified { body, etag })
    }
}

/// Redirect target must be https (http only when `http_permitted`) and the chain shorter than `MAX_REDIRECTS`.
fn follow_redirect(target: &Url, hops: usize, allow_http: bool) -> bool {
    (target.scheme() == "https" || http_permitted(target, allow_http)) && hops < MAX_REDIRECTS
}

/// Plain http is never allowed in production. Tests set `allow_http`; the `e2e-loopback` feature
/// (debug builds only) allows it for loopback hosts alone, so a feed cannot be fetched in the clear
/// from anywhere else. Redirects use the same rule.
fn http_permitted(url: &Url, allow_http: bool) -> bool {
    url.scheme() == "http" && (allow_http || loopback_http(url))
}

/// `localhost`, or an IP literal in 127.0.0.0/8 or `::1`. Anything else, including names such as
/// `127.0.0.1.example.com`, is not loopback.
#[cfg(feature = "e2e-loopback")]
fn loopback_http(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// Without the feature no http host is allowed: production carries no loopback code at all.
#[cfg(not(feature = "e2e-loopback"))]
fn loopback_http(_url: &Url) -> bool {
    false
}

/// Canonical https form of a subscription URL: `webcal://` becomes `https://`, other schemes are rejected.
/// tolon-app stores this form (S1.2).
pub fn normalize_url(raw: &str) -> Result<String, FetchError> {
    parse_url(raw, false).map(String::from)
}

fn map_err(e: reqwest::Error) -> FetchError {
    if e.is_timeout() {
        FetchError::Timeout
    } else if e.is_connect() {
        FetchError::Connect
    } else {
        FetchError::Network
    }
}

/// `webcal://` becomes `https://`; only https is accepted (http: see `http_permitted`).
fn parse_url(raw: &str, allow_http: bool) -> Result<Url, FetchError> {
    let raw = raw.trim();
    let rewritten;
    let raw = match raw.get(..9) {
        Some(p) if p.eq_ignore_ascii_case("webcal://") => {
            rewritten = format!("https://{}", &raw[9..]);
            rewritten.as_str()
        }
        _ => raw,
    };
    let url = Url::parse(raw).map_err(|_| FetchError::InvalidUrl)?;
    if url.scheme() == "https" || http_permitted(&url, allow_http) {
        Ok(url)
    } else {
        Err(FetchError::UnsupportedScheme)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    const ICS: &str = "BEGIN:VCALENDAR\r\nX-WR-CALNAME:Team\r\nEND:VCALENDAR\r\n";

    /// Serves one canned raw response per connection; returns base URL and captured request heads.
    async fn serve(responses: Vec<Vec<u8>>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        tokio::spawn(async move {
            for resp in responses {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let mut head = Vec::new();
                let mut b = [0u8; 1024];
                while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                    match sock.read(&mut b).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => head.extend_from_slice(&b[..n]),
                    }
                }
                seen2
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&head).into_owned());
                let _ = sock.write_all(&resp).await;
                let _ = sock.flush().await;
            }
        });
        (base, seen)
    }

    fn http(status: &str, headers: &str, body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    fn fetcher(timeout: Duration, max: usize) -> IcsFetcher {
        IcsFetcher::with_config(FetchConfig {
            timeout,
            max_body_bytes: max,
            allow_http: true,
        })
        .unwrap()
    }

    fn quick() -> IcsFetcher {
        fetcher(Duration::from_secs(5), MAX_BODY)
    }

    #[test]
    fn s1_2_webcal_rewritten_to_https() {
        let u = parse_url("webcal://example.com/team.ics", false).unwrap();
        assert_eq!(u.as_str(), "https://example.com/team.ics");
        let u = parse_url("WEBCAL://example.com/team.ics", false).unwrap();
        assert_eq!(u.as_str(), "https://example.com/team.ics");
    }

    #[test]
    fn s1_2_normalized_url_is_https() {
        assert_eq!(
            normalize_url("webcal://example.com/team.ics").unwrap(),
            "https://example.com/team.ics"
        );
        assert_eq!(
            normalize_url("http://example.com/a.ics"),
            Err(FetchError::UnsupportedScheme)
        );
        let msg = format!("{}", normalize_url("secret-token:abc").unwrap_err());
        assert!(!msg.contains("secret-token"));
    }

    #[test]
    fn s1_2_redirect_downgrade_refused() {
        // Production config (allow_http false): https may follow, http may not.
        let u = |s: &str| Url::parse(s).unwrap();
        assert!(follow_redirect(&u("https://example.com/"), 0, false));
        assert!(!follow_redirect(&u("http://example.com/"), 0, false));
        assert!(!follow_redirect(&u("ftp://example.com/"), 0, true));
    }

    #[tokio::test]
    async fn s1_2_redirect_chain_capped_at_ten_hops() {
        let (base, seen) = serve(
            (0..12)
                .map(|_| http("302 Found", "Location: /next\r\n", ""))
                .collect(),
        )
        .await;
        let err = quick()
            .fetch(&format!("{base}/a.ics"), None)
            .await
            .unwrap_err();
        assert_eq!(err, FetchError::Status(302));
        assert_eq!(seen.lock().unwrap().len(), MAX_REDIRECTS); // reqwest counts the first URL in previous()
    }

    #[cfg(not(feature = "e2e-loopback"))]
    #[test]
    fn s1_2_http_to_loopback_is_refused_without_the_e2e_feature() {
        for raw in [
            "http://127.0.0.1:8080/a.ics",
            "http://localhost/a.ics",
            "http://[::1]/a.ics",
        ] {
            assert_eq!(parse_url(raw, false), Err(FetchError::UnsupportedScheme));
            assert_eq!(normalize_url(raw), Err(FetchError::UnsupportedScheme));
        }
    }

    #[cfg(not(feature = "e2e-loopback"))]
    #[test]
    fn s1_2_redirect_to_loopback_http_is_refused_without_the_e2e_feature() {
        let u = |s: &str| Url::parse(s).unwrap();
        for raw in [
            "http://127.0.0.1:9/b.ics",
            "http://localhost/b.ics",
            "http://[::1]/b.ics",
        ] {
            assert!(!follow_redirect(&u(raw), 0, false), "{raw}");
        }
    }

    #[cfg(feature = "e2e-loopback")]
    #[test]
    fn s1_2_e2e_feature_allows_http_to_loopback_hosts_only() {
        for raw in [
            "http://127.0.0.1:8080/a.ics",
            "http://127.1.2.3/a.ics",
            "http://[::1]:8080/a.ics",
            "http://localhost/a.ics",
            "http://LOCALHOST:9/a.ics",
        ] {
            assert!(parse_url(raw, false).is_ok(), "{raw}");
            assert!(normalize_url(raw).is_ok(), "{raw}");
        }
        for raw in [
            "http://example.com/a.ics",
            "http://127.0.0.1.example.com/a.ics",
            "http://localhost.example.com/a.ics",
            "http://127.0.0.1@example.com/a.ics",
            "http://10.0.0.1/a.ics",
            "http://192.168.1.1/a.ics",
            "http://169.254.169.254/a.ics",
            "http://0.0.0.0/a.ics",
            "http://[::ffff:127.0.0.1]/a.ics",
            "http://[::2]/a.ics",
            "ftp://127.0.0.1/a.ics",
        ] {
            assert_eq!(
                parse_url(raw, false),
                Err(FetchError::UnsupportedScheme),
                "{raw}"
            );
        }
    }

    #[cfg(feature = "e2e-loopback")]
    #[test]
    fn s1_2_e2e_feature_redirects_stay_on_loopback() {
        let u = |s: &str| Url::parse(s).unwrap();
        assert!(follow_redirect(&u("http://127.0.0.1:9/b.ics"), 0, false));
        assert!(!follow_redirect(&u("http://example.com/b.ics"), 0, false));
        assert!(!follow_redirect(&u("http://10.0.0.1/b.ics"), 0, false));
    }

    #[cfg(feature = "e2e-loopback")]
    #[tokio::test]
    async fn s1_1_e2e_feature_fetches_from_loopback_with_the_default_config() {
        let (base, _) = serve(vec![http("200 OK", "Content-Type: text/calendar\r\n", ICS)]).await;
        let out = IcsFetcher::new()
            .unwrap()
            .fetch(&format!("{base}/a.ics"), None)
            .await;
        assert!(matches!(out, Ok(FetchOutcome::Modified { .. })));
    }

    #[cfg(feature = "e2e-loopback")]
    #[tokio::test]
    async fn s1_2_e2e_feature_does_not_follow_a_redirect_off_loopback() {
        let (base, seen) = serve(vec![http(
            "302 Found",
            "Location: http://example.com/x.ics\r\n",
            "",
        )])
        .await;
        let err = IcsFetcher::new()
            .unwrap()
            .fetch(&format!("{base}/a.ics"), None)
            .await
            .unwrap_err();
        assert_eq!(err, FetchError::Status(302));
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn s1_2_other_schemes_rejected() {
        for raw in [
            "http://example.com/a.ics",
            "ftp://example.com/a.ics",
            "file:///etc/passwd",
            "secret-token:abc",
        ] {
            assert_eq!(parse_url(raw, false), Err(FetchError::UnsupportedScheme));
        }
        assert_eq!(parse_url("not a url", false), Err(FetchError::InvalidUrl));
    }

    #[test]
    fn s1_4_redact_url_keeps_scheme_and_host_only() {
        let r = redact_url(
            "https://user:pw@calendar.google.com:8443/ical/abc/private-123/basic.ics?k=1",
        );
        assert_eq!(r, "https://calendar.google.com");
        assert_eq!(redact_url("garbage private-123"), "<invalid url>");
    }

    #[tokio::test]
    async fn s1_1_accepts_text_calendar_and_returns_etag() {
        let (base, _) = serve(vec![http(
            "200 OK",
            "Content-Type: text/calendar; charset=utf-8\r\nETag: \"v1\"\r\n",
            ICS,
        )])
        .await;
        let out = quick().fetch(&format!("{base}/a.ics"), None).await.unwrap();
        assert_eq!(
            out,
            FetchOutcome::Modified {
                body: ICS.into(),
                etag: Some("\"v1\"".into())
            }
        );
    }

    #[tokio::test]
    async fn s1_1_rejects_non_calendar_body() {
        let (base, _) = serve(vec![http(
            "200 OK",
            "Content-Type: text/html\r\n",
            "<html></html>",
        )])
        .await;
        let err = quick()
            .fetch(&format!("{base}/a.ics"), None)
            .await
            .unwrap_err();
        assert_eq!(err, FetchError::NotCalendar);
    }

    #[tokio::test]
    async fn s1_1_accepts_text_plain_with_vcalendar_body() {
        let (base, _) = serve(vec![http("200 OK", "Content-Type: text/plain\r\n", ICS)]).await;
        let out = quick().fetch(&format!("{base}/a.ics"), None).await;
        assert!(matches!(out, Ok(FetchOutcome::Modified { .. })));
    }

    #[tokio::test]
    async fn s1_3_404_gives_status_404() {
        let (base, _) = serve(vec![http("404 Not Found", "", "")]).await;
        let err = quick()
            .fetch(&format!("{base}/a.ics"), None)
            .await
            .unwrap_err();
        assert_eq!(err, FetchError::Status(404));
        assert_eq!(err.status(), Some(404));
    }

    #[tokio::test]
    async fn s1_5_sends_if_none_match_and_304_is_not_modified() {
        let (base, seen) = serve(vec![http("304 Not Modified", "", "")]).await;
        let out = quick()
            .fetch(&format!("{base}/a.ics"), Some("\"v1\""))
            .await
            .unwrap();
        assert_eq!(out, FetchOutcome::NotModified);
        let head = seen.lock().unwrap()[0].to_ascii_lowercase();
        assert!(head.contains("if-none-match: \"v1\""), "{head}");
    }

    #[tokio::test]
    async fn s1_5_no_etag_means_no_header() {
        let (base, seen) =
            serve(vec![http("200 OK", "Content-Type: text/calendar\r\n", ICS)]).await;
        quick().fetch(&format!("{base}/a.ics"), None).await.unwrap();
        assert!(!seen.lock().unwrap()[0]
            .to_ascii_lowercase()
            .contains("if-none-match"));
    }

    #[tokio::test]
    async fn s1_3_body_over_cap_by_content_length_is_rejected() {
        let (base, _) = serve(vec![http(
            "200 OK",
            "Content-Type: text/calendar\r\n",
            &"x".repeat(2000),
        )])
        .await;
        let err = fetcher(Duration::from_secs(5), 1000)
            .fetch(&format!("{base}/a.ics"), None)
            .await
            .unwrap_err();
        assert_eq!(err, FetchError::TooLarge { limit: 1000 });
    }

    #[tokio::test]
    async fn s1_3_cap_enforced_while_streaming_without_content_length() {
        // Chunked body, no Content-Length: the cap must trip mid-stream.
        let chunk = "x".repeat(600);
        let mut raw = b"HTTP/1.1 200 OK\r\nContent-Type: text/calendar\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec();
        for _ in 0..4 {
            raw.extend_from_slice(format!("{:x}\r\n{chunk}\r\n", chunk.len()).as_bytes());
        }
        raw.extend_from_slice(b"0\r\n\r\n");
        let (base, _) = serve(vec![raw]).await;
        let err = fetcher(Duration::from_secs(5), 1000)
            .fetch(&format!("{base}/a.ics"), None)
            .await
            .unwrap_err();
        assert_eq!(err, FetchError::TooLarge { limit: 1000 });
    }

    #[tokio::test]
    async fn s1_3_timeout_gives_timeout_error() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let _held = listener.accept().await; // accept, never answer
            tokio::time::sleep(Duration::from_secs(5)).await;
        });
        let err = fetcher(Duration::from_millis(200), MAX_BODY)
            .fetch(&format!("{base}/a.ics"), None)
            .await
            .unwrap_err();
        assert_eq!(err, FetchError::Timeout);
    }

    #[derive(Clone, Default)]
    struct Buf(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Buf {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Buf {
        type Writer = Buf;
        fn make_writer(&'a self) -> Buf {
            self.clone()
        }
    }

    #[tokio::test]
    async fn s1_4_secret_absent_from_log() {
        // Global subscriber: a per-thread one races with callsite interest caching in parallel tests.
        static LOG: std::sync::OnceLock<Buf> = std::sync::OnceLock::new();
        let buf = LOG
            .get_or_init(|| {
                let buf = Buf::default();
                let sub = tracing_subscriber::fmt()
                    .with_writer(buf.clone())
                    .with_ansi(false)
                    .finish();
                let _ = tracing::subscriber::set_global_default(sub);
                buf
            })
            .clone();
        let (base, _) = serve(vec![http("404 Not Found", "", "")]).await;
        let err = quick()
            .fetch(&format!("{base}/ical/private-123/basic.ics"), None)
            .await
            .unwrap_err();
        let log = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert!(log.contains("127.0.0.1"), "{log}");
        assert!(log.contains("404"), "{log}");
        assert!(!log.contains("private-123"), "{log}");
        assert!(!format!("{err} {err:?}").contains("private-123"));
    }

    #[tokio::test]
    async fn s1_4_transport_error_hides_url() {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/private-123.ics", l.local_addr().unwrap());
        drop(l); // nothing listens: connection refused
        let err = quick().fetch(&url, None).await.unwrap_err();
        assert_eq!(err, FetchError::Connect);
        assert!(!format!("{err} {err:?}").contains("private-123"));
    }
}

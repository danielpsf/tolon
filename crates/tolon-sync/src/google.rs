//! Google Calendar (F13): OAuth 2.0 for installed apps with PKCE (S256) and a loopback redirect on
//! `127.0.0.1`, read-only scope, and incremental `events.list` sync.
//!
//! Secrets: refresh and access tokens, the authorization code, the account address and calendar
//! ids never reach a log line or an error's text. `GoogleError` carries only a kind or a status.
//! The caller keeps the refresh token in the OS keychain; access tokens live in memory only.

mod map;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::fmt;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use chrono::Utc;
use oauth2::basic::{BasicClient, BasicErrorResponse, BasicErrorResponseType};
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet, EndpointSet,
    ErrorResponse, HttpClientError, PkceCodeChallenge, RedirectUrl, RefreshToken,
    RequestTokenError, Scope, TokenResponse, TokenUrl,
};
use reqwest::{redirect, StatusCode, Url};
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tolon_core::{CalendarId, Event};

/// The only scope Tolon asks for (AC13.2, AGENTS.md rule 7).
pub const SCOPE: &str = "https://www.googleapis.com/auth/calendar.readonly";
const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";
const API_BASE: &str = "https://www.googleapis.com/calendar/v3";

const TIMEOUT: Duration = Duration::from_secs(30);
/// How long the loopback listener waits for the browser to come back.
const SIGN_IN_WAIT: Duration = Duration::from_secs(5 * 60);
/// A connection to the loopback listener that sends nothing is dropped after this.
const READ_WAIT: Duration = Duration::from_secs(10);
const MAX_HEAD: usize = 16 * 1024;
/// Connections read at once on the loopback port; any further one is dropped on arrival, so a local
/// process cannot pin a socket and a task per connection for READ_WAIT each.
const MAX_CONNECTIONS: usize = 16;
/// The first full listing starts this far back. The scheduler plans from one day back
/// (`LOOKBACK` in tolon-app's scheduler) and seven days ahead, so older events are never used. A
/// series still running is listed whatever its start, so a big calendar's history is not fetched.
const HISTORY: chrono::Duration = chrono::Duration::days(1);
/// 250 events a page: about 25,000 events before a listing is cut off as broken.
const MAX_PAGES: usize = 100;
/// An access token is renewed this long before Google says it expires.
const EXPIRY_MARGIN: Duration = Duration::from_secs(60);

/// The OAuth client: an ID and, for a Google "Desktop app" client, its client secret. Google's
/// token endpoint asks for that secret, and Google's installed-app guidance says it cannot be
/// kept confidential in a desktop app, so it is configuration, not a credential; it is still
/// never logged. Endpoints are Google's; tests point them at a local mock server.
#[derive(Clone)]
pub struct GoogleConfig {
    client_id: String,
    client_secret: Option<String>,
    auth_url: String,
    token_url: String,
    revoke_url: String,
    api_base: String,
    /// How long `sign_in` waits for the browser; tests shorten it.
    sign_in_wait: Duration,
}

impl GoogleConfig {
    pub fn new(client_id: String, client_secret: Option<String>) -> Self {
        Self {
            client_id,
            client_secret,
            auth_url: AUTH_URL.into(),
            token_url: TOKEN_URL.into(),
            revoke_url: REVOKE_URL.into(),
            api_base: API_BASE.into(),
            sign_in_wait: SIGN_IN_WAIT,
        }
    }
}

/// Why a Google call failed. `Display` and `Debug` never carry a token, a code, an address or a URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum GoogleError {
    /// The person refused access in the browser.
    #[error("sign-in was refused")]
    Denied,
    /// The refresh token expired or was revoked (`invalid_grant`): sign in again.
    #[error("sign in again")]
    Auth,
    #[error("server answered {0}")]
    Status(u16),
    #[error("request timed out")]
    Timeout,
    #[error("could not connect")]
    Connect,
    #[error("network error")]
    Network,
    #[error("unexpected answer from Google")]
    BadResponse,
    #[error("the browser could not be opened")]
    Browser,
}

/// A calendar of the signed-in account that Tolon can read.
#[derive(Clone, PartialEq, Eq)]
pub struct RemoteCalendar {
    /// Google's calendar id (the address for a primary calendar).
    pub id: String,
    pub name: String,
    pub primary: bool,
}

/// The id can be an address and the name a title: neither is printed.
impl fmt::Debug for RemoteCalendar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteCalendar")
            .field("primary", &self.primary)
            .finish_non_exhaustive()
    }
}

/// A finished sign-in. Keep `refresh_token` in the keychain only.
#[derive(Clone)]
pub struct SignIn {
    /// The primary calendar's id, which is the account's address.
    pub account: String,
    pub refresh_token: String,
    pub calendars: Vec<RemoteCalendar>,
}

impl fmt::Debug for SignIn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SignIn")
            .field("calendars", &self.calendars.len())
            .finish_non_exhaustive()
    }
}

/// The page the browser shows once it is sent back to Tolon (localised by the UI).
pub struct DonePage<'a> {
    pub title: &'a str,
    pub body: &'a str,
}

/// One calendar after a sync.
pub struct Synced {
    pub events: Vec<Event>,
    /// Keep for the next sync (`calendars.etag`).
    pub sync_token: String,
    /// Items that could not be read (no start, invalid RRULE), as VEVENTs are skipped in a feed.
    pub skipped: usize,
}

/// Event titles and the sync token are not printed.
impl fmt::Debug for Synced {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Synced")
            .field("events", &self.events.len())
            .field("skipped", &self.skipped)
            .finish_non_exhaustive()
    }
}

type OAuth = BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

pub struct GoogleClient {
    cfg: GoogleConfig,
    http: reqwest::Client,
    /// Refresh token to (access token, expiry). Memory only.
    access: Mutex<HashMap<String, (String, Instant)>>,
}

impl GoogleClient {
    pub fn new(cfg: GoogleConfig) -> Result<Self, GoogleError> {
        let http = reqwest::Client::builder()
            .timeout(TIMEOUT)
            // Token endpoints must not redirect (oauth2 crate guidance); plain http only reaches the test mock.
            .redirect(redirect::Policy::none())
            .https_only(!cfg!(test))
            .build()
            .map_err(|_| GoogleError::Network)?;
        Ok(Self {
            cfg,
            http,
            access: Mutex::new(HashMap::new()),
        })
    }

    fn oauth(&self) -> Result<OAuth, GoogleError> {
        let bad = |_| GoogleError::BadResponse;
        let mut client = BasicClient::new(ClientId::new(self.cfg.client_id.clone()))
            .set_auth_uri(AuthUrl::new(self.cfg.auth_url.clone()).map_err(bad)?)
            .set_token_uri(TokenUrl::new(self.cfg.token_url.clone()).map_err(bad)?)
            .set_auth_type(oauth2::AuthType::RequestBody);
        if let Some(secret) = &self.cfg.client_secret {
            client = client.set_client_secret(ClientSecret::new(secret.clone()));
        }
        Ok(client)
    }

    /// AC13.1: opens Google's consent page through `open` (the system browser), waits on a
    /// loopback port for the redirect, and trades the code and the PKCE verifier for tokens.
    /// Returns the account and its calendars.
    pub async fn sign_in(
        &self,
        open: impl FnOnce(&str) -> bool,
        page: &DonePage<'_>,
    ) -> Result<SignIn, GoogleError> {
        let result = self.sign_in_inner(open, page).await;
        if let Err(e) = &result {
            tracing::warn!(error = %e, "google sign-in failed");
        }
        result
    }

    async fn sign_in_inner(
        &self,
        open: impl FnOnce(&str) -> bool,
        page: &DonePage<'_>,
    ) -> Result<SignIn, GoogleError> {
        // Google's installed-app guidance: the IPv4 loopback address and any free port.
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| GoogleError::Connect)?;
        let port = listener
            .local_addr()
            .map_err(|_| GoogleError::Connect)?
            .port();
        let redirect = RedirectUrl::new(format!("http://127.0.0.1:{port}"))
            .map_err(|_| GoogleError::BadResponse)?;
        let oauth = self.oauth()?.set_redirect_uri(redirect);
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let (url, state) = oauth
            .authorize_url(CsrfToken::new_random)
            .add_scope(Scope::new(SCOPE.into()))
            .set_pkce_challenge(challenge)
            // A refresh token on every sign-in, also when this account signed in before.
            .add_extra_param("access_type", "offline")
            .add_extra_param("prompt", "consent")
            .url();
        if !open(url.as_str()) {
            return Err(GoogleError::Browser);
        }
        let code = tokio::time::timeout(
            self.cfg.sign_in_wait,
            wait_for_code(&listener, state.secret(), page),
        )
        .await
        .map_err(|_| GoogleError::Timeout)??;
        let tokens = oauth
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(verifier)
            .request_async(&self.http)
            .await
            .map_err(token_error)?;
        let refresh = tokens
            .refresh_token()
            .ok_or(GoogleError::BadResponse)?
            .secret()
            .clone();
        self.remember(
            &refresh,
            tokens.access_token().secret(),
            tokens.expires_in(),
        );
        let calendars = self.calendars(&refresh).await?;
        let account = calendars
            .iter()
            .find(|c| c.primary)
            .map(|c| c.id.clone())
            .ok_or(GoogleError::BadResponse)?;
        Ok(SignIn {
            account,
            refresh_token: refresh,
            calendars,
        })
    }

    fn remember(&self, refresh: &str, access: &str, expires_in: Option<Duration>) {
        let until = Instant::now()
            + expires_in
                .unwrap_or(EXPIRY_MARGIN)
                .saturating_sub(EXPIRY_MARGIN);
        self.access
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(refresh.to_owned(), (access.to_owned(), until));
    }

    fn forget(&self, refresh: &str) {
        self.access
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(refresh);
    }

    /// A valid access token: the cached one, else a refresh (AC13.3).
    async fn access_token(&self, refresh: &str) -> Result<String, GoogleError> {
        let cached = self
            .access
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(refresh)
            .filter(|(_, until)| Instant::now() < *until)
            .map(|(token, _)| token.clone());
        if let Some(token) = cached {
            return Ok(token);
        }
        let tokens = self
            .oauth()?
            .exchange_refresh_token(&RefreshToken::new(refresh.to_owned()))
            .request_async(&self.http)
            .await
            .map_err(token_error)?;
        let access = tokens.access_token().secret().clone();
        self.remember(refresh, &access, tokens.expires_in());
        Ok(access)
    }

    /// GET with the account's access token. A 401 renews the token once; a second 401 asks to sign in again.
    async fn get(&self, refresh: &str, url: &Url) -> Result<Vec<u8>, GoogleError> {
        for _ in 0..2 {
            let access = self.access_token(refresh).await?;
            let resp = self
                .http
                .get(url.clone())
                .bearer_auth(&access)
                .send()
                .await
                .map_err(|e| reqwest_error(&e))?;
            let status = resp.status();
            if status == StatusCode::UNAUTHORIZED {
                self.forget(refresh);
                continue;
            }
            if !status.is_success() {
                return Err(GoogleError::Status(status.as_u16()));
            }
            return resp
                .bytes()
                .await
                .map(|b| b.to_vec())
                .map_err(|e| reqwest_error(&e));
        }
        Err(GoogleError::Auth)
    }

    fn api(&self, path: &[&str]) -> Result<Url, GoogleError> {
        let mut url = Url::parse(&self.cfg.api_base).map_err(|_| GoogleError::BadResponse)?;
        // Each segment is percent-encoded, so a calendar id cannot change the path.
        url.path_segments_mut()
            .map_err(|_| GoogleError::BadResponse)?
            .extend(path);
        Ok(url)
    }

    /// The account's calendars, without the free/busy-only ones (their events have no details).
    pub async fn calendars(&self, refresh: &str) -> Result<Vec<RemoteCalendar>, GoogleError> {
        #[derive(Deserialize, Default)]
        #[serde(rename_all = "camelCase", default)]
        struct Page {
            items: Vec<Entry>,
            next_page_token: Option<String>,
        }
        #[derive(Deserialize, Default)]
        #[serde(rename_all = "camelCase", default)]
        struct Entry {
            id: String,
            summary: Option<String>,
            summary_override: Option<String>,
            primary: bool,
            access_role: Option<String>,
        }
        let mut out = Vec::new();
        let mut page_token: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let mut url = self.api(&["users", "me", "calendarList"])?;
            if let Some(t) = &page_token {
                url.query_pairs_mut().append_pair("pageToken", t);
            }
            let page: Page = serde_json::from_slice(&self.get(refresh, &url).await?)
                .map_err(|_| GoogleError::BadResponse)?;
            out.extend(
                page.items
                    .into_iter()
                    .filter(|e| e.access_role.as_deref() != Some("freeBusyReader"))
                    .map(|e| RemoteCalendar {
                        name: e
                            .summary_override
                            .or(e.summary)
                            .unwrap_or_else(|| e.id.clone()),
                        id: e.id,
                        primary: e.primary,
                    }),
            );
            match page.next_page_token {
                Some(t) => page_token = Some(t),
                None => return Ok(out),
            }
        }
        Err(GoogleError::BadResponse)
    }

    /// Every page of `events.list` for one calendar, with recurrence rules kept
    /// (`singleEvents=false`): a delta after `sync_token`, else everything. Returns the next sync token.
    async fn list_events(
        &self,
        refresh: &str,
        calendar_id: &str,
        sync_token: Option<&str>,
    ) -> Result<(Vec<map::Item>, String), GoogleError> {
        #[derive(Deserialize, Default)]
        #[serde(rename_all = "camelCase", default)]
        struct Page {
            items: Vec<map::Item>,
            next_page_token: Option<String>,
            next_sync_token: Option<String>,
        }
        let mut items = Vec::new();
        let mut page_token: Option<String> = None;
        // Google refuses `timeMin` with `syncToken`: a delta keeps the first listing's bounds.
        let time_min = sync_token
            .is_none()
            .then(|| (Utc::now() - HISTORY).to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
        for _ in 0..MAX_PAGES {
            let mut url = self.api(&["calendars", calendar_id, "events"])?;
            {
                let mut q = url.query_pairs_mut();
                q.append_pair("singleEvents", "false")
                    .append_pair("maxResults", "250");
                if let Some(t) = sync_token {
                    q.append_pair("syncToken", t);
                }
                if let Some(t) = &time_min {
                    q.append_pair("timeMin", t);
                }
                if let Some(t) = &page_token {
                    q.append_pair("pageToken", t);
                }
            }
            let page: Page = serde_json::from_slice(&self.get(refresh, &url).await?)
                .map_err(|_| GoogleError::BadResponse)?;
            items.extend(page.items);
            match (page.next_page_token, page.next_sync_token) {
                (Some(t), _) => page_token = Some(t),
                (None, Some(sync)) => return Ok((items, sync)),
                (None, None) => return Err(GoogleError::BadResponse),
            }
        }
        Err(GoogleError::BadResponse)
    }

    /// AC13.4: applies the changes since `sync_token` to `stored`, or lists everything when there
    /// is no token or Google answers 410 (the token expired).
    pub async fn sync(
        &self,
        refresh: &str,
        calendar_id: &str,
        sync_token: Option<&str>,
        stored: Vec<Event>,
        calendar: &CalendarId,
    ) -> Result<Synced, GoogleError> {
        let listed = match self.list_events(refresh, calendar_id, sync_token).await {
            Ok((items, token)) if sync_token.is_some() => Ok((items, token, stored)),
            Ok((items, token)) => Ok((items, token, Vec::new())),
            Err(GoogleError::Status(410)) if sync_token.is_some() => self
                .list_events(refresh, calendar_id, None)
                .await
                .map(|(items, token)| (items, token, Vec::new())),
            Err(e) => Err(e),
        };
        let (items, sync_token, base) = listed.inspect_err(|e| {
            tracing::warn!(error = %e, "google calendar sync failed");
        })?;
        let (events, skipped) = map::merge(base, items, calendar);
        Ok(Synced {
            events,
            sync_token,
            skipped,
        })
    }

    /// Revokes the grant behind `refresh` (the calendar was removed). Best effort: an answer that
    /// says the token is already invalid counts as done.
    pub async fn revoke(&self, refresh: &str) -> Result<(), GoogleError> {
        self.forget(refresh);
        // One form POST (RFC 7009). Not through oauth2: it refuses a non-https revocation URL, so
        // the mock server could not test it.
        let sent = self
            .http
            .post(&self.cfg.revoke_url)
            .form(&[("token", refresh)])
            .send()
            .await;
        let result = match sent {
            Ok(r) if r.status().is_success() || r.status() == StatusCode::BAD_REQUEST => Ok(()),
            Ok(r) => Err(GoogleError::Status(r.status().as_u16())),
            Err(e) => Err(reqwest_error(&e)),
        };
        if let Err(e) = &result {
            tracing::warn!(error = %e, "google token revocation failed");
        }
        result
    }
}

/// Accepts connections until one is Google's redirect carrying `state` and returns its code.
/// Connections are read side by side, so one that sends nothing (a browser preconnect) cannot
/// hold up the redirect. Anything else (a favicon, a request with another state) gets a 404.
async fn wait_for_code(
    listener: &TcpListener,
    state: &str,
    page: &DonePage<'_>,
) -> Result<String, GoogleError> {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<(TcpStream, String)>(8);
    let room = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (mut sock, _) = accepted.map_err(|_| GoogleError::Connect)?;
                // No room: the socket is dropped here, which closes it at once.
                let Ok(permit) = Arc::clone(&room).try_acquire_owned() else { continue };
                let tx = tx.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    if let Ok(Some(head)) = tokio::time::timeout(READ_WAIT, read_head(&mut sock)).await {
                        let _ = tx.send((sock, head)).await;
                    }
                });
            }
            Some((mut sock, head)) = rx.recv() => match redirect_result(&head, state) {
                Some(result) => {
                    // A refusal gets a page with no text: Tolon's window says what happened.
                    let html = done_html(result.is_ok().then_some(page));
                    respond(&mut sock, "200 OK", &html).await;
                    return result;
                }
                None => respond(&mut sock, "404 Not Found", "").await,
            },
        }
    }
}

async fn read_head(sock: &mut TcpStream) -> Option<String> {
    let mut head = Vec::new();
    let mut buf = [0u8; 1024];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = sock.read(&mut buf).await.ok()?;
        if n == 0 || head.len() + n > MAX_HEAD {
            return None;
        }
        head.extend_from_slice(&buf[..n]);
    }
    Some(String::from_utf8_lossy(&head).into_owned())
}

/// `Some` when the request line is `GET /?...&state=<state>`: the code, or `Denied` when Google
/// sent an error instead. `None` for anything else.
fn redirect_result(head: &str, state: &str) -> Option<Result<String, GoogleError>> {
    let target = head
        .lines()
        .next()?
        .strip_prefix("GET ")?
        .split(' ')
        .next()?;
    let url = Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
    if url.path() != "/" {
        return None;
    }
    let query: HashMap<_, _> = url.query_pairs().collect();
    if !query
        .get("state")
        .is_some_and(|s| same(s.as_bytes(), state.as_bytes()))
    {
        return None;
    }
    Some(match query.get("code") {
        Some(code) if !code.is_empty() => Ok(code.to_string()),
        _ => Err(GoogleError::Denied),
    })
}

/// Equal bytes, compared without stopping at the first difference. The length is not secret: the
/// CSRF token has a fixed size.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len()
        && std::hint::black_box(a.iter().zip(b).fold(0u8, |diff, (x, y)| diff | (x ^ y))) == 0
}

async fn respond(sock: &mut TcpStream, status: &str, body: &str) {
    let msg = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Security-Policy: default-src 'none'\r\nCache-Control: no-store\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    // The browser may already have gone; nothing to do about it.
    let _ = sock.write_all(msg.as_bytes()).await;
    let _ = sock.shutdown().await;
}

fn done_html(page: Option<&DonePage<'_>>) -> String {
    let body = page.map_or_else(String::new, |p| {
        format!("<h1>{}</h1><p>{}</p>", escape(p.title), escape(p.body))
    });
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Tolon</title></head>\
         <body>{body}</body></html>"
    )
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn reqwest_error(e: &reqwest::Error) -> GoogleError {
    if e.is_timeout() {
        GoogleError::Timeout
    } else if e.is_connect() {
        GoogleError::Connect
    } else {
        GoogleError::Network
    }
}

/// The server's description and the raw body are dropped: they can quote what was sent.
fn request_error<T: ErrorResponse + 'static>(
    e: RequestTokenError<HttpClientError<reqwest::Error>, T>,
) -> GoogleError {
    match e {
        RequestTokenError::Request(HttpClientError::Reqwest(e)) => reqwest_error(&e),
        RequestTokenError::Request(_) => GoogleError::Network,
        RequestTokenError::ServerResponse(_)
        | RequestTokenError::Parse(..)
        | RequestTokenError::Other(_) => GoogleError::BadResponse,
    }
}

/// `invalid_grant` means the refresh token (or the code) is no longer valid: sign in again.
fn token_error(
    e: RequestTokenError<HttpClientError<reqwest::Error>, BasicErrorResponse>,
) -> GoogleError {
    match e {
        RequestTokenError::ServerResponse(r)
            if *r.error() == BasicErrorResponseType::InvalidGrant =>
        {
            GoogleError::Auth
        }
        e => request_error(e),
    }
}

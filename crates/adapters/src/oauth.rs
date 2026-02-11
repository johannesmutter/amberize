//! OAuth2 authorization code flow with PKCE for Google / Gmail.
//!
//! This module handles:
//! - PKCE code verifier / challenge generation
//! - Authorization URL construction
//! - Local loopback HTTP server to capture the redirect callback
//! - Token exchange (authorization code → access + refresh tokens)
//! - Token refresh (refresh token → fresh access token)
//! - Token persistence in the Keychain via [`SecretStore`]
//!
//! **Client-ID strategy**: No shared secret is compiled into the binary.
//! Users create their own Google Cloud OAuth "Desktop app" credentials and
//! configure them in the app.  Client credentials are stored in the macOS
//! Keychain under [`GOOGLE_OAUTH_CLIENT_KEY`].

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::HashMap;
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::SecretStore;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Google OAuth2 authorization endpoint.
const GOOGLE_AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";

/// Google OAuth2 token endpoint.
const GOOGLE_TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";

/// Gmail IMAP scope — grants full mailbox access via IMAP.
const GOOGLE_IMAP_SCOPE: &str = "https://mail.google.com/";

/// Email scope — used to confirm the authenticated identity.
const GOOGLE_EMAIL_SCOPE: &str = "email";

/// Keychain key for the shared Google OAuth client credentials.
pub const GOOGLE_OAUTH_CLIENT_KEY: &str = "oauth_app:google";

/// Gmail's IMAP server hostname.
pub const GOOGLE_IMAP_HOST: &str = "imap.gmail.com";

/// Gmail's IMAPS port.
pub const GOOGLE_IMAP_PORT: u16 = 993;

/// How long we wait for the user to complete consent in the browser.
const CALLBACK_TIMEOUT_SECS: u64 = 300;

/// Safety margin subtracted from `expires_in` to avoid using a token that
/// is about to expire.
const TOKEN_EXPIRY_BUFFER_SECS: i64 = 120;

// ---------------------------------------------------------------------------
// Public data types
// ---------------------------------------------------------------------------

/// Google OAuth client credentials (Desktop app type).
///
/// Intentionally does **not** derive `Debug` to prevent `client_secret` from
/// leaking into log output.
#[derive(Clone, Serialize, Deserialize)]
pub struct GoogleOAuthClientConfig {
    pub client_id: String,
    pub client_secret: String,
}

impl std::fmt::Debug for GoogleOAuthClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoogleOAuthClientConfig")
            .field("client_id", &self.client_id)
            .field("client_secret", &"[REDACTED]")
            .finish()
    }
}

/// Persisted token set for one Google account.  Stored as JSON in Keychain
/// under the account's `secret_ref`.
///
/// Custom `Debug` impl redacts sensitive token values.
#[derive(Clone, Serialize, Deserialize)]
pub struct OAuthTokenData {
    pub access_token: String,
    pub refresh_token: String,
    /// RFC 3339 UTC timestamp after which `access_token` should be refreshed.
    pub expires_at_utc: String,
}

impl std::fmt::Debug for OAuthTokenData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthTokenData")
            .field("access_token", &"[REDACTED]")
            .field("refresh_token", &"[REDACTED]")
            .field("expires_at_utc", &self.expires_at_utc)
            .finish()
    }
}

/// Lightweight result returned to the UI after a successful authorization.
///
/// Custom `Debug` impl redacts the access token.
#[derive(Clone, Serialize)]
pub struct OAuthAuthorizeResult {
    pub email: String,
    pub access_token: String,
}

impl std::fmt::Debug for OAuthAuthorizeResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthAuthorizeResult")
            .field("email", &self.email)
            .field("access_token", &"[REDACTED]")
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum OAuthError {
    #[error("Google OAuth client credentials are not configured")]
    ClientNotConfigured,

    #[error("failed to read secret store: {0}")]
    SecretStore(String),

    #[error("failed to parse stored token data: {0}")]
    TokenParse(String),

    #[error("callback did not contain an authorization code")]
    MissingCode,

    #[error("OAuth state mismatch (possible CSRF)")]
    StateMismatch,

    #[error("authorization was denied: {0}")]
    AuthorizationDenied(String),

    #[error("token exchange failed: {0}")]
    TokenExchange(String),

    #[error("token refresh failed: {0}")]
    TokenRefresh(String),

    #[error("network error: {0}")]
    Network(String),

    #[error("callback server error: {0}")]
    CallbackServer(String),

    #[error("callback timed out — user did not complete authorization within {CALLBACK_TIMEOUT_SECS} seconds")]
    CallbackTimeout,

    #[error("failed to open browser: {0}")]
    Browser(String),

    #[error("missing refresh token in stored credentials")]
    MissingRefreshToken,
}

pub type OAuthResult<T> = Result<T, OAuthError>;

// ---------------------------------------------------------------------------
// Client config helpers
// ---------------------------------------------------------------------------

/// Load the shared Google OAuth client config from Keychain.
pub fn load_google_client_config(
    secret_store: &dyn SecretStore,
) -> OAuthResult<GoogleOAuthClientConfig> {
    let json = secret_store
        .get_secret(GOOGLE_OAUTH_CLIENT_KEY)
        .map_err(|e| OAuthError::SecretStore(e.to_string()))?
        .ok_or(OAuthError::ClientNotConfigured)?;

    serde_json::from_str(&json).map_err(|e| OAuthError::TokenParse(e.to_string()))
}

/// Persist the shared Google OAuth client config in Keychain.
pub fn save_google_client_config(
    secret_store: &dyn SecretStore,
    config: &GoogleOAuthClientConfig,
) -> OAuthResult<()> {
    let json = serde_json::to_string(config).map_err(|e| OAuthError::TokenParse(e.to_string()))?;

    secret_store
        .set_secret(GOOGLE_OAUTH_CLIENT_KEY, &json)
        .map_err(|e| OAuthError::SecretStore(e.to_string()))
}

// ---------------------------------------------------------------------------
// Token data helpers
// ---------------------------------------------------------------------------

/// Load per-account token data from Keychain.
pub fn load_token_data(
    secret_store: &dyn SecretStore,
    secret_ref: &str,
) -> OAuthResult<OAuthTokenData> {
    let json = secret_store
        .get_secret(secret_ref)
        .map_err(|e| OAuthError::SecretStore(e.to_string()))?
        .ok_or_else(|| {
            OAuthError::SecretStore(format!("no token data found for secret_ref '{secret_ref}'"))
        })?;

    serde_json::from_str(&json).map_err(|e| OAuthError::TokenParse(e.to_string()))
}

/// Remove per-account token data from Keychain (best-effort cleanup).
pub fn delete_token_data(secret_store: &dyn SecretStore, secret_ref: &str) {
    let _ = secret_store.delete_secret(secret_ref);
}

/// Persist per-account token data in Keychain.
pub fn save_token_data(
    secret_store: &dyn SecretStore,
    secret_ref: &str,
    data: &OAuthTokenData,
) -> OAuthResult<()> {
    let json = serde_json::to_string(data).map_err(|e| OAuthError::TokenParse(e.to_string()))?;

    secret_store
        .set_secret(secret_ref, &json)
        .map_err(|e| OAuthError::SecretStore(e.to_string()))
}

// ---------------------------------------------------------------------------
// Authorization flow (browser → loopback → token exchange)
// ---------------------------------------------------------------------------

/// Run the full Google OAuth2 authorization code flow with PKCE.
///
/// 1. Binds a TCP listener on `127.0.0.1:0` (OS picks a free port).
/// 2. Opens the user's default browser to Google's consent screen.
/// 3. Waits for the redirect callback (up to [`CALLBACK_TIMEOUT_SECS`]).
/// 4. Exchanges the authorization code for access + refresh tokens.
/// 5. Stores the tokens in Keychain under `secret_ref`.
///
/// Returns the access token (for immediate use with XOAUTH2) and the
/// authenticated email address (for display).
pub async fn google_authorize(
    secret_store: &dyn SecretStore,
    config: &GoogleOAuthClientConfig,
    login_hint: &str,
    secret_ref: &str,
) -> OAuthResult<OAuthAuthorizeResult> {
    let code_verifier = generate_pkce_verifier();
    let code_challenge = generate_pkce_challenge(&code_verifier);
    let state = uuid::Uuid::new_v4().to_string();

    // Bind loopback listener (port 0 = OS-assigned).
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| OAuthError::CallbackServer(e.to_string()))?;
    let port = listener
        .local_addr()
        .map_err(|e| OAuthError::CallbackServer(e.to_string()))?
        .port();

    let redirect_uri = format!("http://127.0.0.1:{port}");
    let scopes = format!("{GOOGLE_IMAP_SCOPE} {GOOGLE_EMAIL_SCOPE}");

    let auth_url = format!(
        "{GOOGLE_AUTH_ENDPOINT}\
         ?client_id={client_id}\
         &redirect_uri={redirect_uri}\
         &response_type=code\
         &scope={scopes}\
         &state={state}\
         &code_challenge={code_challenge}\
         &code_challenge_method=S256\
         &login_hint={login_hint}\
         &access_type=offline\
         &prompt=consent",
        client_id = urlencoded(&config.client_id),
        redirect_uri = urlencoded(&redirect_uri),
        scopes = urlencoded(&scopes),
        state = urlencoded(&state),
        code_challenge = urlencoded(&code_challenge),
        login_hint = urlencoded(login_hint),
    );

    // Open browser.
    open_browser(&auth_url)?;

    // Wait for the redirect callback.
    let auth_code = await_callback(listener, &state).await?;

    // Exchange the authorization code for tokens.
    let tokens = exchange_code(config, &auth_code, &redirect_uri, &code_verifier).await?;

    // Persist tokens in Keychain.
    save_token_data(secret_store, secret_ref, &tokens)?;

    Ok(OAuthAuthorizeResult {
        email: login_hint.to_string(),
        access_token: tokens.access_token,
    })
}

// ---------------------------------------------------------------------------
// Token refresh
// ---------------------------------------------------------------------------

/// Ensure the stored access token is still valid.  If it has expired (or is
/// about to), refresh it using the stored refresh token and update Keychain.
///
/// Returns a valid access token ready for XOAUTH2.
pub async fn ensure_fresh_google_token(
    secret_store: &dyn SecretStore,
    secret_ref: &str,
) -> OAuthResult<String> {
    let mut token_data = load_token_data(secret_store, secret_ref)?;

    if !is_token_expired(&token_data.expires_at_utc) {
        return Ok(token_data.access_token);
    }

    // Token is expired — refresh it.
    let client_config = load_google_client_config(secret_store)?;
    let refreshed = refresh_token(&client_config, &token_data.refresh_token).await?;

    token_data.access_token = refreshed.access_token;
    token_data.expires_at_utc = refreshed.expires_at_utc;
    // Google may rotate the refresh token; update if a new one was issued.
    if !refreshed.refresh_token.is_empty() {
        token_data.refresh_token = refreshed.refresh_token;
    }

    save_token_data(secret_store, secret_ref, &token_data)?;

    Ok(token_data.access_token)
}

/// Refresh an access token using a refresh token.
async fn refresh_token(
    config: &GoogleOAuthClientConfig,
    refresh_token_value: &str,
) -> OAuthResult<OAuthTokenData> {
    if refresh_token_value.is_empty() {
        return Err(OAuthError::MissingRefreshToken);
    }

    let params = [
        ("client_id", config.client_id.as_str()),
        ("client_secret", config.client_secret.as_str()),
        ("refresh_token", refresh_token_value),
        ("grant_type", "refresh_token"),
    ];

    let client = reqwest::Client::new();
    let resp = client
        .post(GOOGLE_TOKEN_ENDPOINT)
        .form(&params)
        .send()
        .await
        .map_err(|e| OAuthError::Network(e.to_string()))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| OAuthError::Network(e.to_string()))?;

    if !status.is_success() {
        return Err(OAuthError::TokenRefresh(format!("HTTP {status}: {body}")));
    }

    let raw: TokenResponse =
        serde_json::from_str(&body).map_err(|e| OAuthError::TokenRefresh(e.to_string()))?;

    let expires_at = compute_expires_at(raw.expires_in);

    Ok(OAuthTokenData {
        access_token: raw.access_token,
        refresh_token: raw.refresh_token.unwrap_or_default(),
        expires_at_utc: expires_at,
    })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// PKCE code verifier: 96 random hex characters derived from three UUIDv4s.
///
/// Three UUIDv4s provide 366 random bits (122 each), exceeding RFC 7636's
/// recommendation of 256 bits (32 octets).  The resulting 96-char hex string
/// is well within the allowed 43–128 character range.
fn generate_pkce_verifier() -> String {
    let v1 = uuid::Uuid::new_v4().as_simple().to_string();
    let v2 = uuid::Uuid::new_v4().as_simple().to_string();
    let v3 = uuid::Uuid::new_v4().as_simple().to_string();
    format!("{v1}{v2}{v3}")
}

/// PKCE code challenge: `base64url(sha256(verifier))` without padding.
fn generate_pkce_challenge(verifier: &str) -> String {
    let hash = sha2::Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hash)
}

/// Escape HTML-special characters to prevent XSS when interpolating
/// user-controlled or server-controlled values into the callback HTML page.
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Minimal percent-encoding for URL query values.
fn urlencoded(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{b:02X}"));
            }
        }
    }
    out
}

/// Open a URL in the user's default browser (macOS).
fn open_browser(url: &str) -> OAuthResult<()> {
    std::process::Command::new("open")
        .arg(url)
        .spawn()
        .map_err(|e| OAuthError::Browser(e.to_string()))?;
    Ok(())
}

/// Wait for the OAuth redirect callback on the loopback listener.
///
/// Reads a single HTTP request, extracts `code` and `state` from the query
/// string, validates state, then sends a human-friendly HTML response.
async fn await_callback(listener: TcpListener, expected_state: &str) -> OAuthResult<String> {
    let accept_future = listener.accept();

    let (mut stream, _addr) =
        tokio::time::timeout(Duration::from_secs(CALLBACK_TIMEOUT_SECS), accept_future)
            .await
            .map_err(|_| OAuthError::CallbackTimeout)?
            .map_err(|e| OAuthError::CallbackServer(e.to_string()))?;

    // Read the HTTP request (the redirect is a simple GET).
    let mut buf = vec![0u8; 8192];
    let n = stream
        .read(&mut buf)
        .await
        .map_err(|e| OAuthError::CallbackServer(e.to_string()))?;
    let request = String::from_utf8_lossy(&buf[..n]);

    // Parse the request line: `GET /?code=XXX&state=YYY HTTP/1.1`
    let path = request.split_whitespace().nth(1).unwrap_or("/");

    let query_string = path.split_once('?').map(|(_, q)| q).unwrap_or("");
    let params = parse_query_string(query_string);

    // Check for errors from Google.
    if let Some(error) = params.get("error") {
        let safe_error = html_escape(error);
        send_html_response(
            &mut stream,
            "Authorization failed",
            &format!("Google returned an error: <strong>{safe_error}</strong>. Please close this window and try again."),
        )
        .await;
        return Err(OAuthError::AuthorizationDenied(error.clone()));
    }

    // Validate CSRF state.
    let received_state = params.get("state").ok_or(OAuthError::StateMismatch)?;
    if received_state != expected_state {
        send_html_response(
            &mut stream,
            "Authorization failed",
            "Security check failed (state mismatch). Please close this window and try again.",
        )
        .await;
        return Err(OAuthError::StateMismatch);
    }

    // Extract the authorization code.
    let code = params.get("code").ok_or(OAuthError::MissingCode)?.clone();

    send_html_response(
        &mut stream,
        "Authorization successful",
        "You can close this window and return to <strong>Amberize</strong>.",
    )
    .await;

    Ok(code)
}

/// Send a minimal HTML page as an HTTP response on the callback connection.
async fn send_html_response(stream: &mut tokio::net::TcpStream, title: &str, body_html: &str) {
    let safe_title = html_escape(title);
    let html = format!(
        "<!DOCTYPE html>\
         <html><head><meta charset=\"utf-8\"><title>{safe_title}</title>\
         <style>body{{font-family:system-ui,sans-serif;display:flex;justify-content:center;\
         align-items:center;min-height:80vh;color:#333}}\
         .card{{text-align:center;max-width:400px}}\
         h2{{margin-bottom:0.5em}}</style></head>\
         <body><div class=\"card\"><h2>{safe_title}</h2><p>{body_html}</p></div></body></html>"
    );
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        html.len(),
        html
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.flush().await;
}

/// Exchange an authorization code for access + refresh tokens.
async fn exchange_code(
    config: &GoogleOAuthClientConfig,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> OAuthResult<OAuthTokenData> {
    let params = [
        ("code", code),
        ("client_id", config.client_id.as_str()),
        ("client_secret", config.client_secret.as_str()),
        ("redirect_uri", redirect_uri),
        ("grant_type", "authorization_code"),
        ("code_verifier", code_verifier),
    ];

    let client = reqwest::Client::new();
    let resp = client
        .post(GOOGLE_TOKEN_ENDPOINT)
        .form(&params)
        .send()
        .await
        .map_err(|e| OAuthError::Network(e.to_string()))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| OAuthError::Network(e.to_string()))?;

    if !status.is_success() {
        return Err(OAuthError::TokenExchange(format!("HTTP {status}: {body}")));
    }

    let raw: TokenResponse =
        serde_json::from_str(&body).map_err(|e| OAuthError::TokenExchange(e.to_string()))?;

    let expires_at = compute_expires_at(raw.expires_in);

    Ok(OAuthTokenData {
        access_token: raw.access_token,
        refresh_token: raw.refresh_token.unwrap_or_default(),
        expires_at_utc: expires_at,
    })
}

/// Parse a query string like `code=abc&state=xyz` into a map.
fn parse_query_string(qs: &str) -> HashMap<String, String> {
    qs.split('&')
        .filter(|s| !s.is_empty())
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            Some((percent_decode(key), percent_decode(value)))
        })
        .collect()
}

/// Very basic percent-decoding (covers the characters Google actually sends).
fn percent_decode(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&String::from_utf8_lossy(&bytes[i + 1..i + 3]), 16)
            {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Compute an RFC 3339 expiry timestamp from `expires_in` seconds, with a
/// safety buffer so we refresh slightly before actual expiry.
fn compute_expires_at(expires_in: i64) -> String {
    let effective = expires_in.saturating_sub(TOKEN_EXPIRY_BUFFER_SECS).max(0);
    let expires_at = time::OffsetDateTime::now_utc() + time::Duration::seconds(effective);
    expires_at
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

/// Check whether a token with the given expiry timestamp is expired.
fn is_token_expired(expires_at_utc: &str) -> bool {
    let Ok(expires_at) = time::OffsetDateTime::parse(
        expires_at_utc,
        &time::format_description::well_known::Rfc3339,
    ) else {
        // If we can't parse the timestamp, treat as expired to force a refresh.
        return true;
    };
    time::OffsetDateTime::now_utc() >= expires_at
}

/// Build the XOAUTH2 SASL initial response string.
///
/// Format: `user={email}\x01auth=Bearer {access_token}\x01\x01`
///
/// The caller (imap module) base64-encodes this before sending it on the wire.
pub fn build_xoauth2_sasl(email: &str, access_token: &str) -> Vec<u8> {
    format!("user={email}\x01auth=Bearer {access_token}\x01\x01").into_bytes()
}

// ---------------------------------------------------------------------------
// Wire-format types (Google token endpoint responses)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: i64,
    refresh_token: Option<String>,
}

impl std::fmt::Debug for TokenResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenResponse")
            .field("access_token", &"[REDACTED]")
            .field("expires_in", &self.expires_in)
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_verifier_length() {
        let v = generate_pkce_verifier();
        // Three UUIDv4 simple strings = 96 hex chars — within PKCE's 43–128 range.
        assert_eq!(v.len(), 96);
        assert!(
            v.len() >= 43 && v.len() <= 128,
            "verifier length: {}",
            v.len()
        );
    }

    #[test]
    fn pkce_challenge_is_base64url() {
        let v = generate_pkce_verifier();
        let c = generate_pkce_challenge(&v);
        // SHA-256 = 32 bytes → 43 base64url chars (no padding).
        assert_eq!(c.len(), 43);
        assert!(!c.contains('+'));
        assert!(!c.contains('/'));
        assert!(!c.contains('='));
    }

    #[test]
    fn urlencoded_preserves_unreserved() {
        assert_eq!(urlencoded("abc123"), "abc123");
        assert_eq!(urlencoded("a-b_c.d~e"), "a-b_c.d~e");
    }

    #[test]
    fn urlencoded_encodes_special_chars() {
        assert_eq!(urlencoded("a b"), "a%20b");
        assert_eq!(urlencoded("a@b"), "a%40b");
    }

    #[test]
    fn parse_query_string_basic() {
        let params = parse_query_string("code=abc123&state=xyz");
        assert_eq!(params.get("code").unwrap(), "abc123");
        assert_eq!(params.get("state").unwrap(), "xyz");
    }

    #[test]
    fn parse_query_string_percent_encoded() {
        let params = parse_query_string("code=a%20b&state=x%3Dy");
        assert_eq!(params.get("code").unwrap(), "a b");
        assert_eq!(params.get("state").unwrap(), "x=y");
    }

    #[test]
    fn build_xoauth2_sasl_format() {
        let sasl = build_xoauth2_sasl("user@gmail.com", "ya29.token");
        let expected = b"user=user@gmail.com\x01auth=Bearer ya29.token\x01\x01";
        assert_eq!(sasl, expected);
    }

    #[test]
    fn token_expiry_check() {
        // A timestamp far in the past is expired.
        assert!(is_token_expired("2020-01-01T00:00:00Z"));
        // A timestamp far in the future is not expired.
        assert!(!is_token_expired("2099-01-01T00:00:00Z"));
        // Unparseable timestamps are treated as expired.
        assert!(is_token_expired("not-a-date"));
    }

    #[test]
    fn xoauth2_base64_roundtrip() {
        use base64::engine::general_purpose::STANDARD;
        let sasl = build_xoauth2_sasl("user@gmail.com", "ya29.token");
        let encoded = STANDARD.encode(&sasl);
        let decoded = STANDARD.decode(encoded).unwrap();
        assert_eq!(sasl, decoded);
    }

    #[test]
    fn html_escape_prevents_xss() {
        assert_eq!(
            html_escape("<script>alert(1)</script>"),
            "&lt;script&gt;alert(1)&lt;/script&gt;"
        );
        assert_eq!(html_escape("a&b"), "a&amp;b");
        assert_eq!(html_escape("\"quoted\""), "&quot;quoted&quot;");
        assert_eq!(html_escape("it's"), "it&#x27;s");
        assert_eq!(html_escape("safe text 123"), "safe text 123");
    }

    #[test]
    fn debug_redacts_secrets() {
        let config = GoogleOAuthClientConfig {
            client_id: "my-id".to_string(),
            client_secret: "super-secret".to_string(),
        };
        let debug_str = format!("{config:?}");
        assert!(debug_str.contains("my-id"));
        assert!(!debug_str.contains("super-secret"));
        assert!(debug_str.contains("[REDACTED]"));

        let token = OAuthTokenData {
            access_token: "ya29.token".to_string(),
            refresh_token: "1//refresh".to_string(),
            expires_at_utc: "2099-01-01T00:00:00Z".to_string(),
        };
        let debug_str = format!("{token:?}");
        assert!(!debug_str.contains("ya29.token"));
        assert!(!debug_str.contains("1//refresh"));
        assert!(debug_str.contains("2099-01-01T00:00:00Z"));
    }
}

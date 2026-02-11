use futures_util::stream::{self, BoxStream};
use futures_util::{StreamExt, TryStreamExt};
use thiserror::Error;
use tokio::net::TcpStream;
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};

pub use async_imap::types::Name;
use async_imap::types::{Fetch, NameAttribute, Uid};

/// IMAP connection settings for password-based login.
///
/// Custom `Debug` impl redacts `password`.
#[derive(Clone)]
pub struct ImapConnectionSettings {
    pub host: String,
    pub port: u16,
    pub use_tls: bool,
    pub username: String,
    pub password: String,
}

impl std::fmt::Debug for ImapConnectionSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImapConnectionSettings")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("use_tls", &self.use_tls)
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Error)]
pub enum ImapError {
    #[error("unsupported security mode (TLS is required for now)")]
    UnsupportedSecurityMode,

    #[error("tcp connect failed: {0}")]
    TcpConnect(String),

    #[error("tls handshake failed: {0}")]
    Tls(String),

    #[error("imap protocol error: {0}")]
    Imap(String),

    #[error("login failed: {0}")]
    Login(String),
}

pub type ImapResult<T> = Result<T, ImapError>;

type TokioCompatTcpStream = Compat<TcpStream>;
type TlsStream = async_native_tls::TlsStream<TokioCompatTcpStream>;
pub type TlsSession = async_imap::Session<TlsStream>;

// ---------------------------------------------------------------------------
// XOAUTH2 SASL authenticator for `client.authenticate("XOAUTH2", …)`
// ---------------------------------------------------------------------------

/// XOAUTH2 authenticator for IMAP.
///
/// Implements the `async_imap::Authenticator` trait so we can pass it to
/// `client.authenticate("XOAUTH2", &mut auth)`.
///
/// Wire format (before base64, which the library handles):
/// `user={email}\x01auth=Bearer {access_token}\x01\x01`
struct XOAuth2Authenticator {
    response: Vec<u8>,
}

impl XOAuth2Authenticator {
    fn new(email: &str, access_token: &str) -> Self {
        Self {
            response: crate::oauth::build_xoauth2_sasl(email, access_token),
        }
    }
}

impl async_imap::Authenticator for XOAuth2Authenticator {
    type Response = Vec<u8>;

    /// Return the XOAUTH2 SASL initial response on the first call.
    ///
    /// On subsequent calls (server error challenge), return an empty response
    /// as required by the XOAUTH2 spec — the empty response acknowledges the
    /// error and lets the server send the final tagged NO.
    fn process(&mut self, _challenge: &[u8]) -> Self::Response {
        std::mem::take(&mut self.response)
    }
}

// ---------------------------------------------------------------------------
// TLS connection (shared by both auth methods)
// ---------------------------------------------------------------------------

/// Establish a TLS connection and read the IMAP greeting.
async fn establish_tls_connection(
    host: &str,
    port: u16,
) -> ImapResult<async_imap::Client<TlsStream>> {
    let tcp_stream = TcpStream::connect((host, port))
        .await
        .map_err(|err| ImapError::TcpConnect(err.to_string()))?;
    let tcp_stream = tcp_stream.compat();

    let tls_connector = async_native_tls::TlsConnector::new();
    let tls_stream = tls_connector
        .connect(host, tcp_stream)
        .await
        .map_err(|err| ImapError::Tls(err.to_string()))?;

    let mut client = async_imap::Client::new(tls_stream);
    let _greeting = client
        .read_response()
        .await
        .map_err(|err| ImapError::Imap(err.to_string()))?
        .ok_or_else(|| {
            ImapError::Imap("unexpected end of stream; expected greeting".to_string())
        })?;

    Ok(client)
}

// ---------------------------------------------------------------------------
// Password-based login (existing)
// ---------------------------------------------------------------------------

pub async fn connect_and_login(settings: &ImapConnectionSettings) -> ImapResult<TlsSession> {
    if !settings.use_tls {
        return Err(ImapError::UnsupportedSecurityMode);
    }

    let client = establish_tls_connection(&settings.host, settings.port).await?;

    let session = client
        .login(settings.username.as_str(), settings.password.as_str())
        .await
        .map_err(|(err, _client)| ImapError::Login(err.to_string()))?;

    Ok(session)
}

// ---------------------------------------------------------------------------
// XOAUTH2-based login (new — for Google OAuth accounts)
// ---------------------------------------------------------------------------

/// Connect to an IMAP server over TLS and authenticate using the XOAUTH2
/// SASL mechanism.  Used for Google / Gmail OAuth accounts.
pub async fn connect_and_authenticate_xoauth2(
    host: &str,
    port: u16,
    email: &str,
    access_token: &str,
) -> ImapResult<TlsSession> {
    let client = establish_tls_connection(host, port).await?;

    let authenticator = XOAuth2Authenticator::new(email, access_token);
    let session = client
        .authenticate("XOAUTH2", authenticator)
        .await
        .map_err(|(err, _client)| ImapError::Login(err.to_string()))?;

    Ok(session)
}

pub async fn list_mailboxes(session: &mut TlsSession) -> ImapResult<Vec<Name>> {
    let stream = session
        .list(Some(""), Some("*"))
        .await
        .map_err(|err| ImapError::Imap(err.to_string()))?;

    let names = stream
        .try_collect::<Vec<_>>()
        .await
        .map_err(|err| ImapError::Imap(err.to_string()))?;

    Ok(names)
}

pub async fn select_mailbox(
    session: &mut TlsSession,
    mailbox_name: &str,
) -> ImapResult<async_imap::types::Mailbox> {
    let mailbox = session
        .select(mailbox_name)
        .await
        .map_err(|err| ImapError::Imap(err.to_string()))?;
    Ok(mailbox)
}

pub async fn fetch_uids(session: &mut TlsSession, uid_set: &str) -> ImapResult<Vec<Fetch>> {
    if uid_set.trim().is_empty() {
        return Ok(vec![]);
    }

    let query = "(UID FLAGS INTERNALDATE BODY.PEEK[])";
    let stream = session
        .uid_fetch(uid_set, query)
        .await
        .map_err(|err| ImapError::Imap(err.to_string()))?;

    let fetches = stream
        .try_collect::<Vec<_>>()
        .await
        .map_err(|err| ImapError::Imap(err.to_string()))?;

    Ok(fetches)
}

pub async fn fetch_uids_stream<'a>(
    session: &'a mut TlsSession,
    uid_set: &str,
) -> ImapResult<BoxStream<'a, ImapResult<Fetch>>> {
    if uid_set.trim().is_empty() {
        return Ok(stream::empty::<ImapResult<Fetch>>().boxed());
    }

    let query = "(UID FLAGS INTERNALDATE BODY.PEEK[])";
    let stream = session
        .uid_fetch(uid_set, query)
        .await
        .map_err(|err| ImapError::Imap(err.to_string()))?;

    Ok(stream
        .map_err(|err| ImapError::Imap(err.to_string()))
        .boxed())
}

/// Allowed IMAP search criteria to prevent IMAP command injection.
const ALLOWED_SEARCH_COMMANDS: &[&str] = &[
    "ALL",
    "SEEN",
    "UNSEEN",
    "ANSWERED",
    "UNANSWERED",
    "DELETED",
    "UNDELETED",
    "FLAGGED",
    "UNFLAGGED",
    "NEW",
    "OLD",
    "RECENT",
    "DRAFT",
    "UNDRAFT",
];

/// Execute a UID SEARCH command with validated input.
///
/// Only safe, predefined search criteria are accepted. Arbitrary
/// user input must never be passed as `query`.
pub async fn uid_search(session: &mut TlsSession, query: &str) -> ImapResult<Vec<Uid>> {
    // Validate the query is a known-safe IMAP search command.
    let query_upper = query.trim().to_uppercase();
    if !ALLOWED_SEARCH_COMMANDS.contains(&query_upper.as_str()) {
        // Also allow UID range queries like "UID 1:*"
        let is_uid_range = query_upper.starts_with("UID ")
            && query_upper[4..]
                .trim()
                .chars()
                .all(|c| c.is_ascii_digit() || c == ':' || c == '*' || c == ',');

        if !is_uid_range {
            return Err(ImapError::Imap(format!(
                "rejected unsafe IMAP search query: {}",
                query.chars().take(50).collect::<String>()
            )));
        }
    }

    let mut uids = session
        .uid_search(query)
        .await
        .map_err(|err| ImapError::Imap(err.to_string()))?
        .into_iter()
        .collect::<Vec<_>>();
    uids.sort_unstable();
    Ok(uids)
}

pub fn name_attributes_to_string(attributes: &[NameAttribute<'_>]) -> String {
    attributes
        .iter()
        .map(|attr| format!("{attr:?}"))
        .collect::<Vec<_>>()
        .join(",")
}

pub fn is_hard_excluded_by_attributes(name: &Name) -> bool {
    name.attributes().iter().any(|attr| {
        matches!(
            attr,
            // `\NoSelect` mailboxes cannot be selected, so syncing would always fail.
            NameAttribute::NoSelect
        )
    })
}

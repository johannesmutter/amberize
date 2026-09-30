use serde::Serialize;
use thiserror::Error;

use email_archiver_storage::{
    AccountRow, IngestMessageLocationInput, InsertMessageBlobInput, MessageBlobMetadata, Storage,
    StorageError, UpsertMailboxInput, AUTH_KIND_OAUTH2,
};

use crate::imap::{self, ImapConnectionSettings, ImapError};
use crate::{SecretStore, SecretStoreError};

/// Maximum size of a single MIME message (50 MB). Messages exceeding this
/// limit are skipped to prevent memory exhaustion from malicious or
/// malformed emails.
const MAX_MESSAGE_SIZE_BYTES: usize = 50 * 1024 * 1024;

#[derive(Debug, Clone, Default)]
pub struct SyncSummary {
    pub mailboxes_seen: usize,
    pub mailboxes_synced: usize,
    pub messages_fetched: u64,
    pub messages_ingested: u64,
    pub had_mailbox_errors: bool,
    pub errors: Vec<String>,
}

/// Progress snapshot emitted during sync so callers can update the UI.
#[derive(Debug, Clone, Serialize)]
pub struct SyncProgress {
    /// Email address of the account being synced.
    pub account_email: String,
    /// Name of the mailbox currently being synced (e.g. "INBOX").
    pub mailbox_name: String,
    /// 1-based index of the current mailbox within the enabled set.
    pub mailbox_index: usize,
    /// Total number of enabled mailboxes for this account.
    pub mailbox_count: usize,
    /// Total messages fetched so far across all mailboxes in this sync run.
    pub messages_fetched: u64,
    /// Total messages ingested (new) so far across all mailboxes.
    pub messages_ingested: u64,
}

/// Callback type for receiving progress updates during sync.
pub type SyncProgressFn = Box<dyn Fn(&SyncProgress) + Send + Sync>;

#[derive(Debug, Error)]
pub enum SyncError {
    #[error(
        "The saved password is unavailable. Open Settings > Accounts and re-enter the password."
    )]
    MissingSecret { secret_ref: String },

    #[error("secret store error: {0}")]
    SecretStore(#[from] SecretStoreError),

    #[error("imap error: {0}")]
    Imap(#[from] ImapError),

    #[error("storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("oauth error: {0}")]
    OAuth(String),
}

pub async fn sync_account_once(
    storage: &Storage,
    secret_store: &dyn SecretStore,
    account: &AccountRow,
) -> Result<SyncSummary, SyncError> {
    sync_account_once_with_progress(storage, secret_store, account, None).await
}

pub async fn sync_account_once_with_progress(
    storage: &Storage,
    secret_store: &dyn SecretStore,
    account: &AccountRow,
    on_progress: Option<&SyncProgressFn>,
) -> Result<SyncSummary, SyncError> {
    let mut summary = SyncSummary::default();

    let mut session = connect_imap_for_account(secret_store, account).await?;

    let server_mailboxes = imap::list_mailboxes(&mut session).await?;
    summary.mailboxes_seen = server_mailboxes.len();

    let auto_archive_new_folders = account.mailbox_selection_mode == "auto";

    for name in &server_mailboxes {
        let hard_excluded = is_hard_excluded_mailbox(name);
        let sync_enabled = !hard_excluded && auto_archive_new_folders;

        let _mailbox_id = storage.upsert_mailbox(&UpsertMailboxInput {
            account_id: account.id,
            imap_name: name.name().to_string(),
            delimiter: name.delimiter().map(|d| d.to_string()),
            attributes: Some(imap::name_attributes_to_string(name.attributes())),
            sync_enabled,
            hard_excluded,
            uidvalidity: None,
            last_seen_uid: 0,
        })?;
    }

    let mailboxes = storage.list_mailboxes(account.id)?;
    let enabled_mailboxes = mailboxes
        .into_iter()
        .filter(|m| m.sync_enabled && !m.hard_excluded)
        .collect::<Vec<_>>();

    let mailbox_count = enabled_mailboxes.len();

    for (mailbox_index, mailbox) in enabled_mailboxes.iter().enumerate() {
        // Emit progress at the start of each mailbox.
        if let Some(cb) = on_progress {
            cb(&SyncProgress {
                account_email: account.email_address.clone(),
                mailbox_name: mailbox.imap_name.clone(),
                mailbox_index: mailbox_index + 1,
                mailbox_count,
                messages_fetched: summary.messages_fetched,
                messages_ingested: summary.messages_ingested,
            });
        }

        let mailbox_result = sync_mailbox(
            storage,
            &mut session,
            account.id,
            mailbox,
            &mut summary,
            on_progress,
            &account.email_address,
            mailbox_index + 1,
            mailbox_count,
        )
        .await;

        if let Err(failure) = mailbox_result {
            summary.had_mailbox_errors = true;
            summary
                .errors
                .push(format!("{}: {}", mailbox.imap_name, failure.message));
            storage.update_mailbox_cursor(
                mailbox.id,
                failure.uidvalidity,
                failure.last_seen_uid,
                Some(now_rfc3339()),
                Some(failure.message),
            )?;
            continue;
        }

        summary.mailboxes_synced += 1;
    }

    let status = if summary.had_mailbox_errors {
        "partial"
    } else {
        "ok"
    };
    storage.create_sync_finished_event(
        account.id,
        status,
        summary.messages_ingested,
        0, // messages_gone: not tracked yet
    )?;

    Ok(summary)
}

#[derive(Debug)]
struct MailboxFailure {
    message: String,
    last_seen_uid: u32,
    uidvalidity: Option<u32>,
}

fn effective_cursor(
    stored_generation: Option<u32>,
    stored_uid: u32,
    current: u32,
) -> (Option<u32>, u32) {
    let generation = if current == 0 {
        stored_generation
    } else {
        Some(current)
    };
    let uid = if current != 0 && stored_generation != Some(current) {
        0
    } else {
        stored_uid
    };
    (generation, uid)
}

#[allow(clippy::too_many_arguments)]
async fn sync_mailbox(
    storage: &Storage,
    session: &mut imap::TlsSession,
    account_id: i64,
    mailbox: &email_archiver_storage::MailboxRow,
    summary: &mut SyncSummary,
    on_progress: Option<&SyncProgressFn>,
    account_email: &str,
    mailbox_index: usize,
    mailbox_count: usize,
) -> Result<(), MailboxFailure> {
    let selected = imap::select_mailbox(session, &mailbox.imap_name)
        .await
        .map_err(|e| MailboxFailure {
            message: e.to_string(),
            last_seen_uid: mailbox.last_seen_uid,
            uidvalidity: mailbox.uidvalidity,
        })?;
    let (generation, mut cursor) = effective_cursor(
        mailbox.uidvalidity,
        mailbox.last_seen_uid,
        selected.uid_validity.unwrap_or(0),
    );
    let failure = |message: String, uid| MailboxFailure {
        message,
        last_seen_uid: uid,
        uidvalidity: generation,
    };
    let uids = imap::uid_search(session, &format!("UID {}:*", cursor.saturating_add(1)))
        .await
        .map_err(|e| failure(e.to_string(), cursor))?;
    // SEARCH results are sorted. Fetch one explicit UID so out-of-order server responses
    // cannot advance the cursor over a message that failed or has not been processed.
    for uid in uids
        .into_iter()
        .filter(|uid| *uid > cursor)
        .collect::<Vec<_>>()
    {
        let size = imap::fetch_size(session, uid)
            .await
            .map_err(|e| failure(e.to_string(), cursor))?;
        if size as usize > MAX_MESSAGE_SIZE_BYTES {
            return Err(failure(format!("UID {uid} is {size} bytes, above the 50 MiB archive limit. It remains unarchived; later UIDs in this folder wait until this message is resolved."),cursor));
        }
        let fetches = imap::fetch_uids(session, &uid.to_string())
            .await
            .map_err(|e| failure(e.to_string(), cursor))?;
        let fetch = fetches.iter().find(|f| f.uid == Some(uid)).ok_or_else(|| {
            failure(
                format!("Server returned no message for UID {uid}; retry discovery"),
                cursor,
            )
        })?;
        let body = fetch
            .body()
            .ok_or_else(|| failure(format!("Server returned UID {uid} without a body"), cursor))?;
        // Also reject servers that report a false size before making our second copy.
        if body.len() > MAX_MESSAGE_SIZE_BYTES {
            return Err(failure(
                format!("UID {uid} exceeded the 50 MiB limit despite its reported size"),
                cursor,
            ));
        }
        let sha256 = sha256_hex(body);
        let extracted = extract_metadata(body);
        let now = now_rfc3339();
        let flags = fetch
            .flags()
            .map(|f| format!("{f:?}"))
            .collect::<Vec<_>>()
            .join(",");
        storage
            .ingest_message(
                &InsertMessageBlobInput::raw(sha256, body.to_vec(), now.clone(), extracted),
                &IngestMessageLocationInput {
                    account_id,
                    mailbox_id: mailbox.id,
                    uidvalidity: generation.unwrap_or(0),
                    uid,
                    internal_date: fetch.internal_date().map(|d| d.to_rfc3339()),
                    flags: if flags.is_empty() { None } else { Some(flags) },
                    provider_message_id: None,
                    provider_thread_id: None,
                    provider_labels: None,
                    provider_meta_json: None,
                    first_seen_at: now.clone(),
                    last_seen_at: now,
                },
            )
            .map_err(|e| failure(e.to_string(), cursor))?;
        cursor = uid;
        summary.messages_fetched += 1;
        summary.messages_ingested += 1;
        // Persist bounded progress after each atomic ingestion. A crash between these
        // transactions repeats a deduplicated message instead of losing coverage.
        storage
            .update_mailbox_cursor(mailbox.id, generation, cursor, Some(now_rfc3339()), None)
            .map_err(|e| failure(e.to_string(), cursor))?;
        if let Some(cb) = on_progress {
            cb(&SyncProgress {
                account_email: account_email.into(),
                mailbox_name: mailbox.imap_name.clone(),
                mailbox_index,
                mailbox_count,
                messages_fetched: summary.messages_fetched,
                messages_ingested: summary.messages_ingested,
            });
        }
    }
    storage
        .update_mailbox_cursor(mailbox.id, generation, cursor, Some(now_rfc3339()), None)
        .map_err(|e| failure(e.to_string(), cursor))?;
    Ok(())
}

fn is_hard_excluded_mailbox(name: &async_imap::types::Name) -> bool {
    imap::is_hard_excluded_by_attributes(name)
        || is_hard_excluded_by_common_name(&name.name().to_ascii_lowercase())
}
pub fn is_hard_excluded_by_common_name(_name: &str) -> bool {
    false
}

async fn connect_imap_for_account(
    secret_store: &dyn SecretStore,
    account: &AccountRow,
) -> Result<imap::TlsSession, SyncError> {
    if account.auth_kind == AUTH_KIND_OAUTH2 {
        let access_token =
            crate::oauth::ensure_fresh_google_token(secret_store, &account.secret_ref)
                .await
                .map_err(|e| SyncError::OAuth(e.to_string()))?;

        let session = imap::connect_and_authenticate_xoauth2(
            &account.imap_host,
            account.imap_port,
            &account.email_address,
            &access_token,
        )
        .await?;

        Ok(session)
    } else {
        let password = secret_store
            .get_secret(&account.secret_ref)?
            .ok_or_else(|| SyncError::MissingSecret {
                secret_ref: account.secret_ref.clone(),
            })?;

        let settings = ImapConnectionSettings {
            host: account.imap_host.clone(),
            port: account.imap_port,
            use_tls: account.imap_tls,
            username: account.imap_username.clone(),
            password,
        };

        let session = imap::connect_and_login(&settings).await?;
        Ok(session)
    }
}

fn extract_metadata(raw_mime: &[u8]) -> MessageBlobMetadata {
    let Some(message) = mail_parser::MessageParser::default().parse(raw_mime) else {
        return MessageBlobMetadata::default();
    };

    let message_id = message.message_id().map(|s| s.to_string());
    let date_header = message.date().map(|d| d.to_rfc3339());
    let subject = message.subject().map(|s| s.to_string());
    let body_text = message.body_text(0).map(|t| t.to_string());

    let from_address = message
        .from()
        .and_then(|addr| addr.first())
        .and_then(|addr| format_addr(addr));

    let to_addresses = message.to().and_then(|addr| format_address(addr));
    let cc_addresses = message.cc().and_then(|addr| format_address(addr));

    MessageBlobMetadata {
        message_id,
        date_header,
        from_address,
        to_addresses,
        cc_addresses,
        subject,
        body_text,
    }
}

fn format_address(address: &mail_parser::Address<'_>) -> Option<String> {
    let parts = address
        .iter()
        .filter_map(|addr| format_addr(addr))
        .collect::<Vec<_>>();

    if parts.is_empty() {
        return None;
    }

    Some(parts.join(", "))
}

fn format_addr(addr: &mail_parser::Addr<'_>) -> Option<String> {
    let address = addr.address.as_deref()?;
    let name = addr.name.as_deref();

    match name {
        Some(name) if !name.trim().is_empty() => Some(format!("{name} <{address}>")),
        _ => Some(address.to_string()),
    }
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(data))
}

fn now_rfc3339() -> String {
    let now = time::OffsetDateTime::now_utc();
    now.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_excluded_common_names() {
        assert!(!is_hard_excluded_by_common_name("trash"));
        assert!(!is_hard_excluded_by_common_name("Spam"));
        assert!(!is_hard_excluded_by_common_name("Papierkorb"));
        assert!(!is_hard_excluded_by_common_name("inbox"));
    }
}

#[cfg(test)]
mod cursor_tests {
    use super::*;
    #[test]
    fn generation_reset_does_not_restore_old_high_cursor() {
        assert_eq!(effective_cursor(Some(1), 900, 2), (Some(2), 0));
        assert_eq!(effective_cursor(Some(2), 3, 2), (Some(2), 3));
        assert_eq!(effective_cursor(None, 900, 2), (Some(2), 0));
    }
}

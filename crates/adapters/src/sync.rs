use std::cmp;

use futures_util::StreamExt;
use serde::Serialize;
use thiserror::Error;

use email_archiver_storage::{
    AccountRow, IngestMessageLocationInput, InsertMessageBlobInput, MessageBlobMetadata, Storage,
    StorageError, UpsertMailboxInput, AUTH_KIND_OAUTH2,
};

use crate::imap::{self, ImapConnectionSettings, ImapError};
use crate::{SecretStore, SecretStoreError};

const UID_FETCH_FALLBACK_BATCH_SIZE: usize = 200;

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
    #[error("missing secret for secret_ref '{secret_ref}'")]
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

        if let Err((err_string, partial_max_uid)) = mailbox_result {
            summary.had_mailbox_errors = true;
            // Preserve partial progress: use the highest UID we successfully
            // processed (if any) so the next sync resumes from there.
            let cursor_uid = if partial_max_uid > mailbox.last_seen_uid {
                partial_max_uid
            } else {
                mailbox.last_seen_uid
            };
            let _ = storage.update_mailbox_cursor(
                mailbox.id,
                mailbox.uidvalidity,
                cursor_uid,
                Some(now_rfc3339()),
                Some(err_string),
            );
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

/// Returns `Err((error_string, partial_max_uid))` on failure so the caller
/// can persist partial progress.
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
) -> Result<(), (String, u32)> {
    let selected = imap::select_mailbox(session, mailbox.imap_name.as_str())
        .await
        .map_err(|err| (err.to_string(), mailbox.last_seen_uid))?;

    let current_uidvalidity = selected.uid_validity.unwrap_or(0);
    let mut last_seen_uid = mailbox.last_seen_uid;

    if let Some(stored_uidvalidity) = mailbox.uidvalidity {
        if current_uidvalidity != 0 && stored_uidvalidity != current_uidvalidity {
            last_seen_uid = 0;
        }
    }

    // Only use UIDNEXT as a short-circuit when we have a valid, non-reset cursor.
    // If last_seen_uid was reset to 0 (UIDVALIDITY changed), we must always attempt a fetch.
    if last_seen_uid > 0 {
        if let Some(uid_next) = selected.uid_next {
            if uid_next > 1 {
                let end_uid = uid_next.saturating_sub(1);
                if end_uid <= last_seen_uid {
                    storage
                        .update_mailbox_cursor(
                            mailbox.id,
                            if current_uidvalidity == 0 {
                                mailbox.uidvalidity
                            } else {
                                Some(current_uidvalidity)
                            },
                            last_seen_uid,
                            Some(now_rfc3339()),
                            None,
                        )
                        .map_err(|err| (err.to_string(), last_seen_uid))?;
                    return Ok(());
                }
            }
        }
    }

    let start_uid = last_seen_uid.saturating_add(1);
    let uid_set = format!("{start_uid}:*");

    let mut max_seen_uid = last_seen_uid;
    let mut fetched_in_mailbox: u64 = 0;
    let mut fetch_stream = imap::fetch_uids_stream(session, uid_set.as_str())
        .await
        .map_err(|err| (err.to_string(), max_seen_uid))?;

    while let Some(fetch) = fetch_stream.next().await {
        let fetch = fetch.map_err(|err| (err.to_string(), max_seen_uid))?;

        let Some(uid) = fetch.uid else {
            return Err((
                format!(
                    "mailbox '{}' returned FETCH without UID (seq={})",
                    mailbox.imap_name, fetch.message
                ),
                max_seen_uid,
            ));
        };
        let Some(body) = fetch.body() else {
            return Err((
                format!(
                    "mailbox '{}' returned FETCH without body for uid {uid}",
                    mailbox.imap_name
                ),
                max_seen_uid,
            ));
        };

        // Skip messages exceeding the size limit.
        let body_bytes = body.to_vec();
        if body_bytes.len() > MAX_MESSAGE_SIZE_BYTES {
            eprintln!(
                "warning: skipping uid={uid} in '{}' ({} bytes > {} limit)",
                mailbox.imap_name,
                body_bytes.len(),
                MAX_MESSAGE_SIZE_BYTES,
            );
            max_seen_uid = cmp::max(max_seen_uid, uid);
            continue;
        }

        summary.messages_fetched += 1;
        fetched_in_mailbox += 1;

        let raw_mime = body_bytes;
        let sha256 = sha256_hex(&raw_mime);

        let imported_at = now_rfc3339();
        let extracted = extract_metadata(&raw_mime);

        let flags = fetch
            .flags()
            .map(|flag| format!("{flag:?}"))
            .collect::<Vec<_>>()
            .join(",");

        let internal_date = fetch.internal_date().map(|dt| dt.to_rfc3339());

        let now = now_rfc3339();

        // Atomic: insert blob + location in a single transaction.
        storage
            .ingest_message(
                &InsertMessageBlobInput::raw(sha256, raw_mime, imported_at, extracted),
                &IngestMessageLocationInput {
                    account_id,
                    mailbox_id: mailbox.id,
                    uidvalidity: current_uidvalidity,
                    uid,
                    internal_date,
                    flags: if flags.is_empty() { None } else { Some(flags) },
                    provider_message_id: None,
                    provider_thread_id: None,
                    provider_labels: None,
                    provider_meta_json: None,
                    first_seen_at: now.clone(),
                    last_seen_at: now,
                },
            )
            .map_err(|err| (err.to_string(), max_seen_uid))?;

        summary.messages_ingested += 1;
        max_seen_uid = cmp::max(max_seen_uid, uid);

        // Emit progress after each message so the UI can update.
        if let Some(cb) = on_progress {
            cb(&SyncProgress {
                account_email: account_email.to_string(),
                mailbox_name: mailbox.imap_name.clone(),
                mailbox_index,
                mailbox_count,
                messages_fetched: summary.messages_fetched,
                messages_ingested: summary.messages_ingested,
            });
        }
    }

    // Important: the stream borrows `session` mutably; drop it before issuing other IMAP commands.
    drop(fetch_stream);

    // If the primary UID range fetch yielded nothing on the first sync, fall back to:
    // `UID SEARCH ALL` → fetch explicit UID batches. This helps with servers that accept
    // `UID FETCH 1:*` but return no FETCH responses.
    if last_seen_uid == 0 && fetched_in_mailbox == 0 {
        let searched_all = imap::uid_search(session, "ALL")
            .await
            .map_err(|err| (err.to_string(), max_seen_uid))?;

        let mut uids_to_fetch = searched_all
            .into_iter()
            .filter(|uid| *uid != 0 && *uid > last_seen_uid)
            .collect::<Vec<_>>();
        uids_to_fetch.sort_unstable();

        if !uids_to_fetch.is_empty() {
            for batch in uids_to_fetch.chunks(UID_FETCH_FALLBACK_BATCH_SIZE) {
                let uid_set = uids_to_sequence_set(batch);
                let mut batch_stream = imap::fetch_uids_stream(session, uid_set.as_str())
                    .await
                    .map_err(|err| (err.to_string(), max_seen_uid))?;

                while let Some(fetch) = batch_stream.next().await {
                    let fetch = fetch.map_err(|err| (err.to_string(), max_seen_uid))?;

                    let Some(uid) = fetch.uid else {
                        return Err((
                            format!(
                                "mailbox '{}' returned FETCH without UID (seq={})",
                                mailbox.imap_name, fetch.message
                            ),
                            max_seen_uid,
                        ));
                    };
                    let Some(body) = fetch.body() else {
                        return Err((
                            format!(
                                "mailbox '{}' returned FETCH without body for uid {uid}",
                                mailbox.imap_name
                            ),
                            max_seen_uid,
                        ));
                    };

                    // Skip oversized messages in fallback path too.
                    let body_bytes = body.to_vec();
                    if body_bytes.len() > MAX_MESSAGE_SIZE_BYTES {
                        eprintln!(
                            "warning: skipping uid={uid} in '{}' ({} bytes > {} limit)",
                            mailbox.imap_name,
                            body_bytes.len(),
                            MAX_MESSAGE_SIZE_BYTES,
                        );
                        max_seen_uid = cmp::max(max_seen_uid, uid);
                        continue;
                    }

                    summary.messages_fetched += 1;
                    fetched_in_mailbox += 1;

                    let raw_mime = body_bytes;
                    let sha256 = sha256_hex(&raw_mime);

                    let imported_at = now_rfc3339();
                    let extracted = extract_metadata(&raw_mime);

                    let flags = fetch
                        .flags()
                        .map(|flag| format!("{flag:?}"))
                        .collect::<Vec<_>>()
                        .join(",");

                    let internal_date = fetch.internal_date().map(|dt| dt.to_rfc3339());

                    let now = now_rfc3339();

                    // Atomic: insert blob + location in a single transaction.
                    storage
                        .ingest_message(
                            &InsertMessageBlobInput::raw(sha256, raw_mime, imported_at, extracted),
                            &IngestMessageLocationInput {
                                account_id,
                                mailbox_id: mailbox.id,
                                uidvalidity: current_uidvalidity,
                                uid,
                                internal_date,
                                flags: if flags.is_empty() { None } else { Some(flags) },
                                provider_message_id: None,
                                provider_thread_id: None,
                                provider_labels: None,
                                provider_meta_json: None,
                                first_seen_at: now.clone(),
                                last_seen_at: now,
                            },
                        )
                        .map_err(|err| (err.to_string(), max_seen_uid))?;

                    summary.messages_ingested += 1;
                    max_seen_uid = cmp::max(max_seen_uid, uid);

                    if let Some(cb) = on_progress {
                        cb(&SyncProgress {
                            account_email: account_email.to_string(),
                            mailbox_name: mailbox.imap_name.clone(),
                            mailbox_index,
                            mailbox_count,
                            messages_fetched: summary.messages_fetched,
                            messages_ingested: summary.messages_ingested,
                        });
                    }
                }
            }
        }
    }

    if last_seen_uid == 0 && selected.exists > 0 && fetched_in_mailbox == 0 {
        return Err((
            format!(
                "mailbox '{}' reports {} messages, but fetched 0 messages (uidvalidity={current_uidvalidity}, uidnext={:?})",
                mailbox.imap_name, selected.exists, selected.uid_next
            ),
            max_seen_uid,
        ));
    }

    storage
        .update_mailbox_cursor(
            mailbox.id,
            if current_uidvalidity == 0 {
                mailbox.uidvalidity
            } else {
                Some(current_uidvalidity)
            },
            max_seen_uid,
            Some(now_rfc3339()),
            None,
        )
        .map_err(|err| (err.to_string(), max_seen_uid))?;

    Ok(())
}

fn uids_to_sequence_set(uids: &[u32]) -> String {
    if uids.is_empty() {
        return String::new();
    }

    let mut parts = Vec::new();
    let mut range_start = uids[0];
    let mut prev = uids[0];

    for &uid in &uids[1..] {
        if uid == prev.saturating_add(1) {
            prev = uid;
            continue;
        }

        parts.push(format_uid_range(range_start, prev));
        range_start = uid;
        prev = uid;
    }

    parts.push(format_uid_range(range_start, prev));
    parts.join(",")
}

fn format_uid_range(start: u32, end: u32) -> String {
    if start == end {
        start.to_string()
    } else {
        format!("{start}:{end}")
    }
}

fn is_hard_excluded_mailbox(name: &async_imap::types::Name) -> bool {
    if imap::is_hard_excluded_by_attributes(name) {
        return true;
    }

    let mailbox_name = name.name().to_ascii_lowercase();
    is_hard_excluded_by_common_name(&mailbox_name)
}

pub fn is_hard_excluded_by_common_name(mailbox_name_lower: &str) -> bool {
    // Common mailbox names like “Spam”, “Junk”, “Trash”, “Drafts” etc. are typically selectable
    // and can be valuable to archive (especially for troubleshooting). We only treat `\NoSelect`
    // mailboxes as hard-excluded.
    let _ = mailbox_name_lower;
    false
}

/// Connect to the IMAP server for a given account, using the appropriate
/// authentication method (password or OAuth2 XOAUTH2).
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

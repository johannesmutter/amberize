use std::path::{Path, PathBuf};

use email_archiver_adapters::{
    imap::{ImapConnectionSettings, Name},
    is_hard_excluded_by_common_name,
    oauth::{self, GoogleOAuthClientConfig},
    sync_account_once_with_progress, KeychainSecretStore, SecretStore,
};
use email_archiver_storage::{
    CreateAccountInput, InsertEventInput, MessageListSortOrder, Storage, UpsertMailboxInput,
    AUTH_KIND_OAUTH2, AUTH_KIND_PASSWORD, OAUTH_PROVIDER_GOOGLE, PROVIDER_KIND_CLASSIC_IMAP,
    PROVIDER_KIND_GOOGLE_IMAP,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use tauri_plugin_autostart::AutoLaunchManager;

use crate::app_state::{AppState, UiSyncStatus};
use crate::sync_status_text::format_last_sync_status_text;

const MAILBOX_SELECTION_MODE_AUTO: &str = "auto";
const MAILBOX_SELECTION_MODE_MANUAL: &str = "manual";

const DEFAULT_IMAP_TLS: bool = true;
const DEFAULT_MANUAL_SYNC_ENABLED_INBOX: bool = true;

const UI_SEARCH_LIMIT: usize = 50;
const MAX_SEARCH_QUERY_LEN: usize = 1000;
const MESSAGE_SORT_NEWEST: &str = "newest";
const MESSAGE_SORT_OLDEST: &str = "oldest";

const EVENT_KIND_MESSAGE_EML_EXPORTED: &str = "message_eml_exported";
const EVENT_KIND_ACCOUNT_REMOVED: &str = "account_removed";
const EVENT_KIND_MAILBOX_SYNC_CHANGED: &str = "mailbox_sync_changed";
const EVENT_SYNC_STATUS_UPDATED: &str = "sync_status_updated";

/// Google's IMAP SCOPES used in OAuth authorization.
const GOOGLE_OAUTH_SCOPES: &str = "https://mail.google.com/ email";

#[derive(Debug, Clone, Serialize)]
pub struct UiSyncError {
    pub account_id: i64,
    pub email_address: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UiAggregateSyncSummary {
    pub accounts_seen: usize,
    pub accounts_synced: usize,
    pub accounts_with_errors: usize,
    pub mailboxes_seen_total: usize,
    pub mailboxes_synced_total: usize,
    pub messages_fetched_total: u64,
    pub messages_ingested_total: u64,
    pub errors: Vec<UiSyncError>,
}

/// Input for creating a new IMAP (password) account.
///
/// Custom `Debug` impl redacts `password` to prevent it from leaking into logs.
#[derive(Clone, Deserialize)]
pub struct CreateAccountCommandInput {
    pub label: String,
    pub email_address: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub imap_username: String,
    pub password: String,
    pub mailbox_selection_mode: String,
}

impl std::fmt::Debug for CreateAccountCommandInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreateAccountCommandInput")
            .field("label", &self.label)
            .field("email_address", &self.email_address)
            .field("imap_host", &self.imap_host)
            .field("imap_port", &self.imap_port)
            .field("imap_username", &self.imap_username)
            .field("password", &"[REDACTED]")
            .field("mailbox_selection_mode", &self.mailbox_selection_mode)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UiAccount {
    pub id: i64,
    pub label: String,
    pub email_address: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub imap_tls: bool,
    pub imap_username: String,
    pub mailbox_selection_mode: String,
    pub disabled: bool,
    pub auth_kind: String,
    pub oauth_provider: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UiMailbox {
    pub id: i64,
    pub account_id: i64,
    pub imap_name: String,
    pub sync_enabled: bool,
    pub hard_excluded: bool,
    pub gobd_recommended: bool,
    pub last_seen_uid: u32,
    pub last_sync_at: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateAccountCommandResult {
    pub account: UiAccount,
    pub mailboxes: Vec<UiMailbox>,
}

/// Input for IMAP mailbox discovery.
///
/// Custom `Debug` impl redacts `password`.
#[derive(Clone, Deserialize)]
pub struct ImapDiscoverInput {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

impl std::fmt::Debug for ImapDiscoverInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImapDiscoverInput")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredMailbox {
    pub imap_name: String,
    pub delimiter: Option<String>,
    pub attributes: Option<String>,
    pub hard_excluded: bool,
    pub gobd_recommended: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct UiSyncSummary {
    pub mailboxes_seen: usize,
    pub mailboxes_synced: usize,
    pub messages_fetched: u64,
    pub messages_ingested: u64,
    pub had_mailbox_errors: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct UiSearchMessageRow {
    pub id: i64,
    pub subject: Option<String>,
    pub from_address: Option<String>,
    pub date_header: Option<String>,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UiMessageRow {
    pub sort_timestamp: i64,
    pub has_attachments: bool,
    pub id: i64,
    pub message_blob_id: i64,
    pub subject: Option<String>,
    pub from_address: Option<String>,
    pub date_header: Option<String>,
    pub snippet: String,
    pub account_id: i64,
    pub account_email: String,
    pub mailbox_id: i64,
    pub mailbox_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UiMessageBlobRaw {
    pub id: i64,
    pub sha256: String,
    pub raw_mime_text: String,
}

/// Parsed message detail returned to the UI.
#[derive(Debug, Clone, Serialize)]
pub struct UiMessageDetail {
    pub id: i64,
    pub sha256: String,
    pub subject: Option<String>,
    pub from_address: Option<String>,
    pub to_addresses: Option<String>,
    pub cc_addresses: Option<String>,
    pub date_header: Option<String>,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub attachments: Vec<UiAttachment>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UiAttachment {
    pub embedded_in_body: bool,
    pub filename: Option<String>,
    pub content_type: String,
    pub size: usize,
    pub is_inline: bool,
    /// For inline images referenced by CID, a data URI so the HTML body can
    /// render them directly. Only populated for images within configured size caps.
    pub content_id: Option<String>,
    pub data_uri: Option<String>,
}

const INLINE_IMAGE_SIZE_LIMIT: usize = 2 * 1024 * 1024;
const INLINE_IMAGE_TOTAL_SIZE_LIMIT: usize = 6 * 1024 * 1024;

/// Bound decoded image memory as well as compressed bytes.
fn image_within_pixel_limit(bytes: &[u8]) -> bool {
    let dimensions = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.len() >= 24 {
        Some((
            u32::from_be_bytes(bytes[16..20].try_into().unwrap()),
            u32::from_be_bytes(bytes[20..24].try_into().unwrap()),
        ))
    } else if (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) && bytes.len() >= 10 {
        Some((
            u16::from_le_bytes(bytes[6..8].try_into().unwrap()) as u32,
            u16::from_le_bytes(bytes[8..10].try_into().unwrap()) as u32,
        ))
    } else if bytes.starts_with(b"\xff\xd8") {
        let mut position = 2;
        let mut dimensions = None;
        while position + 4 <= bytes.len() {
            if bytes[position] != 0xff {
                break;
            }
            let marker = bytes[position + 1];
            position += 2;
            if marker == 0xff {
                position -= 1;
                continue;
            }
            if marker == 0xda || marker == 0xd9 {
                break;
            }
            if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
                continue;
            }
            let length =
                u16::from_be_bytes(bytes[position..position + 2].try_into().unwrap()) as usize;
            if length < 2 || position + length > bytes.len() {
                break;
            }
            if matches!(marker,0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) && length >= 7
            {
                dimensions = Some((
                    u16::from_be_bytes(bytes[position + 5..position + 7].try_into().unwrap())
                        as u32,
                    u16::from_be_bytes(bytes[position + 3..position + 5].try_into().unwrap())
                        as u32,
                ));
                break;
            }
            position += length;
        }
        dimensions
    } else {
        None
    };
    dimensions.is_some_and(|(width, height)| {
        width > 0 && height > 0 && u64::from(width) * u64::from(height) <= 4_000_000
    })
}

/// Parse raw MIME bytes into a structured message detail for the UI.
fn parse_mime_to_detail(id: i64, sha256: String, raw: &[u8]) -> UiMessageDetail {
    use base64::Engine;
    use mail_parser::{MessageParser, MimeHeaders};

    let Some(message) = MessageParser::default().parse(raw) else {
        return UiMessageDetail {
            id,
            sha256,
            subject: None,
            from_address: None,
            to_addresses: None,
            cc_addresses: None,
            date_header: None,
            body_text: Some(String::from_utf8_lossy(raw).to_string()),
            body_html: None,
            attachments: Vec::new(),
        };
    };

    let subject = message.subject().map(|s| s.to_string());
    let date_header = message.date().map(|d| d.to_rfc3339());
    let from_address = message.from().and_then(|a| format_address_list(a));
    let to_addresses = message.to().and_then(|a| format_address_list(a));
    let cc_addresses = message.cc().and_then(|a| format_address_list(a));

    let body_text = message.body_text(0).map(|t| t.to_string());

    // Collect inline images first so we can resolve CID references in HTML.
    let mut attachments: Vec<UiAttachment> = Vec::new();
    let mut cid_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    let mut inline_image_bytes_used: usize = 0;

    for part in message.parts.iter() {
        let content_type_raw = part.content_type();
        let ct: String = content_type_raw
            .map(|c| {
                if let Some(sub) = c.subtype() {
                    format!("{}/{}", c.ctype(), sub)
                } else {
                    c.ctype().to_string()
                }
            })
            .unwrap_or_default();

        // Skip the top-level message wrapper and text/html or text/plain body parts.
        let is_text_body = ct == "text/plain" || ct == "text/html" || ct.starts_with("multipart/");
        if is_text_body {
            continue;
        }

        let content_id: Option<String> = part
            .content_id()
            .map(|cid: &str| cid.trim_matches(|c: char| c == '<' || c == '>').to_string());

        let is_inline = part
            .content_disposition()
            .map(|d| d.ctype() == "inline")
            .unwrap_or(false)
            || content_id.is_some();

        let filename: Option<String> = part.attachment_name().map(|n: &str| n.to_string());

        let body = part.contents();
        let size = body.len();

        let data_uri = if is_inline
            && ct.starts_with("image/")
            && image_within_pixel_limit(body)
            && size <= INLINE_IMAGE_SIZE_LIMIT
            && inline_image_bytes_used + size <= INLINE_IMAGE_TOTAL_SIZE_LIMIT
        {
            let b64 = base64::engine::general_purpose::STANDARD.encode(body);
            let uri = format!("data:{};base64,{}", ct, b64);
            // Map CID → data URI for HTML replacement.
            if let Some(ref cid) = content_id {
                cid_map.insert(cid.clone(), uri.clone());
            }
            inline_image_bytes_used += size;
            Some(uri)
        } else {
            None
        };

        attachments.push(UiAttachment {
            embedded_in_body: false,
            filename,
            content_type: ct,
            size,
            is_inline,
            content_id,
            data_uri,
        });
    }

    // Get HTML body, resolving CID references to data URIs.
    let body_html = message.body_html(0).map(|html| {
        let mut resolved = html.to_string();
        for (cid, data_uri) in &cid_map {
            let needle = format!("cid:{}", cid);
            if resolved.len().saturating_add(
                resolved
                    .matches(&needle)
                    .count()
                    .saturating_mul(data_uri.len()),
            ) <= 10 * 1024 * 1024
            {
                let embedded = resolved.contains(&needle);
                resolved = resolved.replace(&needle, data_uri);
                for attachment in &mut attachments {
                    if attachment.content_id.as_ref() == Some(cid) && embedded {
                        attachment.embedded_in_body = true;
                        attachment.data_uri = None;
                    }
                }
            }
        }
        resolved
    });

    UiMessageDetail {
        id,
        sha256,
        subject,
        from_address,
        to_addresses,
        cc_addresses,
        date_header,
        body_text,
        body_html,
        attachments,
    }
}

/// Format a mail_parser Address list into a display string.
fn format_address_list(address: &mail_parser::Address<'_>) -> Option<String> {
    let parts: Vec<String> = address
        .iter()
        .filter_map(|addr| match (&addr.name, &addr.address) {
            (Some(name), Some(email)) => Some(format!("{} <{}>", name, email)),
            (None, Some(email)) => Some(email.to_string()),
            (Some(name), None) => Some(name.to_string()),
            _ => None,
        })
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

#[tauri::command]
pub async fn get_message_detail(
    db_path: String,
    message_blob_id: i64,
) -> Result<UiMessageDetail, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let db_path = validate_db_path(&db_path)?;
        let storage = Storage::open_existing(&db_path).map_err(|e| e.to_string())?;
        let raw = storage
            .get_message_blob_raw_mime(message_blob_id)
            .map_err(|e| e.to_string())?;

        Ok(parse_mime_to_detail(raw.id, raw.sha256, &raw.raw_mime))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn autostart_is_enabled(app_handle: AppHandle) -> Result<bool, String> {
    let autostart_manager = app_handle
        .try_state::<AutoLaunchManager>()
        .ok_or("Launch at login is unavailable on this system")?;
    autostart_manager.is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn autostart_set_enabled(app_handle: AppHandle, enabled: bool) -> Result<(), String> {
    let autostart_manager = app_handle
        .try_state::<AutoLaunchManager>()
        .ok_or("Launch at login is unavailable on this system")?;
    if enabled {
        autostart_manager.enable().map_err(|e| e.to_string())
    } else {
        autostart_manager.disable().map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub fn restart_app(app_handle: AppHandle) {
    app_handle.restart();
}

#[tauri::command]
pub async fn imap_discover_mailboxes(
    input: ImapDiscoverInput,
) -> Result<Vec<DiscoveredMailbox>, String> {
    let settings = ImapConnectionSettings {
        host: input.host,
        port: input.port,
        use_tls: DEFAULT_IMAP_TLS,
        username: input.username,
        password: input.password,
    };

    let mut session = email_archiver_adapters::imap::connect_and_login(&settings)
        .await
        .map_err(|e| e.to_string())?;
    let names = email_archiver_adapters::imap::list_mailboxes(&mut session)
        .await
        .map_err(|e| e.to_string())?;

    Ok(names.into_iter().map(map_discovered_mailbox).collect())
}

#[tauri::command]
pub async fn create_account_and_discover_mailboxes(
    db_path: String,
    input: CreateAccountCommandInput,
) -> Result<CreateAccountCommandResult, String> {
    let db_path = validate_db_path(&db_path)?;
    let mailbox_selection_mode = normalize_mailbox_selection_mode(&input.mailbox_selection_mode)?;

    let server_mailboxes = discover_mailboxes_for_credentials(&input).await?;

    let secret_ref = format!("account:{}", uuid::Uuid::new_v4());

    let storage = Storage::open_existing(&db_path).map_err(|e| e.to_string())?;
    let secret_store = KeychainSecretStore::new();
    secret_store
        .set_secret(&secret_ref, &input.password)
        .map_err(|e| e.to_string())?;
    let mailboxes = server_mailboxes
        .into_iter()
        .map(|mailbox| UpsertMailboxInput {
            account_id: 0,
            sync_enabled: default_sync_enabled_for_mailbox(
                mailbox_selection_mode,
                &mailbox.imap_name,
                mailbox.hard_excluded,
            ),
            imap_name: mailbox.imap_name,
            delimiter: mailbox.delimiter,
            attributes: mailbox.attributes,
            hard_excluded: mailbox.hard_excluded,
            uidvalidity: None,
            last_seen_uid: 0,
        })
        .collect::<Vec<_>>();
    let account_id = match storage.create_account_with_mailboxes(
        &CreateAccountInput {
            label: input.label,
            email_address: input.email_address,
            provider_kind: PROVIDER_KIND_CLASSIC_IMAP.into(),
            imap_host: input.imap_host,
            imap_port: input.imap_port,
            imap_tls: DEFAULT_IMAP_TLS,
            imap_username: input.imap_username,
            auth_kind: AUTH_KIND_PASSWORD.into(),
            secret_ref: secret_ref.clone(),
            mailbox_selection_mode: mailbox_selection_mode.into(),
            oauth_provider: None,
            oauth_scopes: None,
        },
        &mailboxes,
    ) {
        Ok(id) => id,
        Err(error) => {
            let _ = secret_store.delete_secret(&secret_ref);
            return Err(error.to_string());
        }
    };

    let accounts = storage.list_accounts().map_err(|e| e.to_string())?;
    let account = accounts
        .into_iter()
        .find(|a| a.id == account_id)
        .ok_or_else(|| "created account not found".to_string())?;

    let mailboxes = list_mailboxes_internal(&storage, account_id)?;

    Ok(CreateAccountCommandResult {
        account: map_ui_account(&account),
        mailboxes,
    })
}

#[tauri::command]
pub async fn list_accounts(db_path: String) -> Result<Vec<UiAccount>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let db_path = validate_db_path(&db_path)?;
        let storage = Storage::open_existing(&db_path).map_err(|e| e.to_string())?;
        let accounts = storage.list_accounts().map_err(|e| e.to_string())?;
        Ok(accounts.iter().map(map_ui_account).collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn list_mailboxes(db_path: String, account_id: i64) -> Result<Vec<UiMailbox>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let db_path = validate_db_path(&db_path)?;
        let storage = Storage::open_existing(&db_path).map_err(|e| e.to_string())?;
        list_mailboxes_internal(&storage, account_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn set_mailbox_sync_enabled(
    db_path: String,
    mailbox_id: i64,
    sync_enabled: bool,
) -> Result<(), String> {
    let db_path = validate_db_path(&db_path)?;
    let storage = Storage::open_existing(&db_path).map_err(|e| e.to_string())?;

    // Look up mailbox info for the audit event.
    let mailbox_info = storage.get_mailbox_by_id(mailbox_id).ok().flatten();

    storage
        .set_mailbox_sync_enabled(mailbox_id, sync_enabled)
        .map_err(|e| e.to_string())?;

    // Log mailbox_sync_changed event in the audit chain.
    let mailbox_name = mailbox_info
        .as_ref()
        .map(|m| m.imap_name.as_str())
        .unwrap_or("unknown");
    let account_id = mailbox_info.as_ref().map(|m| m.account_id);

    let _ = storage.append_event(&InsertEventInput {
        occurred_at: now_rfc3339(),
        kind: EVENT_KIND_MAILBOX_SYNC_CHANGED.to_string(),
        account_id,
        mailbox_id: Some(mailbox_id),
        message_blob_id: None,
        detail: format!(
            r#"{{"mailbox":"{}","sync_enabled":{}}}"#,
            escape_json_value(mailbox_name),
            sync_enabled,
        ),
    });

    Ok(())
}

#[tauri::command]
pub fn set_account_password(
    db_path: String,
    account_id: i64,
    password: String,
) -> Result<(), String> {
    let db_path = validate_db_path(&db_path)?;

    if password.is_empty() {
        return Err("password is required".to_string());
    }

    let storage = Storage::open_existing(&db_path).map_err(|e| e.to_string())?;
    let accounts = storage.list_accounts().map_err(|e| e.to_string())?;
    let Some(account) = accounts.into_iter().find(|a| a.id == account_id) else {
        return Err("account not found".to_string());
    };

    // Guard: only allow password updates on password-authenticated accounts.
    // OAuth accounts store JSON token data under the same `secret_ref` key —
    // writing a plaintext password would corrupt those tokens.
    if account.auth_kind != AUTH_KIND_PASSWORD {
        return Err(format!(
            "cannot set password on a {} account (auth_kind='{}')",
            account.oauth_provider.as_deref().unwrap_or("non-password"),
            account.auth_kind
        ));
    }

    let secret_store = KeychainSecretStore::new();
    secret_store
        .set_secret(&account.secret_ref, &password)
        .map_err(|e| e.to_string())?;

    // Verify the write immediately so we can surface keychain issues clearly.
    let saved = secret_store
        .get_secret(&account.secret_ref)
        .map_err(|e| e.to_string())?;
    if saved.as_deref() != Some(password.as_str()) {
        return Err("password could not be verified in keychain".to_string());
    }

    Ok(())
}

async fn archive_activation_guard<'a>(
    state: &'a AppState,
    db_path: &str,
) -> Result<(PathBuf, Option<tokio::sync::MutexGuard<'a, ()>>), String> {
    let validated = validate_db_path(db_path)?;
    let canonical=validated.canonicalize().map_err(|e|format!("The saved archive is unavailable at {}: {e}. Reconnect its drive or choose the existing file.",validated.display()))?;
    let canonical_text = canonical.to_str().ok_or_else(|| {
        "The archive path contains unsupported characters. Move it to a path with valid Unicode characters, then select the existing file.".to_string()
    })?;
    let already_active = || -> Result<bool, String> {
        Ok(state
            .active_db_path
            .lock()
            .map_err(|e| e.to_string())?
            .as_deref()
            == Some(canonical_text))
    };
    // Recreated windows acknowledge the current archive without waiting for a
    // potentially long provider sync or changing ownership/status.
    if already_active()? {
        return Ok((canonical, None));
    }
    let guard = state.sync_lock.lock().await;
    // Another activation may have selected this archive while we waited.
    if already_active()? {
        return Ok((canonical, None));
    }
    Ok((canonical, Some(guard)))
}

#[tauri::command]
pub async fn set_active_db_path(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    db_path: String,
) -> Result<(), String> {
    let (canonical, guard) = archive_activation_guard(&state, &db_path).await?;
    let Some(_guard) = guard else {
        return Ok(());
    };
    let worker_app = app_handle.clone();
    let path = canonical.to_string_lossy().into_owned();
    tauri::async_runtime::spawn_blocking(move || crate::bootstrap::activate(&worker_app, &path))
        .await
        .map_err(|e| e.to_string())??;
    state.sync_wakeup.notify_one();
    if let Ok(mut warning) = state.startup_warning.lock() {
        *warning = None;
    }
    crate::bootstrap::verify_async(app_handle);
    Ok(())
}

#[tauri::command]
pub async fn clear_active_db_path(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let _sync_guard = state.sync_lock.lock().await;
    {
        let mut guard = state
            .active_db_path
            .lock()
            .map_err(|_| "internal error: mutex poisoned".to_string())?;
        *guard = None;
    }
    *state.archive_lock.lock().map_err(|e| e.to_string())? = None;
    *state.integrity_status.lock().map_err(|e| e.to_string())? = None;

    {
        let mut guard = state
            .last_sync
            .lock()
            .map_err(|_| "internal error: mutex poisoned".to_string())?;
        let next = UiSyncStatus {
            last_success_at: None,
            error: None,
            sync_in_progress: false,
            last_sync_at: None,
            last_sync_status: "not configured".to_string(),
        };
        *guard = next;
    }

    state.set_tray_status_text("Last sync: not configured");
    let _ = app_handle.emit(EVENT_SYNC_STATUS_UPDATED, ());
    Ok(())
}

#[tauri::command]
pub fn get_sync_status(state: State<'_, AppState>) -> Result<UiSyncStatus, String> {
    let mut status = state
        .last_sync
        .lock()
        .map_err(|_| "internal error: mutex poisoned".to_string())?
        .clone();
    status.sync_in_progress = state.sync_in_progress();
    Ok(status)
}

/// Returns the current background sync interval in seconds.
#[tauri::command]
pub fn get_sync_interval(state: State<'_, AppState>) -> Result<u64, String> {
    Ok(state.sync_interval_secs())
}

/// Sets the background sync interval (in seconds). The new value takes
/// effect on the next sleep cycle — the currently running timer is not
/// interrupted.
#[tauri::command]
pub fn set_sync_interval(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    interval_secs: u64,
) -> Result<(), String> {
    const MIN_INTERVAL_SECS: u64 = 60;
    if interval_secs < MIN_INTERVAL_SECS {
        return Err(format!(
            "interval must be at least {MIN_INTERVAL_SECS} seconds"
        ));
    }
    if interval_secs > 86400 {
        return Err("Interval must be at most one day".into());
    }
    if let Some(mut config) = get_app_config(app_handle.clone())? {
        config.sync_interval_secs = interval_secs;
        save_app_config(app_handle, config)?;
    } else {
        state.set_sync_interval_secs(interval_secs);
    }
    state.sync_wakeup.notify_one();
    Ok(())
}

#[tauri::command]
pub async fn sync_account_once_command(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    db_path: String,
    account_id: i64,
) -> Result<UiSyncSummary, String> {
    let db_path = validate_db_path(&db_path)?.to_string_lossy().to_string();

    let _guard = state.sync_lock.lock().await;
    let active = state
        .active_db_path
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    if !active.as_ref().is_some_and(|active| {
        email_archiver_storage::is_same_archive_file(active, &db_path).unwrap_or(false)
    }) {
        return Err(
            "The active archive changed. Retry synchronization from the current archive.".into(),
        );
    }
    state.set_sync_in_progress(true);
    state.set_tray_status_text("Status: syncing…");
    let _ = app_handle.emit(EVENT_SYNC_STATUS_UPDATED, ());

    let on_progress = crate::background_sync::progress_callback(app_handle.clone());

    let result = async {
        let storage = Storage::open_existing(&db_path).map_err(|e| e.to_string())?;
        let accounts = storage.list_accounts().map_err(|e| e.to_string())?;
        let Some(account) = accounts.iter().find(|a| a.id == account_id) else {
            return Err("account not found".to_string());
        };

        let secret_store = KeychainSecretStore::new();
        sync_account_once_with_progress(&storage, &secret_store, account, Some(&on_progress))
            .await
            .map_err(|e| e.to_string())
    }
    .await;

    let now = now_rfc3339();
    match result {
        Ok(summary) => {
            let status = if summary.had_mailbox_errors {
                "partial"
            } else {
                "ok"
            };
            let status_text = format_last_sync_status_text(status, now.as_str());
            if let Ok(mut guard) = state.last_sync.lock() {
                let mut next = UiSyncStatus {
                    last_success_at: None,
                    error: if summary.had_mailbox_errors {
                        Some(summary.errors.join("; "))
                    } else {
                        None
                    },
                    sync_in_progress: false,
                    last_sync_at: Some(now.clone()),
                    last_sync_status: status_text.clone(),
                };
                next.preserve_success(&guard);
                *guard = next;
            }
            state.set_sync_in_progress(false);
            crate::background_sync::persist_sync_status(&db_path, &state);
            state.set_tray_status_text(&format!("Last sync: {status_text}"));
            let _ = app_handle.emit(EVENT_SYNC_STATUS_UPDATED, ());

            Ok(UiSyncSummary {
                mailboxes_seen: summary.mailboxes_seen,
                mailboxes_synced: summary.mailboxes_synced,
                messages_fetched: summary.messages_fetched,
                messages_ingested: summary.messages_ingested,
                had_mailbox_errors: summary.had_mailbox_errors,
            })
        }
        Err(err) => {
            let status_text = format_last_sync_status_text("error", now.as_str());
            if let Ok(mut guard) = state.last_sync.lock() {
                let mut next = UiSyncStatus {
                    last_success_at: None,
                    error: Some(err.clone()),
                    sync_in_progress: false,
                    last_sync_at: Some(now.clone()),
                    last_sync_status: status_text.clone(),
                };
                next.preserve_success(&guard);
                *guard = next;
            }
            state.set_sync_in_progress(false);
            crate::background_sync::persist_sync_status(&db_path, &state);
            state.set_tray_status_text(&format!("Last sync: {status_text}"));
            let _ = app_handle.emit(EVENT_SYNC_STATUS_UPDATED, ());
            Err(err)
        }
    }
}

#[tauri::command]
pub async fn sync_all_accounts_command(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    db_path: String,
) -> Result<UiAggregateSyncSummary, String> {
    let db_path = validate_db_path(&db_path)?.to_string_lossy().to_string();

    let _guard = state.sync_lock.lock().await;
    let active = state
        .active_db_path
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    if !active.as_ref().is_some_and(|active| {
        email_archiver_storage::is_same_archive_file(active, &db_path).unwrap_or(false)
    }) {
        return Err(
            "The active archive changed. Retry synchronization from the current archive.".into(),
        );
    }
    state.set_sync_in_progress(true);
    state.set_tray_status_text("Status: syncing…");
    let _ = app_handle.emit(EVENT_SYNC_STATUS_UPDATED, ());

    let on_progress = crate::background_sync::progress_callback(app_handle.clone());

    let result: Result<UiAggregateSyncSummary, String> = async {
        let storage = Storage::open_existing(&db_path).map_err(|e| e.to_string())?;
        let accounts = storage.list_accounts().map_err(|e| e.to_string())?;
        let secret_store = KeychainSecretStore::new();

        let mut aggregate = UiAggregateSyncSummary {
            accounts_seen: accounts.len(),
            accounts_synced: 0,
            accounts_with_errors: 0,
            mailboxes_seen_total: 0,
            mailboxes_synced_total: 0,
            messages_fetched_total: 0,
            messages_ingested_total: 0,
            errors: vec![],
        };

        for account in accounts {
            if account.disabled {
                continue;
            }

            let account_id = account.id;
            let email_address = account.email_address.clone();
            match sync_account_once_with_progress(
                &storage,
                &secret_store,
                &account,
                Some(&on_progress),
            )
            .await
            {
                Ok(summary) => {
                    aggregate.accounts_synced += 1;
                    aggregate.mailboxes_seen_total += summary.mailboxes_seen;
                    aggregate.mailboxes_synced_total += summary.mailboxes_synced;
                    aggregate.messages_fetched_total += summary.messages_fetched;
                    aggregate.messages_ingested_total += summary.messages_ingested;
                    if summary.had_mailbox_errors {
                        aggregate.errors.push(UiSyncError {
                            account_id,
                            email_address: email_address.clone(),
                            message: summary.errors.join("; "),
                        });
                        aggregate.accounts_with_errors += 1;
                    }
                }
                Err(err) => {
                    aggregate.accounts_with_errors += 1;
                    aggregate.errors.push(UiSyncError {
                        account_id,
                        email_address,
                        message: err.to_string(),
                    });
                }
            }
        }

        Ok(aggregate)
    }
    .await;

    let now = now_rfc3339();
    match result {
        Ok(aggregate) => {
            let status = if aggregate.accounts_with_errors > 0 {
                "partial"
            } else {
                "ok"
            };
            let status_text = format_last_sync_status_text(status, now.as_str());
            if let Ok(mut guard) = state.last_sync.lock() {
                let mut next = UiSyncStatus {
                    last_success_at: None,
                    error: aggregate.errors.first().map(|e| e.message.clone()),
                    sync_in_progress: false,
                    last_sync_at: Some(now.clone()),
                    last_sync_status: status_text.clone(),
                };
                next.preserve_success(&guard);
                *guard = next;
            }
            state.set_sync_in_progress(false);
            crate::background_sync::persist_sync_status(&db_path, &state);
            state.set_tray_status_text(&format!("Last sync: {status_text}"));
            let _ = app_handle.emit(EVENT_SYNC_STATUS_UPDATED, ());

            Ok(aggregate)
        }
        Err(err) => {
            let status_text = format_last_sync_status_text("error", now.as_str());
            if let Ok(mut guard) = state.last_sync.lock() {
                let mut next = UiSyncStatus {
                    last_success_at: None,
                    error: Some(err.clone()),
                    sync_in_progress: false,
                    last_sync_at: Some(now.clone()),
                    last_sync_status: status_text.clone(),
                };
                next.preserve_success(&guard);
                *guard = next;
            }
            state.set_sync_in_progress(false);
            crate::background_sync::persist_sync_status(&db_path, &state);
            state.set_tray_status_text(&format!("Last sync: {status_text}"));
            let _ = app_handle.emit(EVENT_SYNC_STATUS_UPDATED, ());
            Err(err)
        }
    }
}

#[tauri::command]
pub async fn search_messages(
    db_path: String,
    query: String,
) -> Result<Vec<UiSearchMessageRow>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let db_path = validate_db_path(&db_path)?;
        if query.len() > MAX_SEARCH_QUERY_LEN {
            return Err(format!(
                "search query too long (max {MAX_SEARCH_QUERY_LEN} characters)"
            ));
        }
        let storage = Storage::open_existing(&db_path).map_err(|e| e.to_string())?;
        let results = storage
            .search_message_blobs(&query, UI_SEARCH_LIMIT)
            .map_err(|e| e.to_string())?;

        Ok(results
            .into_iter()
            .map(|r| UiSearchMessageRow {
                id: r.id,
                subject: r.subject,
                from_address: r.from_address,
                date_header: r.date_header,
                snippet: r.snippet,
            })
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // Keep existing flat IPC parameters compatible.
pub async fn list_messages(
    db_path: String,
    account_id: Option<i64>,
    mailbox_name: Option<String>,
    query: String,
    limit: usize,
    offset: usize,
    sort_order: Option<String>,
    date_from: Option<i64>,
    date_to: Option<i64>,
    has_attachments: Option<bool>,
    after: Option<email_archiver_storage::MessageCursor>,
    before: Option<email_archiver_storage::MessageCursor>,
) -> Result<Vec<UiMessageRow>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let db_path = validate_db_path(&db_path)?;
        if query.len() > MAX_SEARCH_QUERY_LEN {
            return Err(format!(
                "search query too long (max {MAX_SEARCH_QUERY_LEN} characters)"
            ));
        }
        let sort_order = normalize_message_sort_order(sort_order.as_deref())?;
        let storage = Storage::open_existing(&db_path).map_err(|e| e.to_string())?;
        let rows = storage
            .query_message_locations(&email_archiver_storage::MessageQuery {
                account_id,
                mailbox_name: mailbox_name.as_deref(),
                user_query: &query,
                limit,
                offset,
                sort_order,
                date_from,
                date_to,
                has_attachments,
                after,
                before,
            })
            .map_err(|e| e.to_string())?;

        Ok(rows
            .into_iter()
            .map(|r| UiMessageRow {
                sort_timestamp: r.sort_timestamp,
                has_attachments: r.has_attachments,
                id: r.id,
                message_blob_id: r.message_blob_id,
                subject: r.subject,
                from_address: r.from_address,
                date_header: r.date_header,
                snippet: r.snippet,
                account_id: r.account_id,
                account_email: r.account_email_address,
                mailbox_id: r.mailbox_id,
                mailbox_name: r.mailbox_name,
            })
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn normalize_message_sort_order(sort_order: Option<&str>) -> Result<MessageListSortOrder, String> {
    let normalized = sort_order
        .unwrap_or(MESSAGE_SORT_NEWEST)
        .trim()
        .to_ascii_lowercase();
    match normalized.as_str() {
        MESSAGE_SORT_NEWEST => Ok(MessageListSortOrder::NewestFirst),
        MESSAGE_SORT_OLDEST => Ok(MessageListSortOrder::OldestFirst),
        _ => Err(format!(
            "sort_order must be '{}' or '{}'",
            MESSAGE_SORT_NEWEST, MESSAGE_SORT_OLDEST
        )),
    }
}

#[tauri::command]
pub async fn get_message_blob_raw_mime(
    db_path: String,
    message_blob_id: i64,
) -> Result<UiMessageBlobRaw, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let db_path = validate_db_path(&db_path)?;
        let storage = Storage::open_existing(&db_path).map_err(|e| e.to_string())?;
        let raw = storage
            .get_message_blob_raw_mime(message_blob_id)
            .map_err(|e| e.to_string())?;

        Ok(UiMessageBlobRaw {
            id: raw.id,
            sha256: raw.sha256,
            raw_mime_text: String::from_utf8_lossy(&raw.raw_mime).to_string(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn export_message_blob_eml(
    db_path: String,
    message_blob_id: i64,
    output_path: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let db_path = validate_db_path(&db_path)?;
        let output_path = validate_output_path(&output_path)?;
        let storage = Storage::open_existing(&db_path).map_err(|e| e.to_string())?;
        let raw = storage
            .get_message_blob_raw_mime(message_blob_id)
            .map_err(|e| e.to_string())?;
        create_parent_dir_if_needed(&output_path).map_err(|e| e.to_string())?;
        crate::output::atomic_export(&db_path, &output_path, |file| {
            use std::io::Write;
            file.write_all(&raw.raw_mime).map_err(|e| e.to_string())
        })?;

        // Audit-relevant: export event (no path is recorded).
        let _ = storage.append_event(&email_archiver_storage::InsertEventInput {
            occurred_at: now_rfc3339(),
            kind: EVENT_KIND_MESSAGE_EML_EXPORTED.to_string(),
            account_id: None,
            mailbox_id: None,
            message_blob_id: Some(message_blob_id),
            detail: r#"{"v":1}"#.to_string(),
        });

        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn remove_account(db_path: String, account_id: i64) -> Result<(), String> {
    let db_path = validate_db_path(&db_path)?;
    let storage = Storage::open_existing(&db_path).map_err(|e| e.to_string())?;

    // Look up the account email for the audit event before disabling.
    let email = storage
        .list_accounts()
        .ok()
        .and_then(|accts| accts.into_iter().find(|a| a.id == account_id))
        .map(|a| a.email_address)
        .unwrap_or_default();

    // Log account_removed event in the audit chain.
    let _ = storage.append_event(&InsertEventInput {
        occurred_at: now_rfc3339(),
        kind: EVENT_KIND_ACCOUNT_REMOVED.to_string(),
        account_id: Some(account_id),
        mailbox_id: None,
        message_blob_id: None,
        detail: format!(r#"{{"email":"{}"}}"#, escape_json_value(&email)),
    });

    storage
        .set_account_disabled(account_id, true)
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn map_ui_account(account: &email_archiver_storage::AccountRow) -> UiAccount {
    UiAccount {
        id: account.id,
        label: account.label.clone(),
        email_address: account.email_address.clone(),
        imap_host: account.imap_host.clone(),
        imap_port: account.imap_port,
        imap_tls: account.imap_tls,
        imap_username: account.imap_username.clone(),
        mailbox_selection_mode: account.mailbox_selection_mode.clone(),
        disabled: account.disabled,
        auth_kind: account.auth_kind.clone(),
        oauth_provider: account.oauth_provider.clone(),
    }
}

fn list_mailboxes_internal(storage: &Storage, account_id: i64) -> Result<Vec<UiMailbox>, String> {
    let rows = storage
        .list_mailboxes(account_id)
        .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|m| {
            let gobd_recommended = is_gobd_recommended(&m.imap_name);
            UiMailbox {
                id: m.id,
                account_id: m.account_id,
                imap_name: m.imap_name,
                sync_enabled: m.sync_enabled,
                hard_excluded: m.hard_excluded,
                gobd_recommended,
                last_seen_uid: m.last_seen_uid,
                last_sync_at: m.last_sync_at,
                last_error: m.last_error,
            }
        })
        .collect())
}

fn normalize_mailbox_selection_mode(mode: &str) -> Result<&'static str, String> {
    let mode_lower = mode.trim().to_ascii_lowercase();
    match mode_lower.as_str() {
        MAILBOX_SELECTION_MODE_AUTO => Ok(MAILBOX_SELECTION_MODE_AUTO),
        MAILBOX_SELECTION_MODE_MANUAL => Ok(MAILBOX_SELECTION_MODE_MANUAL),
        _ => Err("mailbox_selection_mode must be 'auto' or 'manual'".to_string()),
    }
}

async fn discover_mailboxes_for_credentials(
    input: &CreateAccountCommandInput,
) -> Result<Vec<DiscoveredMailbox>, String> {
    let settings = ImapConnectionSettings {
        host: input.imap_host.clone(),
        port: input.imap_port,
        use_tls: DEFAULT_IMAP_TLS,
        username: input.imap_username.clone(),
        password: input.password.clone(),
    };

    let mut session = email_archiver_adapters::imap::connect_and_login(&settings)
        .await
        .map_err(|e| e.to_string())?;
    let names = email_archiver_adapters::imap::list_mailboxes(&mut session)
        .await
        .map_err(|e| e.to_string())?;

    Ok(names.into_iter().map(map_discovered_mailbox).collect())
}

fn map_discovered_mailbox(name: Name) -> DiscoveredMailbox {
    let by_attributes = email_archiver_adapters::imap::is_hard_excluded_by_attributes(&name);
    let by_common_name = is_hard_excluded_by_common_name(name.name());
    let hard_excluded = by_attributes || by_common_name;

    DiscoveredMailbox {
        gobd_recommended: is_gobd_recommended(name.name()),
        imap_name: name.name().to_string(),
        delimiter: name.delimiter().map(|d| d.to_string()),
        attributes: Some(email_archiver_adapters::imap::name_attributes_to_string(
            name.attributes(),
        )),
        hard_excluded,
    }
}

/// Folders recommended for GobD-compliant archiving.
///
/// GobD (Grundsätze zur ordnungsmäßigen Führung und Aufbewahrung von Büchern,
/// Aufzeichnungen und Unterlagen in elektronischer Form) requires that all
/// tax-relevant correspondence is retained.  These folders typically contain
/// business-relevant emails.
///
/// Note: Drafts are excluded because GobD covers *sent* correspondence, not
/// unsent drafts.  A draft that becomes a sent message is captured via the
/// Sent folder.
const GOBD_RECOMMENDED_NAMES: &[&str] = &[
    "inbox",
    "eingang",
    "sent",
    "sent mail",
    "sent messages",
    "gesendet",
    "gesendete elemente",
    "archive",
    "archiv",
    "all mail",
    // Gmail variants (under [Gmail]/ prefix are matched via suffix below)
];

/// Return `true` if the mailbox name matches a GobD-recommended folder.
///
/// Matches case-insensitively against known folder names, and also checks the
/// last path segment for providers that use prefixed paths like `[Gmail]/Sent Mail`.
fn is_gobd_recommended(imap_name: &str) -> bool {
    let lower = imap_name.to_lowercase();

    // Direct match.
    if GOBD_RECOMMENDED_NAMES.contains(&lower.as_str()) {
        return true;
    }

    // Match the last path segment (handles "[Gmail]/Sent Mail", "INBOX/Sent", etc.).
    let last_segment = lower
        .rsplit_once('/')
        .or_else(|| lower.rsplit_once('.'))
        .map(|(_, seg)| seg)
        .unwrap_or(&lower);

    GOBD_RECOMMENDED_NAMES.contains(&last_segment)
}

fn default_sync_enabled_for_mailbox(
    mailbox_selection_mode: &str,
    imap_name: &str,
    hard_excluded: bool,
) -> bool {
    if hard_excluded {
        return false;
    }

    if mailbox_selection_mode == MAILBOX_SELECTION_MODE_AUTO {
        return true;
    }

    if DEFAULT_MANUAL_SYNC_ENABLED_INBOX && imap_name.eq_ignore_ascii_case("INBOX") {
        return true;
    }

    false
}

fn create_parent_dir_if_needed(path: &Path) -> std::io::Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(parent)?;
    Ok(())
}

/// Validate that a database path is safe to use.
///
/// Rejects empty paths and paths containing `..` segments to prevent
/// directory traversal attacks. Only `.sqlite3` / `.sqlite` / `.db`
/// extensions (or no extension) are accepted.
fn validate_db_path(raw: &str) -> Result<PathBuf, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("db_path is required".to_string());
    }

    let path = PathBuf::from(trimmed);

    // Reject path traversal sequences.
    for component in path.components() {
        if let std::path::Component::ParentDir = component {
            return Err("db_path must not contain '..' segments".to_string());
        }
    }

    // Must have a safe extension (or none at all).
    if let Some(ext) = path.extension() {
        let ext_lower = ext.to_string_lossy().to_lowercase();
        if !matches!(ext_lower.as_str(), "sqlite3" | "sqlite" | "db") {
            return Err("db_path must be a .sqlite3, .sqlite, or .db file".to_string());
        }
    }

    Ok(path)
}

/// Validate that an output path is safe for writing.
///
/// Rejects empty paths and paths containing `..` segments.
fn validate_output_path(raw: &str) -> Result<PathBuf, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("output_path is required".to_string());
    }

    let path = PathBuf::from(trimmed);

    for component in path.components() {
        if let std::path::Component::ParentDir = component {
            return Err("output_path must not contain '..' segments".to_string());
        }
    }

    Ok(path)
}

#[tauri::command]
pub async fn diagnose_database(db_path: String) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let db_path = validate_db_path(&db_path)?;
        let storage = Storage::open_existing(&db_path).map_err(|e| e.to_string())?;
        let diagnostic = storage.diagnose_database().map_err(|e| e.to_string())?;
        serde_json::to_value(&diagnostic).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn reset_mailbox_cursors(db_path: String, account_id: i64) -> Result<u64, String> {
    let db_path = validate_db_path(&db_path)?;
    let storage = Storage::open_existing(&db_path).map_err(|e| e.to_string())?;
    storage
        .reset_mailbox_cursors(account_id)
        .map_err(|e| e.to_string())
}

/// Return recent events from the tamper-evident audit log.
///
/// Returns `{ events: [...], total_count: N }` so the UI can show the total
/// and handle pagination.
///
/// `kind_filter` – if provided, only events with this `kind` are returned.
/// `limit`       – max rows (clamped to 500).
/// `offset`      – pagination offset.
#[tauri::command]
pub async fn list_events(
    db_path: String,
    kind_filter: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let db_path = validate_db_path(&db_path)?;
        let storage = Storage::open_existing(&db_path).map_err(|e| e.to_string())?;

        const MAX_LIMIT: usize = 500;
        let limit = limit.unwrap_or(100).min(MAX_LIMIT);
        let offset = offset.unwrap_or(0);

        let total_count = storage
            .event_count(kind_filter.as_deref())
            .map_err(|e| e.to_string())?;

        let events = storage
            .list_recent_events(kind_filter.as_deref(), limit, offset)
            .map_err(|e| e.to_string())?;

        let result = serde_json::json!({
            "events": events,
            "total_count": total_count,
        });
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Export all events as a CSV file.  Returns the path to the generated file.
#[tauri::command]
pub async fn export_events_csv(db_path: String, output_path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let db_path = validate_db_path(&db_path)?;
        let output_path_validated = validate_output_path(&output_path)?;
        let storage = Storage::open_existing(&db_path).map_err(|e| e.to_string())?;

        let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
        let snapshot = storage
            .snapshot_to(&dir.path().join("snapshot.db"))
            .map_err(|e| e.to_string())?;
        crate::output::atomic_export(&db_path, &output_path_validated, |file| {
            use std::io::Write;
            writeln!(
                file,
                "id,occurred_at,kind,account_id,mailbox_id,message_blob_id,detail,hash"
            )
            .map_err(|e| e.to_string())?;
            snapshot
                .visit_events_for_export(|event| {
                    writeln!(
                        file,
                        "{},{},{},{},{},{},{},{}",
                        event.id,
                        csv_escape(&event.occurred_at),
                        csv_escape(&event.kind),
                        event.account_id.map(|v| v.to_string()).unwrap_or_default(),
                        event.mailbox_id.map(|v| v.to_string()).unwrap_or_default(),
                        event
                            .message_blob_id
                            .map(|v| v.to_string())
                            .unwrap_or_default(),
                        csv_escape(event.detail.as_deref().unwrap_or("")),
                        csv_escape(&event.hash)
                    )?;
                    Ok(())
                })
                .map_err(|e| e.to_string())
        })?;

        Ok(output_path_validated.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Escape a value for CSV output (RFC 4180).
fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

#[tauri::command]
pub fn set_export_eml_menu_enabled(state: State<'_, AppState>, enabled: bool) {
    state.set_export_eml_enabled(enabled);
}

// ---------------------------------------------------------------------------
// Google OAuth commands
// ---------------------------------------------------------------------------

/// Built-in Google OAuth client credentials, optionally set at build time.
///
/// When set (via environment variables `GOOGLE_OAUTH_CLIENT_ID` and
/// `GOOGLE_OAUTH_CLIENT_SECRET` during compilation), users get a seamless
/// "Sign in with Google" experience without needing to create their own
/// Google Cloud project.
const EMBEDDED_GOOGLE_CLIENT_ID: Option<&str> = option_env!("GOOGLE_OAUTH_CLIENT_ID");
const EMBEDDED_GOOGLE_CLIENT_SECRET: Option<&str> = option_env!("GOOGLE_OAUTH_CLIENT_SECRET");

/// Return the embedded (compile-time) Google OAuth client config, if both
/// `GOOGLE_OAUTH_CLIENT_ID` and `GOOGLE_OAUTH_CLIENT_SECRET` were provided
/// as environment variables during the build.
fn embedded_google_client_config() -> Option<GoogleOAuthClientConfig> {
    let id = EMBEDDED_GOOGLE_CLIENT_ID?.trim();
    let secret = EMBEDDED_GOOGLE_CLIENT_SECRET?.trim();
    if id.is_empty() || secret.is_empty() {
        return None;
    }
    Some(GoogleOAuthClientConfig {
        client_id: id.to_string(),
        client_secret: secret.to_string(),
    })
}

/// Resolve Google OAuth client credentials.
///
/// Checks, in order:
/// 1. Keychain (user-provided or previously saved credentials)
/// 2. Embedded build-time defaults
///
/// When embedded defaults are used for the first time, they are automatically
/// saved to Keychain so that downstream code (e.g. token refresh during sync)
/// can find them without needing the embedded fallback.
fn ensure_google_client_configured(
    secret_store: &dyn SecretStore,
) -> Result<GoogleOAuthClientConfig, String> {
    // Try Keychain first.
    match oauth::load_google_client_config(secret_store) {
        Ok(config) => return Ok(config),
        Err(oauth::OAuthError::ClientNotConfigured) => {}
        Err(e) => return Err(e.to_string()),
    }

    // Fall back to embedded build-time defaults.
    let config = embedded_google_client_config()
        .ok_or_else(|| "Google OAuth credentials are not configured".to_string())?;

    // Save to Keychain so all downstream code works seamlessly.
    oauth::save_google_client_config(secret_store, &config).map_err(|e| e.to_string())?;

    Ok(config)
}

/// Input for setting Google OAuth client credentials.
///
/// Custom `Debug` impl redacts `client_secret`.
#[derive(Clone, Deserialize)]
pub struct SetGoogleOAuthClientInput {
    pub client_id: String,
    pub client_secret: String,
}

impl std::fmt::Debug for SetGoogleOAuthClientInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SetGoogleOAuthClientInput")
            .field("client_id", &self.client_id)
            .field("client_secret", &"[REDACTED]")
            .finish()
    }
}

/// Input for adding a Google account via OAuth.
#[derive(Debug, Clone, Deserialize)]
pub struct AddGoogleOAuthAccountInput {
    pub email: String,
    pub mailbox_selection_mode: String,
}

/// Returns whether Google OAuth client credentials are configured
/// (either in Keychain or embedded at build time).
#[tauri::command]
pub fn get_google_oauth_configured() -> Result<bool, String> {
    let secret_store = KeychainSecretStore::new();
    match oauth::load_google_client_config(&secret_store) {
        Ok(_) => return Ok(true),
        Err(oauth::OAuthError::ClientNotConfigured) => {}
        Err(e) => return Err(e.to_string()),
    }
    // Fall back to embedded defaults.
    Ok(embedded_google_client_config().is_some())
}

/// Returns whether the app has embedded (build-time) Google OAuth credentials.
#[tauri::command]
pub fn get_google_oauth_has_embedded() -> bool {
    embedded_google_client_config().is_some()
}

/// Store (or replace) the Google OAuth client credentials in Keychain.
#[tauri::command]
pub fn set_google_oauth_client(input: SetGoogleOAuthClientInput) -> Result<(), String> {
    let client_id = input.client_id.trim().to_string();
    let client_secret = input.client_secret.trim().to_string();

    if client_id.is_empty() {
        return Err("Client ID is required".to_string());
    }
    if client_secret.is_empty() {
        return Err("Client Secret is required".to_string());
    }

    let secret_store = KeychainSecretStore::new();
    oauth::save_google_client_config(
        &secret_store,
        &GoogleOAuthClientConfig {
            client_id,
            client_secret,
        },
    )
    .map_err(|e| e.to_string())
}

/// Run the full Google OAuth flow and create an account.
///
/// 1. Opens the browser for Google consent.
/// 2. Waits for the loopback redirect with the authorization code.
/// 3. Exchanges the code for access + refresh tokens.
/// 4. Connects to Gmail IMAP via XOAUTH2, discovers mailboxes.
/// 5. Creates the account and mailboxes in the database.
#[tauri::command]
pub async fn add_google_oauth_account(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    db_path: String,
    input: AddGoogleOAuthAccountInput,
    operation_id: String,
) -> Result<CreateAccountCommandResult, String> {
    use crate::app_state::OAuthOperation;
    uuid::Uuid::parse_str(&operation_id).map_err(|_| "Invalid authorization operation")?;
    let db_path = validate_db_path(&db_path)?.to_string_lossy().into_owned();
    let email = input.email.trim().to_string();
    if email.is_empty() {
        return Err("Email address is required".into());
    }
    let mode = normalize_mailbox_selection_mode(&input.mailbox_selection_mode)?;
    let (sender, mut cancellation) = tokio::sync::oneshot::channel();
    {
        let mut operations = state.oauth_operations.lock().map_err(|e| e.to_string())?;
        if operations.contains_key(&operation_id) {
            return Err("Authorization was canceled or already started".into());
        }
        if operations.len() > 512 {
            operations.retain(|_, v| matches!(v, OAuthOperation::Running(_)));
        }
        operations.insert(operation_id.clone(), OAuthOperation::Running(sender));
    }
    let secret_store = KeychainSecretStore::new();
    let secret_ref = format!("account:{}", uuid::Uuid::new_v4());
    let browser_operation = operation_id.clone();
    let report_browser = move |url: &str, error: Option<&str>| {
        let _ = app_handle.emit(
            "google_oauth_browser",
            serde_json::json!({"operation_id":browser_operation,"url":url,"launch_error":error}),
        );
    };
    let flow = async {
        let client = ensure_google_client_configured(&secret_store)?;
        let auth = oauth::google_authorize(
            &secret_store,
            &client,
            &email,
            &secret_ref,
            Some(&report_browser),
        )
        .await
        .map_err(|e| e.to_string())?;
        let mut session = email_archiver_adapters::imap::connect_and_authenticate_xoauth2(
            oauth::GOOGLE_IMAP_HOST,
            oauth::GOOGLE_IMAP_PORT,
            &auth.email,
            &auth.access_token,
        )
        .await
        .map_err(|e| e.to_string())?;
        let names = email_archiver_adapters::imap::list_mailboxes(&mut session)
            .await
            .map_err(|e| e.to_string())?;
        Ok::<_, String>((
            auth,
            names
                .into_iter()
                .map(map_discovered_mailbox)
                .collect::<Vec<_>>(),
        ))
    };
    let staged = tokio::select! {result=flow=>result,_=&mut cancellation=>Err("Authorization canceled. No account was added.".into())};
    let result = (|| -> Result<CreateAccountCommandResult, String> {
        let (auth, names) = staged?;
        // This lock serializes cancellation with credential + database commit.
        let mut operations = state.oauth_operations.lock().map_err(|e| e.to_string())?;
        if !matches!(
            operations.get(&operation_id),
            Some(OAuthOperation::Running(_))
        ) {
            return Err("Authorization canceled. No account was added.".into());
        }
        let storage = Storage::open_existing(&db_path).map_err(|e| e.to_string())?;
        let mailboxes = names
            .into_iter()
            .map(|m| UpsertMailboxInput {
                account_id: 0,
                sync_enabled: default_sync_enabled_for_mailbox(mode, &m.imap_name, m.hard_excluded),
                imap_name: m.imap_name,
                delimiter: m.delimiter,
                attributes: m.attributes,
                hard_excluded: m.hard_excluded,
                uidvalidity: None,
                last_seen_uid: 0,
            })
            .collect::<Vec<_>>();
        oauth::save_token_data(&secret_store, &secret_ref, &auth.tokens)
            .map_err(|e| e.to_string())?;
        let id = match storage.create_account_with_mailboxes(
            &CreateAccountInput {
                label: auth.email.clone(),
                email_address: auth.email.clone(),
                provider_kind: PROVIDER_KIND_GOOGLE_IMAP.into(),
                imap_host: oauth::GOOGLE_IMAP_HOST.into(),
                imap_port: oauth::GOOGLE_IMAP_PORT,
                imap_tls: true,
                imap_username: auth.email,
                auth_kind: AUTH_KIND_OAUTH2.into(),
                secret_ref: secret_ref.clone(),
                mailbox_selection_mode: mode.into(),
                oauth_provider: Some(OAUTH_PROVIDER_GOOGLE.into()),
                oauth_scopes: Some(GOOGLE_OAUTH_SCOPES.into()),
            },
            &mailboxes,
        ) {
            Ok(id) => id,
            Err(e) => {
                oauth::delete_token_data(&secret_store, &secret_ref);
                return Err(e.to_string());
            }
        };
        operations.insert(operation_id.clone(), OAuthOperation::Committed(id));
        let account = storage
            .list_accounts()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|a| a.id == id)
            .ok_or("Created account not found")?;
        Ok(CreateAccountCommandResult {
            account: map_ui_account(&account),
            mailboxes: list_mailboxes_internal(&storage, id)?,
        })
    })();
    if result.is_err() {
        if let Ok(mut operations) = state.oauth_operations.lock() {
            if !matches!(
                operations.get(&operation_id),
                Some(OAuthOperation::Committed(_))
            ) {
                operations.insert(operation_id, OAuthOperation::Cancelled);
            }
        }
    }
    result
}

#[tauri::command]
pub fn cancel_google_oauth(
    state: State<'_, AppState>,
    operation_id: String,
) -> Result<Option<i64>, String> {
    use crate::app_state::OAuthOperation;
    uuid::Uuid::parse_str(&operation_id).map_err(|_| "Invalid authorization operation")?;
    let mut operations = state.oauth_operations.lock().map_err(|e| e.to_string())?;
    if let Some(OAuthOperation::Committed(id)) = operations.get(&operation_id) {
        return Ok(Some(*id));
    }
    if let Some(OAuthOperation::Running(sender)) =
        operations.insert(operation_id, OAuthOperation::Cancelled)
    {
        let _ = sender.send(());
    }
    Ok(None)
}

// ---------------------------------------------------------------------------
// Archive stats (mail count + DB file size)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ArchiveStats {
    pub total_messages: u64,
    pub account_messages: Option<u64>,
    pub db_size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfigV1 {
    pub db_path: String,
    #[serde(default = "crate::configuration::default_interval")]
    pub sync_interval_secs: u64,
}

#[tauri::command]
pub async fn get_archive_stats(
    db_path: String,
    account_id: Option<i64>,
) -> Result<ArchiveStats, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let db_path = validate_db_path(&db_path)?;
        let storage = Storage::open_existing(&db_path).map_err(|e| e.to_string())?;
        let diag = storage.diagnose_database().map_err(|e| e.to_string())?;

        let total_messages = diag.message_blobs_count;

        let account_messages = match account_id {
            Some(aid) => {
                let count = storage
                    .count_message_locations_for_account(aid)
                    .map_err(|e| e.to_string())?;
                Some(count)
            }
            None => None,
        };

        let db_size_bytes = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);

        Ok(ArchiveStats {
            total_messages,
            account_messages,
            db_size_bytes,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_archive_date_range(
    db_path: String,
) -> Result<email_archiver_storage::ArchiveDateRange, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let db_path = validate_db_path(&db_path)?;
        let storage = Storage::open_existing(&db_path).map_err(|e| e.to_string())?;
        storage.get_archive_date_range().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn is_sync_folder_path(path: String) -> bool {
    let path_lower = path.trim().to_ascii_lowercase();
    if path_lower.is_empty() {
        return false;
    }

    const SYNC_FOLDER_MARKERS: [&str; 6] = [
        "/library/mobile documents/",
        "/icloud drive/",
        "/icloud/",
        "/dropbox/",
        "/onedrive/",
        "/googledrive/",
    ];
    SYNC_FOLDER_MARKERS
        .iter()
        .any(|marker| path_lower.contains(marker))
}

#[tauri::command]
pub fn get_app_config(app_handle: AppHandle) -> Result<Option<AppConfigV1>, String> {
    crate::configuration::load(&resolve_app_config_path(&app_handle)?).map(|(config, _)| {
        config.map(|c| AppConfigV1 {
            db_path: c.db_path,
            sync_interval_secs: c.sync_interval_secs,
        })
    })
}

#[tauri::command]
pub fn save_app_config(app_handle: AppHandle, config: AppConfigV1) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    let _guard = state.config_lock.lock().map_err(|e| e.to_string())?;
    let db_path = validate_db_path(&config.db_path)?;
    let storage = Storage::open_existing(&db_path).map_err(|e| e.to_string())?;
    crate::configuration::save(
        &resolve_app_config_path(&app_handle)?,
        &crate::configuration::AppConfig {
            db_path: db_path.to_string_lossy().into(),
            sync_interval_secs: config.sync_interval_secs,
        },
    )?;
    storage
        .set_sync_interval_secs(config.sync_interval_secs)
        .map_err(|e| e.to_string())?;
    state.set_sync_interval_secs(config.sync_interval_secs);
    Ok(())
}

#[tauri::command]
pub fn clear_app_config(app_handle: AppHandle) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    let _guard = state.config_lock.lock().map_err(|e| e.to_string())?;
    crate::configuration::clear(&resolve_app_config_path(&app_handle)?)
}

#[tauri::command]
pub async fn select_archive(
    app_handle: AppHandle,
    db_path: String,
    create: bool,
) -> Result<AppConfigV1, String> {
    let _sync_guard = app_handle.state::<AppState>();
    let _sync_guard = _sync_guard.sync_lock.lock().await;
    let validated = validate_db_path(&db_path)?;
    let worker_path = validated.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if create {
            Storage::create_new(worker_path)
        } else {
            Storage::open_existing(worker_path)
        }
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let config = AppConfigV1 {
        db_path: validated.to_string_lossy().into(),
        sync_interval_secs: app_handle.state::<AppState>().sync_interval_secs(),
    };
    // Hold writer ownership before committing a remembered path.
    crate::bootstrap::activate_with(&app_handle, &config.db_path, || {
        save_app_config(app_handle.clone(), config.clone())
    })?;
    crate::bootstrap::verify_async(app_handle.clone());
    Ok(config)
}

#[tauri::command]
pub fn get_startup_warning(state: State<'_, AppState>) -> Option<String> {
    state.startup_warning.lock().ok().and_then(|s| s.clone())
}

#[tauri::command]
pub fn frontend_ready(app_handle: AppHandle) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    state
        .frontend_ready
        .store(true, std::sync::atomic::Ordering::SeqCst);
    for action in state
        .pending_actions
        .lock()
        .map_err(|e| e.to_string())?
        .drain(..)
    {
        let _ = app_handle.emit(&action, ());
    }
    Ok(())
}

#[tauri::command]
pub fn get_integrity_status(
    state: State<'_, AppState>,
) -> Result<Option<email_archiver_storage::IntegrityStatus>, String> {
    let guard = state
        .integrity_status
        .lock()
        .map_err(|_| "internal error: mutex poisoned".to_string())?;
    Ok(guard.clone())
}

// ---------------------------------------------------------------------------

fn now_rfc3339() -> String {
    let now = time::OffsetDateTime::now_utc();
    now.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

pub(crate) fn resolve_app_config_path(app_handle: &AppHandle) -> Result<PathBuf, String> {
    #[cfg(debug_assertions)]
    if let Some(directory) = std::env::var_os("AMBERIZE_TEST_CONFIG_DIR") {
        return Ok(PathBuf::from(directory).join("config.json"));
    }
    let config_dir = app_handle
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?;
    Ok(config_dir.join("config.json"))
}

/// Minimal JSON string escaping for values interpolated into detail JSON.
fn escape_json_value(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

#[cfg(test)]
mod image_tests {
    use super::*;
    #[test]
    fn tiny_compressed_images_cannot_announce_unbounded_pixel_allocations() {
        let mut png = vec![0u8; 24];
        png[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        png[16..20].copy_from_slice(&100000_u32.to_be_bytes());
        png[20..24].copy_from_slice(&100000_u32.to_be_bytes());
        assert!(!image_within_pixel_limit(&png));
        png[16..20].copy_from_slice(&100_u32.to_be_bytes());
        png[20..24].copy_from_slice(&100_u32.to_be_bytes());
        assert!(image_within_pixel_limit(&png));
        assert!(!image_within_pixel_limit(b"unknown format"));
    }
}

#[cfg(test)]
mod archive_restore_tests {
    use super::*;
    use std::time::Duration;

    fn run(future: impl std::future::Future<Output = ()>) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future);
    }

    fn fixture() -> (tempfile::TempDir, PathBuf, AppState) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("archive.sqlite3");
        let file = std::fs::File::create(&path).unwrap();
        let state = AppState::default();
        *state.active_db_path.lock().unwrap() =
            Some(path.canonicalize().unwrap().to_string_lossy().into_owned());
        *state.archive_lock.lock().unwrap() = Some(file);
        *state.startup_warning.lock().unwrap() = Some("preserved warning".into());
        state.set_sync_in_progress(true);
        state.last_sync.lock().unwrap().last_sync_status = "Syncing".into();
        (directory, path, state)
    }

    fn assert_preserved(state: &AppState, path: &Path) {
        assert_eq!(
            state.active_db_path.lock().unwrap().as_deref(),
            path.canonicalize().unwrap().to_str()
        );
        assert!(state.archive_lock.lock().unwrap().is_some());
        assert_eq!(
            state.startup_warning.lock().unwrap().as_deref(),
            Some("preserved warning")
        );
        assert!(state.sync_in_progress());
        assert_eq!(state.last_sync.lock().unwrap().last_sync_status, "Syncing");
    }

    #[test]
    fn same_archive_restores_while_sync_lock_is_held() {
        run(async {
            let (_directory, path, state) = fixture();
            let _sync_guard = state.sync_lock.lock().await;
            let (canonical, guard) = tokio::time::timeout(
                Duration::from_millis(100),
                archive_activation_guard(&state, path.to_str().unwrap()),
            )
            .await
            .expect("restoring the active archive must not wait for sync")
            .unwrap();
            assert_eq!(canonical, path.canonicalize().unwrap());
            assert!(guard.is_none());
            assert_preserved(&state, &path);
            assert!(
                tokio::time::timeout(Duration::from_millis(5), state.sync_wakeup.notified())
                    .await
                    .is_err()
            );
        });
    }

    #[test]
    fn different_archive_waits_for_sync_before_activation() {
        run(async {
            let (directory, path, state) = fixture();
            let other = directory.path().join("other.sqlite3");
            std::fs::File::create(&other).unwrap();
            let sync_guard = state.sync_lock.lock().await;
            let activation = archive_activation_guard(&state, other.to_str().unwrap());
            tokio::pin!(activation);
            assert!(
                tokio::time::timeout(Duration::from_millis(10), activation.as_mut())
                    .await
                    .is_err()
            );
            assert_preserved(&state, &path);
            drop(sync_guard);
            let (canonical, guard) = tokio::time::timeout(Duration::from_millis(100), activation)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(canonical, other.canonicalize().unwrap());
            assert!(guard.is_some());
        });
    }

    #[test]
    fn missing_archive_fails_promptly_and_preserves_active_archive() {
        run(async {
            let (directory, path, state) = fixture();
            let _sync_guard = state.sync_lock.lock().await;
            let missing = directory.path().join("missing.sqlite3");
            let error = tokio::time::timeout(
                Duration::from_millis(100),
                archive_activation_guard(&state, missing.to_str().unwrap()),
            )
            .await
            .expect("missing archives must not wait for sync")
            .unwrap_err();
            assert!(error.contains("saved archive is unavailable"));
            assert!(!missing.exists());
            assert_preserved(&state, &path);
        });
    }

    #[cfg(unix)]
    #[test]
    fn symlink_to_active_archive_does_not_wait_for_sync() {
        run(async {
            let (directory, path, state) = fixture();
            let alias = directory.path().join("alias.sqlite3");
            std::os::unix::fs::symlink(&path, &alias).unwrap();
            let _sync_guard = state.sync_lock.lock().await;
            let (canonical, guard) = tokio::time::timeout(
                Duration::from_millis(100),
                archive_activation_guard(&state, alias.to_str().unwrap()),
            )
            .await
            .expect("canonical aliases must not wait for sync")
            .unwrap();
            assert_eq!(canonical, path.canonicalize().unwrap());
            assert!(guard.is_none());
            assert_preserved(&state, &path);
        });
    }

    // APFS rejects these filenames before path resolution; Linux permits them.
    #[cfg(target_os = "linux")]
    #[test]
    fn non_unicode_target_is_not_mistaken_for_an_active_archive() {
        use std::os::unix::ffi::OsStringExt;
        run(async {
            let directory = tempfile::tempdir().unwrap();
            let target = directory.path().join(std::ffi::OsString::from_vec(
                b"archive-\xff.sqlite3".to_vec(),
            ));
            std::fs::File::create(&target).unwrap();
            let alias = directory.path().join("alias.sqlite3");
            std::os::unix::fs::symlink(&target, &alias).unwrap();
            let state = AppState::default();
            let _sync_guard = state.sync_lock.lock().await;
            let error = tokio::time::timeout(
                Duration::from_millis(100),
                archive_activation_guard(&state, alias.to_str().unwrap()),
            )
            .await
            .unwrap()
            .unwrap_err();
            assert!(error.contains("valid Unicode"));
            assert!(state.active_db_path.lock().unwrap().is_none());
        });
    }

    #[test]
    fn rechecks_active_path_after_waiting_for_sync() {
        run(async {
            let (directory, path, state) = fixture();
            let other = directory.path().join("other.sqlite3");
            std::fs::File::create(&other).unwrap();
            let sync_guard = state.sync_lock.lock().await;
            let activation = archive_activation_guard(&state, other.to_str().unwrap());
            tokio::pin!(activation);
            assert!(
                tokio::time::timeout(Duration::from_millis(10), activation.as_mut())
                    .await
                    .is_err()
            );
            assert_preserved(&state, &path);
            *state.active_db_path.lock().unwrap() =
                Some(other.canonicalize().unwrap().to_string_lossy().into_owned());
            drop(sync_guard);
            let (_, guard) = tokio::time::timeout(Duration::from_millis(100), activation)
                .await
                .unwrap()
                .unwrap();
            assert!(guard.is_none(), "activation became redundant while waiting");
        });
    }
}

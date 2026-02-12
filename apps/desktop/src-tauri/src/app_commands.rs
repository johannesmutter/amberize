use std::path::{Path, PathBuf};

use email_archiver_adapters::{
    imap::{ImapConnectionSettings, Name},
    is_hard_excluded_by_common_name,
    oauth::{self, GoogleOAuthClientConfig},
    sync_account_once_with_progress, KeychainSecretStore, SecretStore, SyncProgressFn,
};
use email_archiver_storage::{
    CreateAccountInput, InsertEventInput, Storage, UpsertMailboxInput, AUTH_KIND_OAUTH2,
    AUTH_KIND_PASSWORD, OAUTH_PROVIDER_GOOGLE, PROVIDER_KIND_CLASSIC_IMAP,
    PROVIDER_KIND_GOOGLE_IMAP,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use tauri_plugin_autostart::ManagerExt as AutostartManagerExt;

use crate::app_state::{AppState, UiSyncStatus};
use crate::sync_status_text::format_last_sync_status_text;

const MAILBOX_SELECTION_MODE_AUTO: &str = "auto";
const MAILBOX_SELECTION_MODE_MANUAL: &str = "manual";

const DEFAULT_IMAP_TLS: bool = true;
const DEFAULT_MANUAL_SYNC_ENABLED_INBOX: bool = true;

const UI_SEARCH_LIMIT: usize = 50;
const MAX_SEARCH_QUERY_LEN: usize = 1000;

const EVENT_KIND_MESSAGE_EML_EXPORTED: &str = "message_eml_exported";
const EVENT_KIND_ACCOUNT_CREATED: &str = "account_created";
const EVENT_KIND_ACCOUNT_REMOVED: &str = "account_removed";
const EVENT_KIND_MAILBOX_SYNC_CHANGED: &str = "mailbox_sync_changed";
const EVENT_SYNC_STATUS_UPDATED: &str = "sync_status_updated";
const EVENT_SYNC_PROGRESS: &str = "sync_progress";

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
            resolved = resolved.replace(&format!("cid:{}", cid), data_uri);
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
pub fn get_message_detail(
    db_path: String,
    message_blob_id: i64,
) -> Result<UiMessageDetail, String> {
    let db_path = validate_db_path(&db_path)?;
    let storage = Storage::open_or_create(&db_path).map_err(|e| e.to_string())?;
    let raw = storage
        .get_message_blob_raw_mime(message_blob_id)
        .map_err(|e| e.to_string())?;

    Ok(parse_mime_to_detail(raw.id, raw.sha256, &raw.raw_mime))
}

#[tauri::command]
pub fn autostart_is_enabled(app_handle: AppHandle) -> Result<bool, String> {
    let autostart_manager = app_handle.autolaunch();
    autostart_manager.is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn autostart_set_enabled(app_handle: AppHandle, enabled: bool) -> Result<(), String> {
    let autostart_manager = app_handle.autolaunch();
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

    let secret_store = KeychainSecretStore::new();
    secret_store
        .set_secret(&secret_ref, &input.password)
        .map_err(|e| e.to_string())?;

    let storage = Storage::open_or_create(&db_path).map_err(|e| e.to_string())?;

    let account_email = input.email_address.clone();
    let account_imap_host = input.imap_host.clone();

    let account_id = storage
        .create_account(&CreateAccountInput {
            label: input.label,
            email_address: input.email_address,
            provider_kind: PROVIDER_KIND_CLASSIC_IMAP.to_string(),
            imap_host: input.imap_host,
            imap_port: input.imap_port,
            imap_tls: DEFAULT_IMAP_TLS,
            imap_username: input.imap_username,
            auth_kind: AUTH_KIND_PASSWORD.to_string(),
            secret_ref,
            mailbox_selection_mode: mailbox_selection_mode.to_string(),
            oauth_provider: None,
            oauth_scopes: None,
        })
        .map_err(|e| e.to_string())?;

    for mailbox in server_mailboxes {
        let hard_excluded = mailbox.hard_excluded;
        let sync_enabled = default_sync_enabled_for_mailbox(
            mailbox_selection_mode,
            mailbox.imap_name.as_str(),
            hard_excluded,
        );

        let _ = storage
            .upsert_mailbox(&UpsertMailboxInput {
                account_id,
                imap_name: mailbox.imap_name,
                delimiter: mailbox.delimiter,
                attributes: mailbox.attributes,
                sync_enabled,
                hard_excluded,
                uidvalidity: None,
                last_seen_uid: 0,
            })
            .map_err(|e| e.to_string())?;
    }

    // Log account_created event in the audit chain.
    let _ = storage.append_event(&InsertEventInput {
        occurred_at: now_rfc3339(),
        kind: EVENT_KIND_ACCOUNT_CREATED.to_string(),
        account_id: Some(account_id),
        mailbox_id: None,
        message_blob_id: None,
        detail: format!(
            r#"{{"email":"{}","imap_host":"{}"}}"#,
            escape_json_value(&account_email),
            escape_json_value(&account_imap_host),
        ),
    });

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
pub fn list_accounts(db_path: String) -> Result<Vec<UiAccount>, String> {
    let db_path = validate_db_path(&db_path)?;
    let storage = Storage::open_or_create(&db_path).map_err(|e| e.to_string())?;
    let accounts = storage.list_accounts().map_err(|e| e.to_string())?;
    Ok(accounts.iter().map(map_ui_account).collect())
}

#[tauri::command]
pub fn list_mailboxes(db_path: String, account_id: i64) -> Result<Vec<UiMailbox>, String> {
    let db_path = validate_db_path(&db_path)?;
    let storage = Storage::open_or_create(&db_path).map_err(|e| e.to_string())?;
    list_mailboxes_internal(&storage, account_id)
}

#[tauri::command]
pub fn set_mailbox_sync_enabled(
    db_path: String,
    mailbox_id: i64,
    sync_enabled: bool,
) -> Result<(), String> {
    let db_path = validate_db_path(&db_path)?;
    let storage = Storage::open_or_create(&db_path).map_err(|e| e.to_string())?;

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

    let storage = Storage::open_or_create(&db_path).map_err(|e| e.to_string())?;
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

#[tauri::command]
pub fn set_active_db_path(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    db_path: String,
) -> Result<(), String> {
    let validated = validate_db_path(&db_path)?;

    {
        let mut guard = state
            .active_db_path
            .lock()
            .map_err(|_| "internal error: mutex poisoned".to_string())?;
        *guard = Some(validated.to_string_lossy().to_string());
    }

    // Record app startup and detect any coverage gaps now that we know
    // which database to use.
    crate::background_sync::record_startup_and_detect_gaps(&app_handle);

    // Run integrity verification against the event chain and root hash.
    crate::background_sync::verify_integrity_at_startup(&app_handle);

    let last = state
        .last_sync
        .lock()
        .map_err(|_| "internal error: mutex poisoned".to_string())?
        .clone();
    state.set_tray_status_text(&format!("Last sync: {}", last.last_sync_status));

    let _ = app_handle.emit(EVENT_SYNC_STATUS_UPDATED, ());
    Ok(())
}

#[tauri::command]
pub fn clear_active_db_path(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    {
        let mut guard = state
            .active_db_path
            .lock()
            .map_err(|_| "internal error: mutex poisoned".to_string())?;
        *guard = None;
    }

    {
        let mut guard = state
            .last_sync
            .lock()
            .map_err(|_| "internal error: mutex poisoned".to_string())?;
        *guard = UiSyncStatus {
            sync_in_progress: false,
            last_sync_at: None,
            last_sync_status: "not configured".to_string(),
        };
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
pub fn set_sync_interval(state: State<'_, AppState>, interval_secs: u64) -> Result<(), String> {
    const MIN_INTERVAL_SECS: u64 = 60;
    if interval_secs < MIN_INTERVAL_SECS {
        return Err(format!(
            "interval must be at least {MIN_INTERVAL_SECS} seconds"
        ));
    }
    state.set_sync_interval_secs(interval_secs);
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
    state.set_sync_in_progress(true);
    state.set_tray_status_text("Status: syncing…");
    let _ = app_handle.emit(EVENT_SYNC_STATUS_UPDATED, ());

    let progress_handle = app_handle.clone();
    let on_progress: SyncProgressFn = Box::new(move |p| {
        let _ = progress_handle.emit(EVENT_SYNC_PROGRESS, p);
    });

    let result = async {
        let storage = Storage::open_or_create(&db_path).map_err(|e| e.to_string())?;
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
                *guard = UiSyncStatus {
                    sync_in_progress: false,
                    last_sync_at: Some(now.clone()),
                    last_sync_status: status_text.clone(),
                };
            }
            state.set_sync_in_progress(false);
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
                *guard = UiSyncStatus {
                    sync_in_progress: false,
                    last_sync_at: Some(now.clone()),
                    last_sync_status: status_text.clone(),
                };
            }
            state.set_sync_in_progress(false);
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
    state.set_sync_in_progress(true);
    state.set_tray_status_text("Status: syncing…");
    let _ = app_handle.emit(EVENT_SYNC_STATUS_UPDATED, ());

    let progress_handle = app_handle.clone();
    let on_progress: SyncProgressFn = Box::new(move |p| {
        let _ = progress_handle.emit(EVENT_SYNC_PROGRESS, p);
    });

    let result = async {
        let storage = Storage::open_or_create(&db_path).map_err(|e| e.to_string())?;
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
                *guard = UiSyncStatus {
                    sync_in_progress: false,
                    last_sync_at: Some(now.clone()),
                    last_sync_status: status_text.clone(),
                };
            }
            state.set_sync_in_progress(false);
            state.set_tray_status_text(&format!("Last sync: {status_text}"));
            let _ = app_handle.emit(EVENT_SYNC_STATUS_UPDATED, ());

            Ok(aggregate)
        }
        Err(err) => {
            let status_text = format_last_sync_status_text("error", now.as_str());
            if let Ok(mut guard) = state.last_sync.lock() {
                *guard = UiSyncStatus {
                    sync_in_progress: false,
                    last_sync_at: Some(now.clone()),
                    last_sync_status: status_text.clone(),
                };
            }
            state.set_sync_in_progress(false);
            state.set_tray_status_text(&format!("Last sync: {status_text}"));
            let _ = app_handle.emit(EVENT_SYNC_STATUS_UPDATED, ());
            Err(err)
        }
    }
}

#[tauri::command]
pub fn search_messages(db_path: String, query: String) -> Result<Vec<UiSearchMessageRow>, String> {
    let db_path = validate_db_path(&db_path)?;
    if query.len() > MAX_SEARCH_QUERY_LEN {
        return Err(format!(
            "search query too long (max {MAX_SEARCH_QUERY_LEN} characters)"
        ));
    }
    let storage = Storage::open_or_create(&db_path).map_err(|e| e.to_string())?;
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
}

#[tauri::command]
pub fn list_messages(
    db_path: String,
    account_id: Option<i64>,
    mailbox_name: Option<String>,
    query: String,
    limit: usize,
    offset: usize,
) -> Result<Vec<UiMessageRow>, String> {
    let db_path = validate_db_path(&db_path)?;
    if query.len() > MAX_SEARCH_QUERY_LEN {
        return Err(format!(
            "search query too long (max {MAX_SEARCH_QUERY_LEN} characters)"
        ));
    }
    let storage = Storage::open_or_create(&db_path).map_err(|e| e.to_string())?;
    let rows = storage
        .list_message_location_rows(account_id, mailbox_name.as_deref(), &query, limit, offset)
        .map_err(|e| e.to_string())?;

    Ok(rows
        .into_iter()
        .map(|r| UiMessageRow {
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
}

#[tauri::command]
pub fn get_message_blob_raw_mime(
    db_path: String,
    message_blob_id: i64,
) -> Result<UiMessageBlobRaw, String> {
    let db_path = validate_db_path(&db_path)?;
    let storage = Storage::open_or_create(&db_path).map_err(|e| e.to_string())?;
    let raw = storage
        .get_message_blob_raw_mime(message_blob_id)
        .map_err(|e| e.to_string())?;

    Ok(UiMessageBlobRaw {
        id: raw.id,
        sha256: raw.sha256,
        raw_mime_text: String::from_utf8_lossy(&raw.raw_mime).to_string(),
    })
}

#[tauri::command]
pub fn export_message_blob_eml(
    db_path: String,
    message_blob_id: i64,
    output_path: String,
) -> Result<(), String> {
    let db_path = validate_db_path(&db_path)?;
    let output_path = validate_output_path(&output_path)?;
    let storage = Storage::open_or_create(&db_path).map_err(|e| e.to_string())?;
    let raw = storage
        .get_message_blob_raw_mime(message_blob_id)
        .map_err(|e| e.to_string())?;
    create_parent_dir_if_needed(&output_path).map_err(|e| e.to_string())?;
    std::fs::write(&output_path, raw.raw_mime).map_err(|e| e.to_string())?;

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
}

#[tauri::command]
pub fn remove_account(db_path: String, account_id: i64) -> Result<(), String> {
    let db_path = validate_db_path(&db_path)?;
    let storage = Storage::open_or_create(&db_path).map_err(|e| e.to_string())?;

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
pub fn diagnose_database(db_path: String) -> Result<serde_json::Value, String> {
    let db_path = validate_db_path(&db_path)?;
    let storage = Storage::open_or_create(&db_path).map_err(|e| e.to_string())?;
    let diagnostic = storage.diagnose_database().map_err(|e| e.to_string())?;
    serde_json::to_value(&diagnostic).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn reset_mailbox_cursors(db_path: String, account_id: i64) -> Result<u64, String> {
    let db_path = validate_db_path(&db_path)?;
    let storage = Storage::open_or_create(&db_path).map_err(|e| e.to_string())?;
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
pub fn list_events(
    db_path: String,
    kind_filter: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<serde_json::Value, String> {
    let db_path = validate_db_path(&db_path)?;
    let storage = Storage::open_or_create(&db_path).map_err(|e| e.to_string())?;

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
}

/// Export all events as a CSV file.  Returns the path to the generated file.
#[tauri::command]
pub fn export_events_csv(db_path: String, output_path: String) -> Result<String, String> {
    let db_path = validate_db_path(&db_path)?;
    let output_path_validated = validate_output_path(&output_path)?;
    let storage = Storage::open_or_create(&db_path).map_err(|e| e.to_string())?;

    let total = storage.event_count(None).map_err(|e| e.to_string())?;

    let mut file = std::fs::File::create(&output_path_validated)
        .map_err(|e| format!("cannot create file: {e}"))?;

    use std::io::Write;
    writeln!(
        file,
        "id,occurred_at,kind,account_id,mailbox_id,message_blob_id,detail,hash"
    )
    .map_err(|e| e.to_string())?;

    let page_size = 500;
    let mut offset = 0usize;

    while (offset as u64) < total {
        let events = storage
            .list_recent_events(None, page_size, offset)
            .map_err(|e| e.to_string())?;

        if events.is_empty() {
            break;
        }

        for event in &events {
            writeln!(
                file,
                "{},{},{},{},{},{},{},{}",
                event.id,
                csv_escape(&event.occurred_at),
                csv_escape(&event.kind),
                event
                    .account_id
                    .map(|id| id.to_string())
                    .unwrap_or_default(),
                event
                    .mailbox_id
                    .map(|id| id.to_string())
                    .unwrap_or_default(),
                event
                    .message_blob_id
                    .map(|id| id.to_string())
                    .unwrap_or_default(),
                csv_escape(event.detail.as_deref().unwrap_or("")),
                csv_escape(&event.hash),
            )
            .map_err(|e| e.to_string())?;
        }

        offset += events.len();
    }

    Ok(output_path_validated.to_string_lossy().to_string())
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
    db_path: String,
    input: AddGoogleOAuthAccountInput,
) -> Result<CreateAccountCommandResult, String> {
    let db_path = validate_db_path(&db_path)?.to_string_lossy().to_string();
    let email = input.email.trim().to_string();
    if email.is_empty() {
        return Err("Email address is required".to_string());
    }
    let mailbox_selection_mode = normalize_mailbox_selection_mode(&input.mailbox_selection_mode)?;

    let secret_store = KeychainSecretStore::new();

    // Resolve Google OAuth client config (Keychain → embedded defaults).
    let client_config = ensure_google_client_configured(&secret_store)?;

    // Generate a unique secret_ref for this account's tokens.
    let secret_ref = format!("account:{}", uuid::Uuid::new_v4());

    // Run the OAuth authorization flow (opens browser, waits for callback).
    // After this succeeds, tokens are stored in Keychain under `secret_ref`.
    // If any subsequent step fails, we must clean up the orphaned Keychain
    // entry to avoid accumulating leaked credentials.
    let auth_result = oauth::google_authorize(&secret_store, &client_config, &email, &secret_ref)
        .await
        .map_err(|e| e.to_string())?;

    match create_google_account_after_auth(
        &secret_store,
        &db_path,
        &email,
        &auth_result.access_token,
        &secret_ref,
        mailbox_selection_mode,
    )
    .await
    {
        Ok(result) => Ok(result),
        Err(e) => {
            // Clean up the orphaned token data in Keychain so we don't
            // leave credentials for an account that was never created.
            oauth::delete_token_data(&secret_store, &secret_ref);
            Err(e)
        }
    }
}

/// Inner helper for [`add_google_oauth_account`] that runs after the OAuth
/// browser flow succeeds.  Separated so the caller can clean up the Keychain
/// entry if this function fails.
async fn create_google_account_after_auth(
    _secret_store: &dyn email_archiver_adapters::SecretStore,
    db_path: &str,
    email: &str,
    access_token: &str,
    secret_ref: &str,
    mailbox_selection_mode: &str,
) -> Result<CreateAccountCommandResult, String> {
    // Connect to Gmail IMAP with XOAUTH2 and discover mailboxes.
    let mut session = email_archiver_adapters::imap::connect_and_authenticate_xoauth2(
        oauth::GOOGLE_IMAP_HOST,
        oauth::GOOGLE_IMAP_PORT,
        email,
        access_token,
    )
    .await
    .map_err(|e| format!("Gmail IMAP connection failed: {e}"))?;

    let names = email_archiver_adapters::imap::list_mailboxes(&mut session)
        .await
        .map_err(|e| e.to_string())?;

    let server_mailboxes: Vec<DiscoveredMailbox> =
        names.into_iter().map(map_discovered_mailbox).collect();

    // Create the account in the database.
    let storage = Storage::open_or_create(db_path).map_err(|e| e.to_string())?;

    let account_id = storage
        .create_account(&CreateAccountInput {
            label: email.to_string(),
            email_address: email.to_string(),
            provider_kind: PROVIDER_KIND_GOOGLE_IMAP.to_string(),
            imap_host: oauth::GOOGLE_IMAP_HOST.to_string(),
            imap_port: oauth::GOOGLE_IMAP_PORT,
            imap_tls: true,
            imap_username: email.to_string(),
            auth_kind: AUTH_KIND_OAUTH2.to_string(),
            secret_ref: secret_ref.to_string(),
            mailbox_selection_mode: mailbox_selection_mode.to_string(),
            oauth_provider: Some(OAUTH_PROVIDER_GOOGLE.to_string()),
            oauth_scopes: Some(GOOGLE_OAUTH_SCOPES.to_string()),
        })
        .map_err(|e| e.to_string())?;

    // Insert discovered mailboxes.
    for mailbox in server_mailboxes {
        let hard_excluded = mailbox.hard_excluded;
        let sync_enabled = default_sync_enabled_for_mailbox(
            mailbox_selection_mode,
            mailbox.imap_name.as_str(),
            hard_excluded,
        );

        let _ = storage
            .upsert_mailbox(&UpsertMailboxInput {
                account_id,
                imap_name: mailbox.imap_name,
                delimiter: mailbox.delimiter,
                attributes: mailbox.attributes,
                sync_enabled,
                hard_excluded,
                uidvalidity: None,
                last_seen_uid: 0,
            })
            .map_err(|e| e.to_string())?;
    }

    // Log account_created event in the audit chain.
    let _ = storage.append_event(&InsertEventInput {
        occurred_at: now_rfc3339(),
        kind: EVENT_KIND_ACCOUNT_CREATED.to_string(),
        account_id: Some(account_id),
        mailbox_id: None,
        message_blob_id: None,
        detail: format!(
            r#"{{"email":"{}","imap_host":"{}","provider":"google"}}"#,
            escape_json_value(email),
            escape_json_value(oauth::GOOGLE_IMAP_HOST),
        ),
    });

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
}

#[tauri::command]
pub fn get_archive_stats(db_path: String, account_id: Option<i64>) -> Result<ArchiveStats, String> {
    let db_path = validate_db_path(&db_path)?;
    let storage = Storage::open_or_create(&db_path).map_err(|e| e.to_string())?;
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
}

#[tauri::command]
pub fn get_archive_date_range(
    db_path: String,
) -> Result<email_archiver_storage::ArchiveDateRange, String> {
    let db_path = validate_db_path(&db_path)?;
    let storage = Storage::open_or_create(&db_path).map_err(|e| e.to_string())?;
    storage.get_archive_date_range().map_err(|e| e.to_string())
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
    let path = resolve_app_config_path(&app_handle)?;
    if !path.exists() {
        return Ok(None);
    }

    let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    if raw.trim().is_empty() {
        return Ok(None);
    }
    let parsed: AppConfigV1 = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    if parsed.db_path.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(parsed))
}

#[tauri::command]
pub fn save_app_config(app_handle: AppHandle, config: AppConfigV1) -> Result<(), String> {
    let db_path = validate_db_path(&config.db_path)?;
    let mut normalized = config;
    normalized.db_path = db_path.to_string_lossy().to_string();

    let path = resolve_app_config_path(&app_handle)?;
    create_parent_dir_if_needed(&path).map_err(|e| e.to_string())?;
    let serialized = serde_json::to_string_pretty(&normalized).map_err(|e| e.to_string())?;
    std::fs::write(&path, serialized).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_app_config(app_handle: AppHandle) -> Result<(), String> {
    let path = resolve_app_config_path(&app_handle)?;
    if !path.exists() {
        return Ok(());
    }
    std::fs::remove_file(&path).map_err(|e| e.to_string())
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

fn resolve_app_config_path(app_handle: &AppHandle) -> Result<PathBuf, String> {
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

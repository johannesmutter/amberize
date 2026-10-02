//! Backend bootstrap runs even when no webview exists.
use crate::{app_commands, app_state::AppState, background_sync, configuration};
use email_archiver_storage::Storage;
use std::{fs::OpenOptions, path::Path};
use tauri::{AppHandle, Manager};

pub fn activate(app: &AppHandle, path: &str) -> Result<(), String> {
    activate_with(app, path, || Ok(()))
}
pub fn activate_with(
    app: &AppHandle,
    path: &str,
    commit: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let canonical=Path::new(path).canonicalize().map_err(|e|format!("The saved archive is unavailable at {path}: {e}. Reconnect its disk or choose the existing archive."))?;
    let state = app.state::<AppState>();
    let mut active = state.active_db_path.lock().map_err(|e| e.to_string())?;
    if active.as_deref() == canonical.to_str() {
        return commit();
    }
    let storage = Storage::open_existing(&canonical).map_err(|e| e.to_string())?;
    let restored = AppState::default();
    restore_last_sync(&restored, &storage)?;
    let next_status = restored
        .last_sync
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    let lock_path = canonical.with_file_name(format!(
        "{}.amberize-lock",
        canonical.file_name().unwrap().to_string_lossy()
    ));
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
        .map_err(|e| e.to_string())?;
    file.try_lock()
        .map_err(|e| format!("This archive is already open in another Amberize process: {e}"))?;
    commit()?;
    *state.archive_lock.lock().map_err(|e| e.to_string())? = Some(file);
    *active = Some(canonical.to_string_lossy().into());
    *state.integrity_status.lock().map_err(|e| e.to_string())? = None;
    *state.last_sync.lock().map_err(|e| e.to_string())? = next_status;
    Ok(())
}

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn_blocking(move || {
        let bootstrap_state = app.state::<AppState>();
        let _sync_guard = bootstrap_state.sync_lock.blocking_lock();
        let result = (|| -> Result<(), String> {
            let (config, warning) =
                configuration::load(&app_commands::resolve_app_config_path(&app)?)?;
            let state = app.state::<AppState>();
            if let Some(warning) = warning {
                *state.startup_warning.lock().map_err(|e| e.to_string())? = Some(warning);
            }
            if let Some(config) = config {
                state.set_sync_interval_secs(config.sync_interval_secs);
                activate(&app, &config.db_path)?;
                let storage = Storage::open_existing(&config.db_path).map_err(|e| e.to_string())?;
                storage
                    .set_sync_interval_secs(config.sync_interval_secs)
                    .map_err(|e| e.to_string())?;
                background_sync::record_startup_and_detect_gaps(&app);
                background_sync::verify_integrity_at_startup(&app);
                restore_last_sync(&state, &storage)?;
                state.sync_wakeup.notify_one();
            }
            Ok(())
        })();
        if let Err(error) = result {
            let state = app.state::<AppState>();
            state.set_archive_recovery_warning(error);
            state.set_tray_status_text("Archive unavailable — open Amberize to recover");
        }
    });
}

pub fn verify_async(app: AppHandle) {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _guard = state.sync_lock.blocking_lock();
        background_sync::verify_integrity_at_startup(&app)
    });
}

fn restore_last_sync(state: &AppState, storage: &Storage) -> Result<(), String> {
    if let Some(event) = storage
        .list_recent_events(Some("ui_sync_finished"), 1, 0)
        .map_err(|e| e.to_string())?
        .first()
    {
        let mut restored: crate::app_state::UiSyncStatus =
            serde_json::from_str(event.detail.as_deref().unwrap_or("{}"))
                .map_err(|e| e.to_string())?;
        restored.sync_in_progress = false;
        *state.last_sync.lock().map_err(|e| e.to_string())? = restored;
        return Ok(());
    }
    let events = storage
        .list_recent_events(Some("sync_finished"), 1, 0)
        .map_err(|e| e.to_string())?;
    if let Some(event) = events.first() {
        let detail: serde_json::Value =
            serde_json::from_str(event.detail.as_deref().unwrap_or("{}"))
                .map_err(|e| e.to_string())?;
        let mut outcome = detail
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        if storage
            .list_accounts()
            .map_err(|e| e.to_string())?
            .iter()
            .filter(|a| !a.disabled)
            .count()
            > 1
        {
            outcome = "unknown";
        }
        if let Ok(mut status) = state.last_sync.lock() {
            status.last_sync_at = Some(event.occurred_at.clone());
            status.last_sync_status =
                crate::sync_status_text::format_last_sync_status_text(outcome, &event.occurred_at);
            if outcome == "ok" {
                status.last_success_at = Some(event.occurred_at.clone());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restoring_an_aggregate_failure_does_not_use_a_later_individual_success() {
        let storage = Storage::open_in_memory_for_tests().unwrap();
        let state = AppState::default();
        *state.last_sync.lock().unwrap() = crate::app_state::UiSyncStatus {
            last_success_at: Some("previous success".into()),
            last_sync_at: Some("failed attempt".into()),
            last_sync_status: "Partial — failed attempt".into(),
            error: Some("one account failed".into()),
            ..Default::default()
        };
        background_sync::persist_sync_status(storage.db_path().to_str().unwrap(), &state);
        storage
            .append_event(&email_archiver_storage::InsertEventInput {
                occurred_at: "later".into(),
                kind: "sync_finished".into(),
                account_id: None,
                mailbox_id: None,
                message_blob_id: None,
                detail: r#"{"status":"ok"}"#.into(),
            })
            .unwrap();
        let restored = AppState::default();
        restore_last_sync(&restored, &storage).unwrap();
        let status = restored.last_sync.lock().unwrap();
        assert!(status.last_sync_status.starts_with("Partial"));
        assert_eq!(status.error.as_deref(), Some("one account failed"));
        assert_eq!(status.last_success_at.as_deref(), Some("previous success"));
    }
}

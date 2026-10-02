use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use email_archiver_storage::IntegrityStatus;
use serde::{Deserialize, Serialize};
use tauri::{menu::MenuItem, Wry};
use tokio::sync::Mutex as TokioMutex;

/// Default background sync interval: five minutes.
const DEFAULT_SYNC_INTERVAL_SECS: u64 = 5 * 60;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UiSyncStatus {
    pub last_success_at: Option<String>,
    pub error: Option<String>,
    pub sync_in_progress: bool,
    pub last_sync_at: Option<String>,
    pub last_sync_status: String,
}

pub struct AppState {
    pub config_lock: Mutex<()>,
    pub archive_lock: Mutex<Option<std::fs::File>>,
    pub startup_warning: Mutex<Option<String>>,
    archive_recovery_warning: Mutex<Option<String>>,
    pub sync_wakeup: tokio::sync::Notify,
    pub window_visible: AtomicBool,
    pub frontend_ready: AtomicBool,
    pub pending_actions: Mutex<Vec<String>>,
    pub oauth_operations: Mutex<std::collections::HashMap<String, OAuthOperation>>,
    pub export_operations:
        Mutex<std::collections::HashMap<String, crate::auditor_export::ExportOperation>>,
    pub active_db_path: Mutex<Option<String>>,
    pub sync_lock: TokioMutex<()>,
    pub sync_in_progress: AtomicBool,
    pub last_sync: Mutex<UiSyncStatus>,
    pub tray_status_item: Mutex<Option<MenuItem<Wry>>>,
    pub export_eml_item: Mutex<Option<MenuItem<Wry>>>,
    /// Background sync interval in seconds. Updated from the UI.
    pub sync_interval_secs: AtomicU64,
    /// Result of the most recent integrity verification (set on startup).
    pub integrity_status: Mutex<Option<IntegrityStatus>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            config_lock: Mutex::new(()),
            archive_lock: Mutex::new(None),
            startup_warning: Mutex::new(None),
            archive_recovery_warning: Mutex::new(None),
            sync_wakeup: tokio::sync::Notify::new(),
            window_visible: AtomicBool::new(false),
            frontend_ready: AtomicBool::new(false),
            pending_actions: Mutex::new(vec![]),
            oauth_operations: Mutex::new(std::collections::HashMap::new()),
            export_operations: Mutex::new(std::collections::HashMap::new()),
            active_db_path: Mutex::new(None),
            sync_lock: TokioMutex::new(()),
            sync_in_progress: AtomicBool::new(false),
            last_sync: Mutex::new(UiSyncStatus {
                last_success_at: None,
                error: None,
                sync_in_progress: false,
                last_sync_at: None,
                last_sync_status: "never".to_string(),
            }),
            tray_status_item: Mutex::new(None),
            export_eml_item: Mutex::new(None),
            sync_interval_secs: AtomicU64::new(DEFAULT_SYNC_INTERVAL_SECS),
            integrity_status: Mutex::new(None),
        }
    }
}

impl AppState {
    pub fn set_archive_recovery_warning(&self, message: String) {
        if let (Ok(mut warning), Ok(mut recovery)) = (
            self.startup_warning.lock(),
            self.archive_recovery_warning.lock(),
        ) {
            *recovery = Some(message.clone());
            *warning = Some(message);
        }
    }

    pub fn clear_archive_recovery_warning(&self) -> Result<(), String> {
        let mut warning = self.startup_warning.lock().map_err(|e| e.to_string())?;
        let mut recovery = self
            .archive_recovery_warning
            .lock()
            .map_err(|e| e.to_string())?;
        if recovery.is_some() && *warning == *recovery {
            *warning = None;
        }
        *recovery = None;
        Ok(())
    }

    pub fn set_sync_in_progress(&self, in_progress: bool) {
        self.sync_in_progress.store(in_progress, Ordering::SeqCst);
    }

    pub fn sync_in_progress(&self) -> bool {
        self.sync_in_progress.load(Ordering::SeqCst)
    }

    pub fn sync_interval_secs(&self) -> u64 {
        self.sync_interval_secs.load(Ordering::SeqCst)
    }

    pub fn set_sync_interval_secs(&self, secs: u64) {
        self.sync_interval_secs.store(secs, Ordering::SeqCst);
    }

    pub fn set_tray_status_item(&self, item: MenuItem<Wry>) {
        if let Ok(mut guard) = self.tray_status_item.lock() {
            *guard = Some(item);
        }
    }

    pub fn set_export_eml_item(&self, item: MenuItem<Wry>) {
        if let Ok(mut guard) = self.export_eml_item.lock() {
            *guard = Some(item);
        }
    }

    pub fn set_export_eml_enabled(&self, enabled: bool) {
        let item = {
            let Ok(guard) = self.export_eml_item.lock() else {
                return;
            };
            guard.clone()
        };
        let Some(item) = item else {
            return;
        };
        let _ = item.set_enabled(enabled);
    }

    pub fn set_tray_status_text(&self, text: &str) {
        let item = {
            let Ok(guard) = self.tray_status_item.lock() else {
                return;
            };
            guard.clone()
        };
        let Some(item) = item else {
            return;
        };
        let _ = item.set_text(text);
    }
}

pub enum OAuthOperation {
    Running(tokio::sync::oneshot::Sender<()>),
    Cancelled,
    Committed(i64),
}

impl UiSyncStatus {
    pub fn preserve_success(&mut self, previous: &Self) {
        self.last_success_at = if self.last_sync_status.starts_with("OK") {
            self.last_sync_at.clone()
        } else {
            previous.last_success_at.clone()
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_recovery_clears_only_the_resolved_warning() {
        let state = AppState::default();
        state.set_archive_recovery_warning("Archive unavailable".into());
        state.clear_archive_recovery_warning().unwrap();
        assert!(state.startup_warning.lock().unwrap().is_none());
        state.set_archive_recovery_warning("Archive unavailable".into());
        *state.startup_warning.lock().unwrap() = Some("Verification failed".into());
        state.clear_archive_recovery_warning().unwrap();
        assert_eq!(
            state.startup_warning.lock().unwrap().as_deref(),
            Some("Verification failed")
        );
        assert!(state.integrity_status.lock().unwrap().is_none());
    }
    #[test]
    fn failure_preserves_last_success_instead_of_reporting_attempt_as_success() {
        let previous = UiSyncStatus {
            last_success_at: Some("previous success".into()),
            ..Default::default()
        };
        let mut failed = UiSyncStatus {
            last_sync_at: Some("failed attempt".into()),
            last_sync_status: "Error — today".into(),
            ..Default::default()
        };
        failed.preserve_success(&previous);
        assert_eq!(failed.last_success_at, previous.last_success_at);
    }
}

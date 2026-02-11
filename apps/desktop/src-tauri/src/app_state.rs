use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use email_archiver_storage::IntegrityStatus;
use serde::Serialize;
use tauri::{menu::MenuItem, Wry};
use tokio::sync::Mutex as TokioMutex;

/// Default background sync interval: 15 minutes.
const DEFAULT_SYNC_INTERVAL_SECS: u64 = 5 * 60;

#[derive(Debug, Clone, Default, Serialize)]
pub struct UiSyncStatus {
    pub sync_in_progress: bool,
    pub last_sync_at: Option<String>,
    pub last_sync_status: String,
}

pub struct AppState {
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
            active_db_path: Mutex::new(None),
            sync_lock: TokioMutex::new(()),
            sync_in_progress: AtomicBool::new(false),
            last_sync: Mutex::new(UiSyncStatus {
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

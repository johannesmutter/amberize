//! Opt-in debug-only native lifecycle probe. Requires an isolated test config directory.
use crate::app_state::AppState;
use std::time::Duration;
use tauri::{AppHandle, Manager};

pub fn start(app: AppHandle) {
    let Some(directory) = std::env::var_os("AMBERIZE_TEST_CONFIG_DIR") else {
        return;
    };
    if std::env::var_os("AMBERIZE_TEST_LIFECYCLE").is_none() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let mut stages = vec![];
        let result = async {
            for _ in 0..100 {
                if app
                    .state::<AppState>()
                    .integrity_status
                    .lock()
                    .ok()
                    .is_some_and(|s| s.is_some())
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            let state = app.state::<AppState>();
            if state
                .active_db_path
                .lock()
                .map_err(|e| e.to_string())?
                .is_none()
            {
                return Err("Backend did not restore the test archive".into());
            }
            if app.get_webview_window("main").is_some() {
                return Err("Background startup created a webview".into());
            }
            stages.push("backend restored archive without a webview");
            let sync_guard = if std::env::var_os("AMBERIZE_TEST_RESTORE_DURING_SYNC").is_some() {
                Some(state.sync_lock.lock().await)
            } else {
                None
            };
            let handle = app.clone();
            app.run_on_main_thread(move || crate::menubar::show_main_window(&handle))
                .map_err(|e| e.to_string())?;
            for _ in 0..100 {
                if state
                    .frontend_ready
                    .load(std::sync::atomic::Ordering::SeqCst)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            if !state
                .frontend_ready
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                return Err("First webview did not register its listeners".into());
            }
            stages.push("first webview ready");
            app.get_webview_window("main")
                .ok_or("Main window missing")?
                .close()
                .map_err(|e| e.to_string())?;
            tokio::time::sleep(Duration::from_secs(33)).await;
            if app.get_webview_window("main").is_some() {
                return Err("Closed webview was not released".into());
            }
            stages.push("closed webview released; backend still running");
            let handle = app.clone();
            app.run_on_main_thread(move || crate::menubar::show_main_window(&handle))
                .map_err(|e| e.to_string())?;
            for _ in 0..100 {
                if state
                    .frontend_ready
                    .load(std::sync::atomic::Ordering::SeqCst)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            if !state
                .frontend_ready
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                return Err("Recreated webview did not register its listeners".into());
            }
            stages.push("recreated webview ready");
            if sync_guard.is_some() {
                if state.sync_lock.try_lock().is_ok() {
                    return Err("Sync lock was not held across window restoration".into());
                }
                stages.push(
                    "both webviews restored the active archive while sync lock remained held",
                );
            }
            drop(sync_guard);
            Ok::<_, String>(())
        }
        .await;
        let output = std::path::PathBuf::from(directory).join("native-smoke.json");
        let report = serde_json::json!({"stages":stages,"error":result.err(),"interval":app.state::<AppState>().sync_interval_secs()});
        let _ = std::fs::write(output, serde_json::to_vec_pretty(&report).unwrap());
        app.exit(if report["error"].is_null() { 0 } else { 1 });
    });
}

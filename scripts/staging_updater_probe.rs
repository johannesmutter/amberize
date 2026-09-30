//! Injected only into an isolated v0.2.3 test build; never part of a distributable.
use std::{path::PathBuf, sync::Mutex, time::Duration};
use tauri::{AppHandle, Manager};

static REPORT_LOCK: Mutex<()> = Mutex::new(());

fn report_directory() -> Result<PathBuf, String> {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true") {
        return Err("Updater automation requires a fresh hosted CI profile".into());
    }
    let root = PathBuf::from(std::env::var_os("RUNNER_TEMP").ok_or("RUNNER_TEMP missing")?);
    let directory = PathBuf::from(
        std::env::var_os("AMBERIZE_UPDATER_QA_DIR").ok_or("QA report directory missing")?,
    );
    if !directory.starts_with(root) {
        return Err("QA reports must stay inside RUNNER_TEMP".into());
    }
    Ok(directory)
}

#[tauri::command]
pub fn qa_updater_observation(stage: String) -> Result<(), String> {
    if ![
        "ready",
        "update_available",
        "install_clicked",
        "installed",
        "restart_clicked",
        "install_error",
        "automation_error",
    ]
    .contains(&stage.as_str())
    {
        return Err("Unknown observation".into());
    }
    let _guard = REPORT_LOCK.lock().map_err(|_| "Report lock unavailable")?;
    let file = report_directory()?.join("observations.json");
    let mut stages: Vec<String> = if file.exists() {
        serde_json::from_slice(&std::fs::read(&file).map_err(|_| "Cannot read observations")?)
            .map_err(|_| "Invalid observations")?
    } else {
        vec![]
    };
    if !stages.contains(&stage) {
        stages.push(stage);
        std::fs::write(file, serde_json::to_vec(&stages).unwrap())
            .map_err(|_| "Cannot write observations")?;
    }
    Ok(())
}

pub fn start(app: AppHandle) {
    report_directory().expect("Unsafe updater QA environment");
    // Use the unmodified older Svelte UI and its plugin check/download/install calls.
    // The injected observer only clicks existing buttons and reports fixed stage names.
    const SCRIPT: &str = r#"
    (() => {
      if (!window.__TAURI_INTERNALS__ || window.__AMBERIZE_UPDATER_QA) return;
      window.__AMBERIZE_UPDATER_QA = true;
      const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
      const report = stage => invoke('qa_updater_observation', {stage});
      let started = false, installed = false, busy = false;
      const timer = setInterval(async () => {
        if (busy) return;
        busy = true;
        try {
          const config = await invoke('get_app_config');
          if (!config?.db_path) return;
          if (!started) {
            const button = [...document.querySelectorAll('.update-banner button')]
              .find(button => button.textContent.trim() === 'Install & Restart');
            if (!button || button.disabled) return;
            if (!document.querySelector('.update-banner')?.textContent.includes('v0.2.4-6'))
              throw new Error('Unexpected candidate version');
            await invoke('autostart_set_enabled', {enabled:true});
            if (!await invoke('autostart_is_enabled')) throw new Error('Autostart unavailable');
            await report('ready');
            await report('update_available');
            started = true;
            await report('install_clicked');
            button.click();
            return;
          }
          const restart = [...document.querySelectorAll('.update-banner button')]
            .find(button => button.textContent.trim() === 'Restart now');
          if (restart && !installed) {
            installed = true;
            await report('installed');
            await report('restart_clicked');
            clearInterval(timer);
            restart.click();
            return;
          }
          const retry = [...document.querySelectorAll('.update-banner button')]
            .find(button => button.textContent.trim() === 'Install & Restart');
          if (retry && !retry.disabled && started) {
            await report('install_error');
            clearInterval(timer);
          }
        } catch {
          await report('automation_error');
          clearInterval(timer);
        } finally { busy = false; }
      }, 250);
    })();
    "#;
    tauri::async_runtime::spawn(async move {
        for _ in 0..120 {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.eval(SCRIPT);
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    });
}

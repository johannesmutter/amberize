#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_commands;
mod app_state;
mod auditor_export;
mod background_sync;
mod bootstrap;
mod configuration;
mod documentation;
mod menubar;
#[cfg(debug_assertions)]
mod native_smoke;
mod output;
mod sync_status_text;

use tauri::Manager;

fn main() {
    #[allow(unused_mut)]
    let mut context = tauri::generate_context!();
    #[cfg(debug_assertions)]
    if std::env::var_os("AMBERIZE_TEST_CONFIG_DIR").is_some() {
        context.config_mut().identifier = "com.amberize.isolated-test".into();
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            menubar::show_main_window(app)
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .on_window_event(menubar::on_main_window_close)
        .setup(|app| {
            app.manage(app_state::AppState::default());
            menubar::setup_menubar(app)?;
            bootstrap::start(app.handle().clone());
            background_sync::start_background_sync(app.handle().clone());
            if !std::env::args().any(|a| a == "--background") {
                menubar::show_main_window(app.handle());
            }

            #[cfg(debug_assertions)]
            native_smoke::start(app.handle().clone());

            #[cfg(debug_assertions)]
            if let Some(window) = app.get_webview_window("main") {
                window.open_devtools();
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_commands::select_archive,
            app_commands::frontend_ready,
            app_commands::get_startup_warning,
            app_commands::autostart_is_enabled,
            app_commands::autostart_set_enabled,
            app_commands::restart_app,
            app_commands::imap_discover_mailboxes,
            app_commands::create_account_and_discover_mailboxes,
            app_commands::list_accounts,
            app_commands::list_mailboxes,
            app_commands::set_mailbox_sync_enabled,
            app_commands::set_account_password,
            app_commands::set_active_db_path,
            app_commands::clear_active_db_path,
            app_commands::get_sync_status,
            app_commands::get_sync_interval,
            app_commands::set_sync_interval,
            app_commands::sync_account_once_command,
            app_commands::sync_all_accounts_command,
            app_commands::search_messages,
            app_commands::list_messages,
            app_commands::get_message_blob_raw_mime,
            app_commands::get_message_detail,
            app_commands::export_message_blob_eml,
            app_commands::remove_account,
            app_commands::diagnose_database,
            app_commands::reset_mailbox_cursors,
            app_commands::list_events,
            app_commands::export_events_csv,
            app_commands::set_export_eml_menu_enabled,
            app_commands::get_google_oauth_configured,
            app_commands::get_google_oauth_has_embedded,
            app_commands::set_google_oauth_client,
            app_commands::add_google_oauth_account,
            app_commands::cancel_google_oauth,
            documentation::generate_documentation,
            documentation::open_documentation,
            auditor_export::export_auditor_package,
            auditor_export::cancel_auditor_export,
            app_commands::get_archive_stats,
            app_commands::get_archive_date_range,
            app_commands::get_integrity_status,
            app_commands::is_sync_folder_path,
            app_commands::get_app_config,
            app_commands::save_app_config,
            app_commands::clear_app_config
        ])
        .build(context)
        .expect("error while building tauri application")
        .run(|app, event| match event {
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen {
                has_visible_windows: false,
                ..
            } => menubar::show_main_window(app),
            tauri::RunEvent::ExitRequested {
                api, code: None, ..
            } => api.prevent_exit(),
            _ => {
                let _ = app;
            }
        });
}

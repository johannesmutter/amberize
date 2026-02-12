use tauri::{
    menu::{CheckMenuItemBuilder, MenuBuilder, MenuItem, PredefinedMenuItem, Submenu},
    tray::TrayIconBuilder,
    App, AppHandle, Emitter, Manager, WindowEvent, Wry,
};

#[cfg(target_os = "macos")]
use tauri::ActivationPolicy;

use crate::app_state::AppState;

use tauri_plugin_autostart::{MacosLauncher, ManagerExt as AutostartManagerExt};

type TrayMenu = tauri::menu::Menu<Wry>;
type TrayCheckMenuItem = tauri::menu::CheckMenuItem<Wry>;

const TRAY_MENU_ID_OPEN: &str = "open";
const TRAY_MENU_ID_STATUS: &str = "status";
const TRAY_MENU_ID_SYNC_NOW: &str = "sync_now";
const TRAY_MENU_ID_EXPORT_AUDITOR: &str = "export_auditor";
const TRAY_MENU_ID_DOCUMENTATION: &str = "documentation";
const TRAY_MENU_ID_LAUNCH_AT_LOGIN: &str = "launch_at_login";
const TRAY_MENU_ID_QUIT: &str = "quit";

const APP_MENU_ID_SETTINGS: &str = "settings";
const APP_MENU_ID_CHECK_UPDATES: &str = "check_updates";
const APP_MENU_ID_EXPORT_EML: &str = "export_eml";

const EVENT_TRAY_SYNC_NOW: &str = "tray_sync_now";
const EVENT_TRAY_EXPORT_AUDITOR: &str = "tray_export_auditor";
const EVENT_TRAY_DOCUMENTATION: &str = "tray_documentation";
const EVENT_MENU_OPEN_SETTINGS: &str = "menu_open_settings";
const EVENT_MENU_CHECK_UPDATES: &str = "menu_check_updates";
const EVENT_MENU_EXPORT_EML: &str = "menu_export_eml";

pub fn setup_menubar(app: &mut App) -> tauri::Result<()> {
    setup_activation_policy(app);
    setup_autostart_plugin(app);
    setup_native_menu(app)?;

    let launch_at_login_item = build_launch_at_login_item(app)?;
    let (tray_menu, status_item) = build_tray_menu(app, &launch_at_login_item)?;

    let mut tray_builder = TrayIconBuilder::new()
        .menu(&tray_menu)
        .show_menu_on_left_click(true)
        .on_menu_event(move |app_handle, event| {
            handle_tray_menu_event(app_handle, event.id.as_ref(), &launch_at_login_item);
        });

    // Use an explicit, small PNG for crisp rendering in the macOS menu bar.
    tray_builder = tray_builder.icon(tauri::include_image!("icons/tray-icon.png"));

    tray_builder.build(app)?;

    app.state::<AppState>().set_tray_status_item(status_item);
    app.state::<AppState>()
        .set_tray_status_text("Last sync: never");

    Ok(())
}

#[cfg(target_os = "macos")]
fn setup_activation_policy(app: &mut App) {
    app.set_activation_policy(ActivationPolicy::Regular);
}

#[cfg(not(target_os = "macos"))]
fn setup_activation_policy(_app: &mut App) {}

/// Initialise the autostart plugin. On macOS this uses LaunchAgent;
/// on Windows it writes to the Run registry key; on Linux it creates
/// an XDG autostart desktop entry. The `MacosLauncher` parameter is
/// ignored on non-macOS platforms.
fn setup_autostart_plugin(app: &mut App) {
    let _ = app.handle().plugin(tauri_plugin_autostart::init(
        MacosLauncher::LaunchAgent,
        None,
    ));
}

/// Build the native application menu.
/// On macOS this becomes the global menu bar; on Windows and Linux it
/// renders as an in-window menu bar at the top of the main window.
fn setup_native_menu(app: &mut App) -> tauri::Result<()> {
    {
        // Build the app submenu (Amberize menu)
        let settings_item = MenuItem::with_id(
            app,
            APP_MENU_ID_SETTINGS,
            "Settings...",
            true,
            Some("CmdOrCtrl+,"),
        )?;
        let check_updates_item = MenuItem::with_id(
            app,
            APP_MENU_ID_CHECK_UPDATES,
            "Check for Updates...",
            true,
            None::<&str>,
        )?;
        let separator = PredefinedMenuItem::separator(app)?;
        let quit_item = PredefinedMenuItem::quit(app, Some("Quit Amberize"))?;

        let app_submenu = Submenu::with_items(
            app,
            "Amberize",
            true,
            &[&settings_item, &check_updates_item, &separator, &quit_item],
        )?;

        // Build the File submenu (export starts disabled until an email is selected)
        let export_eml_item = MenuItem::with_id(
            app,
            APP_MENU_ID_EXPORT_EML,
            "Export Selected Email...",
            false,
            Some("CmdOrCtrl+E"),
        )?;

        app.state::<AppState>()
            .set_export_eml_item(export_eml_item.clone());

        let file_submenu = Submenu::with_items(app, "File", true, &[&export_eml_item])?;

        // Build the Edit submenu with standard clipboard and undo shortcuts
        let undo = PredefinedMenuItem::undo(app, Some("Undo"))?;
        let redo = PredefinedMenuItem::redo(app, Some("Redo"))?;
        let cut = PredefinedMenuItem::cut(app, Some("Cut"))?;
        let copy = PredefinedMenuItem::copy(app, Some("Copy"))?;
        let paste = PredefinedMenuItem::paste(app, Some("Paste"))?;
        let select_all = PredefinedMenuItem::select_all(app, Some("Select All"))?;

        let edit_submenu = Submenu::with_items(
            app,
            "Edit",
            true,
            &[
                &undo,
                &redo,
                &PredefinedMenuItem::separator(app)?,
                &cut,
                &copy,
                &paste,
                &PredefinedMenuItem::separator(app)?,
                &select_all,
            ],
        )?;

        // Build the Window submenu
        let minimize = PredefinedMenuItem::minimize(app, Some("Minimize"))?;
        let close = PredefinedMenuItem::close_window(app, Some("Close"))?;

        let window_submenu = Submenu::with_items(app, "Window", true, &[&minimize, &close])?;

        // Build the main menu
        let menu = MenuBuilder::new(app)
            .items(&[&app_submenu, &file_submenu, &edit_submenu, &window_submenu])
            .build()?;

        // Set as the app menu
        app.set_menu(menu)?;

        // Handle menu events
        app.on_menu_event(move |app_handle, event| match event.id.as_ref() {
            APP_MENU_ID_SETTINGS => {
                show_main_window(app_handle);
                let _ = app_handle.emit(EVENT_MENU_OPEN_SETTINGS, ());
            }
            APP_MENU_ID_CHECK_UPDATES => {
                show_main_window(app_handle);
                let _ = app_handle.emit(EVENT_MENU_CHECK_UPDATES, ());
            }
            APP_MENU_ID_EXPORT_EML => {
                let _ = app_handle.emit(EVENT_MENU_EXPORT_EML, ());
            }
            _ => {}
        });
    }

    Ok(())
}

fn build_launch_at_login_item(app: &App) -> tauri::Result<TrayCheckMenuItem> {
    let autostart_enabled = is_autostart_enabled(app.handle());

    CheckMenuItemBuilder::with_id(TRAY_MENU_ID_LAUNCH_AT_LOGIN, "Launch at login")
        .checked(autostart_enabled)
        .build(app)
}

fn build_tray_menu(
    app: &App,
    launch_at_login_item: &TrayCheckMenuItem,
) -> tauri::Result<(TrayMenu, MenuItem<Wry>)> {
    let status_item = MenuItem::with_id(
        app,
        TRAY_MENU_ID_STATUS,
        "Last sync: never",
        false,
        None::<&str>,
    )?;
    let open_item = MenuItem::with_id(app, TRAY_MENU_ID_OPEN, "Open", true, None::<&str>)?;
    let sync_now_item =
        MenuItem::with_id(app, TRAY_MENU_ID_SYNC_NOW, "Sync now", true, None::<&str>)?;
    let export_auditor_item = MenuItem::with_id(
        app,
        TRAY_MENU_ID_EXPORT_AUDITOR,
        "Export (auditor package)",
        true,
        None::<&str>,
    )?;
    let documentation_item = MenuItem::with_id(
        app,
        TRAY_MENU_ID_DOCUMENTATION,
        "Documentation",
        true,
        None::<&str>,
    )?;
    let quit_item = MenuItem::with_id(app, TRAY_MENU_ID_QUIT, "Quit", true, None::<&str>)?;
    let separator_status = PredefinedMenuItem::separator(app)?;
    let separator_1 = PredefinedMenuItem::separator(app)?;
    let separator_2 = PredefinedMenuItem::separator(app)?;
    let separator_3 = PredefinedMenuItem::separator(app)?;

    let menu = MenuBuilder::new(app)
        .items(&[
            &status_item,
            &separator_status,
            &open_item,
            &sync_now_item,
            &export_auditor_item,
            &separator_1,
            &documentation_item,
            &separator_2,
            launch_at_login_item,
            &separator_3,
            &quit_item,
        ])
        .build()?;

    Ok((menu, status_item))
}

fn handle_tray_menu_event(
    app_handle: &AppHandle,
    menu_id: &str,
    launch_at_login_item: &TrayCheckMenuItem,
) {
    match menu_id {
        TRAY_MENU_ID_OPEN => show_main_window(app_handle),
        TRAY_MENU_ID_SYNC_NOW => {
            let _ = app_handle.emit(EVENT_TRAY_SYNC_NOW, ());
            show_main_window(app_handle);
        }
        TRAY_MENU_ID_EXPORT_AUDITOR => {
            let _ = app_handle.emit(EVENT_TRAY_EXPORT_AUDITOR, ());
            show_main_window(app_handle);
        }
        TRAY_MENU_ID_DOCUMENTATION => {
            let _ = app_handle.emit(EVENT_TRAY_DOCUMENTATION, ());
            show_main_window(app_handle);
        }
        TRAY_MENU_ID_LAUNCH_AT_LOGIN => toggle_autostart(app_handle, launch_at_login_item),
        TRAY_MENU_ID_QUIT => app_handle.exit(0),
        _ => {}
    }
}

fn show_main_window(app_handle: &AppHandle) {
    let Some(window) = app_handle.get_webview_window("main") else {
        return;
    };

    #[cfg(target_os = "macos")]
    let _ = app_handle.set_activation_policy(ActivationPolicy::Regular);

    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

/// Handle main window close: hide instead of close, restore Accessory policy on macOS.
pub fn on_main_window_close(window: &tauri::Window<Wry>, event: &WindowEvent) {
    if window.label() != "main" {
        return;
    }

    if let WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();
        let _ = window.app_handle().emit("main_window_hidden", ());
        let _ = window.hide();

        #[cfg(target_os = "macos")]
        let _ = window
            .app_handle()
            .set_activation_policy(ActivationPolicy::Accessory);
    }
}

fn toggle_autostart(app_handle: &AppHandle, launch_at_login_item: &TrayCheckMenuItem) {
    let Some(new_enabled_state) = toggle_autostart_enabled(app_handle) else {
        return;
    };

    let _ = launch_at_login_item.set_checked(new_enabled_state);
}

fn is_autostart_enabled(app_handle: &AppHandle) -> bool {
    let autostart_manager = app_handle.autolaunch();
    autostart_manager.is_enabled().unwrap_or(false)
}

fn toggle_autostart_enabled(app_handle: &AppHandle) -> Option<bool> {
    let autostart_manager = app_handle.autolaunch();
    let currently_enabled = autostart_manager.is_enabled().ok()?;

    if currently_enabled {
        autostart_manager.disable().ok()?;
        return Some(false);
    }

    autostart_manager.enable().ok()?;
    Some(true)
}

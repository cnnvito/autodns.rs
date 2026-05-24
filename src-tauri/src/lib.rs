mod commands;
mod config;
mod desktop;
mod dns;
mod logging;
mod preferences;
mod service;
mod store;
mod system_dns;

use commands::*;
use service::DesktopService;
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{TrayIcon, TrayIconBuilder},
    Emitter, Manager, WindowEvent, Wry,
};

const TRAY_SHOW: &str = "show";
const TRAY_HIDE: &str = "hide";
const TRAY_TOGGLE_SERVICE: &str = "toggle-service";
const TRAY_RESTART_SERVICE: &str = "restart-service";
const TRAY_CLEAR_CACHE: &str = "clear-cache";
const TRAY_QUIT: &str = "quit";

struct TrayState {
    tray: TrayIcon,
    status: MenuItem<Wry>,
    listen: MenuItem<Wry>,
    toggle_service: MenuItem<Wry>,
    restart_service: MenuItem<Wry>,
    clear_cache: MenuItem<Wry>,
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(DesktopService::new())
        .setup(|app| {
            if let Err(err) = app.state::<DesktopService>().initialize() {
                app.state::<DesktopService>()
                    .record_error("initialize local database", &err.to_string());
            }
            if let Err(err) = setup_tray(app) {
                app.state::<DesktopService>()
                    .record_error("initialize tray icon", &err.to_string());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let service = window.state::<DesktopService>();
                if service.allow_quit() {
                    return;
                }

                match preferences::load_desktop_preferences() {
                    Ok(prefs) if prefs.close_behavior == preferences::CLOSE_BEHAVIOR_QUIT => {}
                    Ok(prefs) if prefs.close_behavior == preferences::CLOSE_BEHAVIOR_HIDE => {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                    _ => {
                        api.prevent_close();
                        let _ = window.emit("desktop:close-requested", ());
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            start_autodns,
            stop_autodns,
            status,
            recent_logs,
            clear_logs,
            clear_dns_cache,
            lookup_domain,
            export_logs,
            load_config,
            managed_config,
            validate_config,
            apply_config,
            load_preferences,
            save_preferences,
            system_dns_status,
            save_system_dns_settings,
            apply_system_dns,
            restore_system_dns,
            hide_window,
            show_window,
            quit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let status = MenuItem::with_id(app, "tray-status", "状态：已停止", false, None::<&str>)?;
    let listen = MenuItem::with_id(app, "tray-listen", "监听：-", false, None::<&str>)?;
    let show = MenuItem::with_id(app, TRAY_SHOW, "显示窗口", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, TRAY_HIDE, "隐藏窗口", true, None::<&str>)?;
    let toggle_service =
        MenuItem::with_id(app, TRAY_TOGGLE_SERVICE, "启动服务", true, None::<&str>)?;
    let restart_service =
        MenuItem::with_id(app, TRAY_RESTART_SERVICE, "重启服务", false, None::<&str>)?;
    let clear_cache =
        MenuItem::with_id(app, TRAY_CLEAR_CACHE, "清理 DNS 缓存", false, None::<&str>)?;
    let quit = MenuItem::with_id(app, TRAY_QUIT, "退出", true, None::<&str>)?;
    let separator_1 = PredefinedMenuItem::separator(app)?;
    let separator_2 = PredefinedMenuItem::separator(app)?;
    let separator_3 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &status,
            &listen,
            &separator_1,
            &show,
            &hide,
            &separator_2,
            &toggle_service,
            &restart_service,
            &clear_cache,
            &separator_3,
            &quit,
        ],
    )?;
    #[cfg(target_os = "windows")]
    let icon = Image::from_bytes(include_bytes!("../icons/tray-icon-windows.png"))?;
    #[cfg(not(target_os = "windows"))]
    let icon = Image::from_bytes(include_bytes!("../icons/tray-icon.png"))?;

    let tray = TrayIconBuilder::with_id("autodns")
        .icon(icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("已停止")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            TRAY_SHOW => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            TRAY_HIDE => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }
            TRAY_TOGGLE_SERVICE => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let service = app.state::<DesktopService>();
                    let result = if service.status().running {
                        service.stop().await
                    } else {
                        service.start(String::new()).await
                    };
                    if let Err(err) = result {
                        service.record_error("tray service toggle", &err.to_string());
                    }
                    refresh_tray_state(&app);
                });
            }
            TRAY_RESTART_SERVICE => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let service = app.state::<DesktopService>();
                    if service.status().running {
                        if let Err(err) = service.stop().await {
                            service.record_error("tray service restart", &err.to_string());
                        } else if let Err(err) = service.start(String::new()).await {
                            service.record_error("tray service restart", &err.to_string());
                        }
                    }
                    refresh_tray_state(&app);
                });
            }
            TRAY_CLEAR_CACHE => {
                app.state::<DesktopService>().clear_dns_cache();
            }
            TRAY_QUIT => {
                let service = app.state::<DesktopService>();
                service.set_allow_quit();
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    app.manage(TrayState {
        tray,
        status,
        listen,
        toggle_service,
        restart_service,
        clear_cache,
    });
    refresh_tray_state(app.handle());
    Ok(())
}

pub(crate) fn refresh_tray_state(app: &tauri::AppHandle) {
    let service_status = app.state::<DesktopService>().status();
    let Some(tray_state) = app.try_state::<TrayState>() else {
        return;
    };
    let listen = if service_status.listen.is_empty() {
        "-"
    } else {
        service_status.listen.as_str()
    };

    if service_status.running {
        let _ = tray_state.status.set_text("状态：运行中");
        let _ = tray_state.listen.set_text(format!("监听：{listen}"));
        let _ = tray_state.toggle_service.set_text("停止服务");
        let _ = tray_state.restart_service.set_enabled(true);
        let _ = tray_state.clear_cache.set_enabled(true);
        let _ = tray_state
            .tray
            .set_tooltip(Some(format!("运行中 · {listen}")));
    } else {
        let _ = tray_state.status.set_text("状态：已停止");
        let _ = tray_state.listen.set_text("监听：-");
        let _ = tray_state.toggle_service.set_text("启动服务");
        let _ = tray_state.restart_service.set_enabled(false);
        let _ = tray_state.clear_cache.set_enabled(false);
        let _ = tray_state.tray.set_tooltip(Some("已停止"));
    }
}

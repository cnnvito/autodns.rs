mod certificates;
mod commands;
mod config;
mod desktop;
mod dns;
mod environment;
mod history;
mod logging;
mod preferences;
mod service;
mod store;
mod system_dns;

use commands::*;
use desktop::DesktopWindowState;
use service::DesktopService;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, PhysicalPosition, PhysicalSize, WindowEvent, Wry,
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

#[derive(Default)]
pub(crate) struct WindowReadyState {
    ready: AtomicBool,
}

#[derive(Default)]
struct WindowStateSaveThrottle {
    last_saved_at: Mutex<Option<Instant>>,
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let _ = reveal_main_window(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(DesktopService::new())
        .manage(WindowReadyState::default())
        .manage(WindowStateSaveThrottle::default())
        .setup(|app| {
            let app_handle = app.handle().clone();
            let pending_status_emit = Arc::new(AtomicBool::new(false));
            app.state::<DesktopService>().set_status_listener({
                let pending_status_emit = pending_status_emit.clone();
                move || schedule_desktop_status(app_handle.clone(), pending_status_emit.clone())
            });
            schedule_startup_window_fallback(app.handle().clone());
            let initialized = match app.state::<DesktopService>().initialize() {
                Ok(()) => true,
                Err(err) => {
                    app.state::<DesktopService>()
                        .record_error("initialize local database", &err.to_string());
                    false
                }
            };
            if let Err(err) = setup_tray(app) {
                app.state::<DesktopService>()
                    .record_error("initialize tray icon", &err.to_string());
            }
            if initialized && app.state::<DesktopService>().service_enabled() {
                schedule_service_autostart(app.handle().clone());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            match event {
                WindowEvent::Resized(_) | WindowEvent::Moved(_) => {
                    schedule_window_state_save(window);
                }
                _ => {}
            }

            if let WindowEvent::CloseRequested { api, .. } = event {
                save_window_state(window);
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
            clear_dns_cache,
            lookup_domain,
            check_upstream_health,
            list_dns_history,
            dns_history_top_domains,
            dns_history_upstream_names,
            dns_history_overview,
            clear_dns_history,
            managed_config,
            validate_config,
            validate_server_certificate,
            apply_config,
            load_preferences,
            save_preferences,
            certificate_defaults,
            generate_server_certificate,
            system_dns_status,
            save_system_dns_settings,
            apply_system_dns,
            restore_system_dns,
            show_main_window,
            hide_window,
            quit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let text = tray_text();
    let status = MenuItem::with_id(
        app,
        "tray-status",
        format!("{}{}", text.status_prefix, text.stopped),
        false,
        None::<&str>,
    )?;
    let listen = MenuItem::with_id(
        app,
        "tray-listen",
        format!("{}-", text.listen_prefix),
        false,
        None::<&str>,
    )?;
    let show = MenuItem::with_id(app, TRAY_SHOW, text.show, true, None::<&str>)?;
    let hide = MenuItem::with_id(app, TRAY_HIDE, text.hide, true, None::<&str>)?;
    let toggle_service = MenuItem::with_id(
        app,
        TRAY_TOGGLE_SERVICE,
        text.start_service,
        true,
        None::<&str>,
    )?;
    let restart_service = MenuItem::with_id(
        app,
        TRAY_RESTART_SERVICE,
        text.restart_service,
        false,
        None::<&str>,
    )?;
    let clear_cache =
        MenuItem::with_id(app, TRAY_CLEAR_CACHE, text.clear_cache, false, None::<&str>)?;
    let quit = MenuItem::with_id(app, TRAY_QUIT, text.quit, true, None::<&str>)?;
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
    let tray_app = app.handle().clone();

    let tray = TrayIconBuilder::with_id(environment::tray_id())
        .icon(icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip(text.stopped)
        .menu(&menu)
        .show_menu_on_left_click(!cfg!(target_os = "windows"))
        .on_tray_icon_event(move |_tray, event| {
            if cfg!(target_os = "windows")
                && matches!(
                    event,
                    TrayIconEvent::DoubleClick {
                        button: MouseButton::Left,
                        ..
                    }
                )
            {
                let _ = reveal_main_window(&tray_app);
            }
        })
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
                    emit_desktop_status(&app);
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
                    emit_desktop_status(&app);
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
    let text = tray_text();

    if service_status.running {
        let _ = tray_state
            .status
            .set_text(format!("{}{}", text.status_prefix, text.running));
        let _ = tray_state
            .listen
            .set_text(format!("{}{listen}", text.listen_prefix));
        let _ = tray_state.toggle_service.set_text(text.stop_service);
        let _ = tray_state.restart_service.set_enabled(true);
        let _ = tray_state.clear_cache.set_enabled(true);
        let _ = tray_state
            .tray
            .set_tooltip(Some(format!("{} · {listen}", text.running)));
    } else {
        let _ = tray_state
            .status
            .set_text(format!("{}{}", text.status_prefix, text.stopped));
        let _ = tray_state
            .listen
            .set_text(format!("{}-", text.listen_prefix));
        let _ = tray_state.toggle_service.set_text(text.start_service);
        let _ = tray_state.restart_service.set_enabled(false);
        let _ = tray_state.clear_cache.set_enabled(false);
        let _ = tray_state.tray.set_tooltip(Some(text.stopped.to_string()));
    }
}

#[derive(Clone, Copy)]
struct TrayText {
    status_prefix: &'static str,
    listen_prefix: &'static str,
    running: &'static str,
    stopped: &'static str,
    show: &'static str,
    hide: &'static str,
    start_service: &'static str,
    stop_service: &'static str,
    restart_service: &'static str,
    clear_cache: &'static str,
    quit: &'static str,
}

fn tray_text() -> TrayText {
    TrayText {
        status_prefix: "Status: ",
        listen_prefix: "Listen: ",
        running: "Running",
        stopped: "Stopped",
        show: "Show window",
        hide: "Hide window",
        start_service: "Start service",
        stop_service: "Stop service",
        restart_service: "Restart service",
        clear_cache: "Clear DNS cache",
        quit: "Quit",
    }
}

pub(crate) fn emit_desktop_status(app: &tauri::AppHandle) {
    let status = app.state::<DesktopService>().status();
    let _ = app.emit("desktop:status", status);
}

pub(crate) fn reveal_main_window(app: &tauri::AppHandle) -> tauri::Result<()> {
    app.state::<WindowReadyState>()
        .ready
        .store(true, Ordering::SeqCst);
    if let Some(window) = app.get_webview_window("main") {
        restore_window_state(&window);
        window.show()?;
        window.set_focus()?;
    }
    Ok(())
}

fn schedule_window_state_save(window: &tauri::Window) {
    let throttle = window.state::<WindowStateSaveThrottle>();
    let now = Instant::now();
    let mut last_saved_at = match throttle.last_saved_at.lock() {
        Ok(guard) => guard,
        Err(_) => return,
    };
    if last_saved_at
        .map(|last| now.duration_since(last) < Duration::from_millis(500))
        .unwrap_or(false)
    {
        return;
    }
    *last_saved_at = Some(now);
    drop(last_saved_at);
    save_window_state(window);
}

fn save_window_state(window: &tauri::Window) {
    let size = match window.inner_size() {
        Ok(size) => size,
        Err(_) => return,
    };
    let position = window.outer_position().ok();
    let maximized = window.is_maximized().unwrap_or(false);
    let state = DesktopWindowState {
        width: size.width,
        height: size.height,
        x: position.map(|position| position.x),
        y: position.map(|position| position.y),
        maximized,
    };
    let _ = preferences::save_window_state(state);
}

fn restore_window_state(window: &tauri::WebviewWindow) {
    let Some(state) = preferences::saved_window_state() else {
        return;
    };
    if window.is_maximized().unwrap_or(false) {
        let _ = window.unmaximize();
    }
    if let (Some(x), Some(y)) = (state.x, state.y) {
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }
    let _ = window.set_size(PhysicalSize::new(state.width, state.height));
    if state.maximized {
        let _ = window.maximize();
    }
}

fn schedule_desktop_status(app: tauri::AppHandle, pending: Arc<AtomicBool>) {
    if pending.swap(true, Ordering::AcqRel) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        pending.store(false, Ordering::Release);
        emit_desktop_status(&app);
    });
}

fn schedule_startup_window_fallback(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(8)).await;
        if app.state::<WindowReadyState>().ready.load(Ordering::SeqCst) {
            return;
        }
        let _ = reveal_main_window(&app);
    });
}

fn schedule_service_autostart(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let service = app.state::<DesktopService>();
        if let Err(err) = service.start(String::new()).await {
            service.record_error("auto start DNS service", &err.to_string());
        }
        refresh_tray_state(&app);
        emit_desktop_status(&app);
    });
}

use crate::desktop::{
    ApplyConfigResult, ConfigDocument, DesktopConfig, DesktopPreferences, DesktopStatus,
    DnsLookupResult, SystemDnsSettings, SystemDnsStatus,
};
use crate::logging::LogEntry;
use crate::preferences;
use crate::service::DesktopService;
use tauri::{AppHandle, Manager, State};

fn to_command_error(err: anyhow::Error) -> String {
    err.to_string()
}

#[tauri::command]
pub async fn start_autodns(
    app: AppHandle,
    service: State<'_, DesktopService>,
    config_path: String,
) -> Result<(), String> {
    let result = service.start(config_path).await;
    crate::refresh_tray_state(&app);
    result.map_err(to_command_error)
}

#[tauri::command]
pub async fn stop_autodns(
    app: AppHandle,
    service: State<'_, DesktopService>,
) -> Result<(), String> {
    let result = service.stop().await;
    crate::refresh_tray_state(&app);
    result.map_err(to_command_error)
}

#[tauri::command]
pub fn status(service: State<'_, DesktopService>) -> DesktopStatus {
    service.status()
}

#[tauri::command]
pub fn recent_logs(service: State<'_, DesktopService>) -> Vec<LogEntry> {
    service.recent_logs()
}

#[tauri::command]
pub fn clear_dns_cache(service: State<'_, DesktopService>) -> usize {
    service.clear_dns_cache()
}

#[tauri::command]
pub async fn lookup_domain(
    service: State<'_, DesktopService>,
    domain: String,
    record_type: String,
) -> Result<DnsLookupResult, String> {
    service
        .lookup_domain(domain, record_type)
        .await
        .map_err(to_command_error)
}

#[tauri::command]
pub fn managed_config(service: State<'_, DesktopService>) -> Result<ConfigDocument, String> {
    service.managed_config().map_err(to_command_error)
}

#[tauri::command]
pub fn validate_config(
    service: State<'_, DesktopService>,
    config: DesktopConfig,
) -> Result<(), String> {
    service.validate_config(config).map_err(to_command_error)
}

#[tauri::command]
pub async fn apply_config(
    app: AppHandle,
    service: State<'_, DesktopService>,
    doc: ConfigDocument,
) -> Result<ApplyConfigResult, String> {
    let result = service.apply_config(doc).await;
    crate::refresh_tray_state(&app);
    result.map_err(to_command_error)
}

#[tauri::command]
pub fn load_preferences() -> Result<DesktopPreferences, String> {
    preferences::load_desktop_preferences().map_err(to_command_error)
}

#[tauri::command]
pub fn save_preferences(prefs: DesktopPreferences) -> Result<DesktopPreferences, String> {
    preferences::save_desktop_preferences(prefs).map_err(to_command_error)
}

#[tauri::command]
pub async fn system_dns_status(
    app: AppHandle,
    force: Option<bool>,
) -> Result<SystemDnsStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let service = app.state::<DesktopService>();
        service.system_dns_status(force.unwrap_or(false))
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(to_command_error)
}

#[tauri::command]
pub async fn save_system_dns_settings(
    app: AppHandle,
    settings: SystemDnsSettings,
) -> Result<SystemDnsStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let service = app.state::<DesktopService>();
        service.save_system_dns_settings(settings)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(to_command_error)
}

#[tauri::command]
pub async fn apply_system_dns(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let service = app.state::<DesktopService>();
        service.apply_system_dns()
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(to_command_error)
}

#[tauri::command]
pub async fn restore_system_dns(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let service = app.state::<DesktopService>();
        service.restore_system_dns()
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(to_command_error)
}

#[tauri::command]
pub fn hide_window(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.hide().map_err(|err| err.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn quit_app(app: AppHandle, service: State<'_, DesktopService>) {
    service.set_allow_quit();
    app.exit(0);
}

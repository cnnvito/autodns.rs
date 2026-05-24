use crate::desktop::{
    ApplyConfigResult, ConfigDocument, DesktopConfig, DesktopPreferences, DesktopStatus,
    DnsLookupResult, SystemDnsSettings, SystemDnsStatus,
};
use crate::logging::LogEntry;
use crate::preferences;
use crate::service::DesktopService;
use chrono::Utc;
use std::fs;
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
pub fn clear_logs(service: State<'_, DesktopService>) {
    service.clear_logs();
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
pub fn export_logs(service: State<'_, DesktopService>) -> Result<String, String> {
    let dir = crate::config::desktop_config_dir()
        .map_err(to_command_error)?
        .join("exports");
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let filename = format!("autodns-logs-{}.json", Utc::now().format("%Y%m%d-%H%M%S"));
    let path = dir.join(filename);
    let data = serde_json::to_vec_pretty(&service.recent_logs()).map_err(|err| err.to_string())?;
    fs::write(&path, data).map_err(|err| err.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn load_config(
    service: State<'_, DesktopService>,
    path: String,
) -> Result<ConfigDocument, String> {
    service.load_config(path).map_err(to_command_error)
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
pub fn system_dns_status(service: State<'_, DesktopService>) -> Result<SystemDnsStatus, String> {
    service.system_dns_status().map_err(to_command_error)
}

#[tauri::command]
pub fn save_system_dns_settings(
    service: State<'_, DesktopService>,
    settings: SystemDnsSettings,
) -> Result<SystemDnsStatus, String> {
    service
        .save_system_dns_settings(settings)
        .map_err(to_command_error)
}

#[tauri::command]
pub fn apply_system_dns(service: State<'_, DesktopService>) -> Result<(), String> {
    service.apply_system_dns().map_err(to_command_error)
}

#[tauri::command]
pub fn restore_system_dns(service: State<'_, DesktopService>) -> Result<(), String> {
    service.restore_system_dns().map_err(to_command_error)
}

#[tauri::command]
pub fn hide_window(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.hide().map_err(|err| err.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn show_window(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(|err| err.to_string())?;
        window.set_focus().map_err(|err| err.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn quit_app(app: AppHandle, service: State<'_, DesktopService>) {
    service.set_allow_quit();
    app.exit(0);
}

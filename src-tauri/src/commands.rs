use crate::desktop::{
    ApplyConfigResult, ConfigDocument, DesktopConfig, DesktopPreferences, DesktopStatus,
    DnsHistoryList, DnsHistoryOverview, DnsHistoryTopDomain, DnsLookupResult, SystemDnsSettings,
    SystemDnsStatus,
};
use crate::preferences;
use crate::service::DesktopService;
use serde::Serialize;
use std::collections::BTreeMap;
use tauri::{AppHandle, Manager, State};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    code: String,
    message: String,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    values: BTreeMap<String, String>,
}

fn to_command_error(err: anyhow::Error) -> CommandError {
    command_error_from_message(&err.to_string())
}

fn command_error_from_message(message: &str) -> CommandError {
    let mut values = BTreeMap::new();
    let code = match message {
        "server.listen is required" => "config.serverListenRequired",
        "system DNS takeover is disabled" => "systemDns.takeoverDisabled",
        "no network adapter is selected" => "systemDns.noAdapterSelected",
        "target DNS server is empty" => "systemDns.emptyTargetServer",
        _ => {
            if let Some((protocol, listen)) =
                parse_listener_error(message, " listen address ", " is already in use")
            {
                values.insert("protocol".to_string(), protocol);
                values.insert("listen".to_string(), listen);
                "dns.listenerAddressInUse"
            } else if let Some((protocol, listen)) =
                parse_listener_error(message, " listen address ", " permission denied")
            {
                values.insert("protocol".to_string(), protocol);
                values.insert("listen".to_string(), listen);
                "dns.listenerPermissionDenied"
            } else if let Some((protocol, rest)) = message.split_once(" listen address ") {
                if let Some((listen, reason)) = rest.split_once(" bind failed: ") {
                    values.insert("protocol".to_string(), protocol.to_string());
                    values.insert("listen".to_string(), listen.to_string());
                    values.insert("reason".to_string(), reason.to_string());
                    "dns.listenerBindFailed"
                } else {
                    "command.unknown"
                }
            } else {
                "command.unknown"
            }
        }
    };

    CommandError {
        code: code.to_string(),
        message: message.to_string(),
        values,
    }
}

fn parse_listener_error(message: &str, middle: &str, suffix: &str) -> Option<(String, String)> {
    let (protocol_part, rest) = message.split_once(middle)?;
    let protocol = protocol_part
        .split(|ch: char| ch == ':' || ch.is_whitespace())
        .next_back()?;
    let listen = rest.strip_suffix(suffix)?;
    Some((protocol.to_string(), listen.to_string()))
}

#[tauri::command]
pub async fn start_autodns(
    app: AppHandle,
    service: State<'_, DesktopService>,
    config_path: String,
) -> Result<DesktopStatus, CommandError> {
    let result = service.start(config_path).await;
    crate::refresh_tray_state(&app);
    crate::emit_desktop_status(&app);
    result.map(|()| service.status()).map_err(to_command_error)
}

#[tauri::command]
pub async fn stop_autodns(
    app: AppHandle,
    service: State<'_, DesktopService>,
) -> Result<DesktopStatus, CommandError> {
    let result = service.stop().await;
    crate::refresh_tray_state(&app);
    crate::emit_desktop_status(&app);
    result.map(|()| service.status()).map_err(to_command_error)
}

#[tauri::command]
pub fn status(service: State<'_, DesktopService>) -> DesktopStatus {
    service.status()
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
) -> Result<DnsLookupResult, CommandError> {
    service
        .lookup_domain(domain, record_type)
        .await
        .map_err(to_command_error)
}

#[tauri::command]
pub async fn list_dns_history(
    app: AppHandle,
    domain: Option<String>,
    status_filter: Option<String>,
    window: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<DnsHistoryList, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let service = app.state::<DesktopService>();
        service.list_dns_history(
            domain.unwrap_or_default(),
            status_filter.unwrap_or_else(|| "all".to_string()),
            window.unwrap_or_else(|| "all".to_string()),
            limit.unwrap_or(100),
            offset.unwrap_or(0),
        )
    })
    .await
    .map_err(|err| command_error_from_message(&err.to_string()))?
    .map_err(to_command_error)
}

#[tauri::command]
pub async fn dns_history_top_domains(
    app: AppHandle,
    limit: Option<usize>,
    domain: Option<String>,
    status_filter: Option<String>,
    window: Option<String>,
) -> Result<Vec<DnsHistoryTopDomain>, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let service = app.state::<DesktopService>();
        service.dns_history_top_domains(
            limit.unwrap_or(20),
            domain.unwrap_or_default(),
            status_filter.unwrap_or_else(|| "all".to_string()),
            window.unwrap_or_else(|| "all".to_string()),
        )
    })
    .await
    .map_err(|err| command_error_from_message(&err.to_string()))?
    .map_err(to_command_error)
}

#[tauri::command]
pub async fn dns_history_overview(app: AppHandle) -> Result<DnsHistoryOverview, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let service = app.state::<DesktopService>();
        service.dns_history_overview()
    })
    .await
    .map_err(|err| command_error_from_message(&err.to_string()))?
    .map_err(to_command_error)
}

#[tauri::command]
pub async fn clear_dns_history(app: AppHandle) -> Result<usize, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let service = app.state::<DesktopService>();
        service.clear_dns_history()
    })
    .await
    .map_err(|err| command_error_from_message(&err.to_string()))?
    .map_err(to_command_error)
}

#[tauri::command]
pub fn managed_config(service: State<'_, DesktopService>) -> Result<ConfigDocument, CommandError> {
    service.managed_config().map_err(to_command_error)
}

#[tauri::command]
pub fn validate_config(
    service: State<'_, DesktopService>,
    config: DesktopConfig,
) -> Result<(), CommandError> {
    service.validate_config(config).map_err(to_command_error)
}

#[tauri::command]
pub async fn apply_config(
    app: AppHandle,
    service: State<'_, DesktopService>,
    doc: ConfigDocument,
) -> Result<ApplyConfigResult, CommandError> {
    let result = service.apply_config(doc).await;
    crate::refresh_tray_state(&app);
    crate::emit_desktop_status(&app);
    result.map_err(to_command_error)
}

#[tauri::command]
pub fn load_preferences() -> Result<DesktopPreferences, CommandError> {
    preferences::load_desktop_preferences().map_err(to_command_error)
}

#[tauri::command]
pub fn save_preferences(
    app: AppHandle,
    prefs: DesktopPreferences,
) -> Result<DesktopPreferences, CommandError> {
    let saved = preferences::save_desktop_preferences(prefs).map_err(to_command_error)?;
    crate::refresh_tray_state(&app);
    Ok(saved)
}

#[tauri::command]
pub async fn system_dns_status(
    app: AppHandle,
    force: Option<bool>,
) -> Result<SystemDnsStatus, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let service = app.state::<DesktopService>();
        service.system_dns_status(force.unwrap_or(false))
    })
    .await
    .map_err(|err| command_error_from_message(&err.to_string()))?
    .map_err(to_command_error)
}

#[tauri::command]
pub async fn save_system_dns_settings(
    app: AppHandle,
    settings: SystemDnsSettings,
) -> Result<SystemDnsStatus, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let service = app.state::<DesktopService>();
        service.save_system_dns_settings(settings)
    })
    .await
    .map_err(|err| command_error_from_message(&err.to_string()))?
    .map_err(to_command_error)
}

#[tauri::command]
pub async fn apply_system_dns(app: AppHandle) -> Result<SystemDnsStatus, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let service = app.state::<DesktopService>();
        service.apply_system_dns()
    })
    .await
    .map_err(|err| command_error_from_message(&err.to_string()))?
    .map_err(to_command_error)
}

#[tauri::command]
pub async fn restore_system_dns(app: AppHandle) -> Result<SystemDnsStatus, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let service = app.state::<DesktopService>();
        service.restore_system_dns()
    })
    .await
    .map_err(|err| command_error_from_message(&err.to_string()))?
    .map_err(to_command_error)
}

#[tauri::command]
pub fn hide_window(app: AppHandle) -> Result<(), CommandError> {
    if let Some(window) = app.get_webview_window("main") {
        window
            .hide()
            .map_err(|err| command_error_from_message(&err.to_string()))?;
    }
    Ok(())
}

#[tauri::command]
pub fn show_main_window(app: AppHandle) -> Result<(), CommandError> {
    crate::reveal_main_window(&app).map_err(|err| command_error_from_message(&err.to_string()))
}

#[tauri::command]
pub fn quit_app(app: AppHandle, service: State<'_, DesktopService>) {
    service.set_allow_quit();
    app.exit(0);
}

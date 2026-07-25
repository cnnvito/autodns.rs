use crate::certificates;
use crate::config::core_config_from_desktop;
use crate::desktop::{
    ApplyConfigResult, CertificateDefaults, ConfigDocument, DesktopConfig, DesktopPreferences,
    DesktopStatus, DnsHistoryList, DnsHistoryOverview, DnsHistoryTopDomain, DnsLookupResult,
    GenerateCertificateRequest, GeneratedCertificate, SystemDnsSettings, SystemDnsStatus,
    UpstreamHealthCheckResult,
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
        "DNS service is not running" => "dns.serviceNotRunning",
        "system DNS takeover is disabled" => "systemDns.takeoverDisabled",
        "no network adapter is selected" => "systemDns.noAdapterSelected",
        "target DNS server is empty" => "systemDns.emptyTargetServer",
        _ => {
            if let Some(name) = message.strip_prefix("upstream not found: ") {
                values.insert("name".to_string(), name.to_string());
                "dns.upstreamNotFound"
            } else if let Some((protocol, listen)) =
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
pub async fn check_upstream_health(
    app: AppHandle,
    service: State<'_, DesktopService>,
    upstream_name: String,
) -> Result<UpstreamHealthCheckResult, CommandError> {
    let result = service
        .check_upstream_health(upstream_name)
        .await
        .map_err(to_command_error)?;
    crate::emit_desktop_status(&app);
    Ok(result)
}

#[tauri::command]
pub async fn list_dns_history(
    app: AppHandle,
    domain: Option<String>,
    status_filter: Option<String>,
    window: Option<String>,
    upstream_name: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<DnsHistoryList, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let service = app.state::<DesktopService>();
        service.list_dns_history(
            domain.unwrap_or_default(),
            status_filter.unwrap_or_else(|| "all".to_string()),
            window.unwrap_or_else(|| "all".to_string()),
            upstream_name.unwrap_or_default(),
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
    upstream_name: Option<String>,
) -> Result<Vec<DnsHistoryTopDomain>, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let service = app.state::<DesktopService>();
        service.dns_history_top_domains(
            limit.unwrap_or(20),
            domain.unwrap_or_default(),
            status_filter.unwrap_or_else(|| "all".to_string()),
            window.unwrap_or_else(|| "all".to_string()),
            upstream_name.unwrap_or_default(),
        )
    })
    .await
    .map_err(|err| command_error_from_message(&err.to_string()))?
    .map_err(to_command_error)
}

#[tauri::command]
pub async fn dns_history_upstream_names(
    app: AppHandle,
    limit: Option<usize>,
) -> Result<Vec<String>, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let service = app.state::<DesktopService>();
        service.dns_history_upstream_names(limit.unwrap_or(200))
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
pub fn validate_server_certificate(config: DesktopConfig) -> Result<(), CommandError> {
    let mut core = core_config_from_desktop(config);
    core.apply_defaults();
    crate::dns::validate_server_tls_config(&core.server).map_err(to_command_error)
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
    app.state::<DesktopService>()
        .set_dns_history_enabled(saved.history_enabled);
    crate::refresh_tray_state(&app);
    Ok(saved)
}

#[tauri::command]
pub fn certificate_defaults() -> Result<CertificateDefaults, CommandError> {
    certificates::certificate_defaults().map_err(to_command_error)
}

#[tauri::command]
pub async fn generate_server_certificate(
    app: AppHandle,
    request: GenerateCertificateRequest,
) -> Result<GeneratedCertificate, CommandError> {
    let generated =
        tauri::async_runtime::spawn_blocking(move || certificates::generate_certificate(request))
            .await
            .map_err(|err| command_error_from_message(&err.to_string()))?
            .map_err(to_command_error)?;
    crate::refresh_tray_state(&app);
    Ok(generated)
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

// The localization codes sent to the frontend are recovered from error message
// text, so any wording change in the error sources silently downgrades the
// message to `command.unknown`. These tests pin the mapping and the wording of
// the messages it depends on.
#[cfg(test)]
mod tests {
    use super::command_error_from_message;

    fn code_of(message: &str) -> String {
        command_error_from_message(message).code
    }

    #[test]
    fn maps_known_messages_to_codes() {
        assert_eq!(
            code_of("server.listen is required"),
            "config.serverListenRequired"
        );
        assert_eq!(
            code_of("DNS service is not running"),
            "dns.serviceNotRunning"
        );
        assert_eq!(
            code_of("system DNS takeover is disabled"),
            "systemDns.takeoverDisabled"
        );
        assert_eq!(
            code_of("no network adapter is selected"),
            "systemDns.noAdapterSelected"
        );
        assert_eq!(
            code_of("target DNS server is empty"),
            "systemDns.emptyTargetServer"
        );
        assert_eq!(code_of("anything else"), "command.unknown");
    }

    #[test]
    fn maps_upstream_not_found_with_name() {
        let error = command_error_from_message("upstream not found: cloudflare");
        assert_eq!(error.code, "dns.upstreamNotFound");
        assert_eq!(
            error.values.get("name").map(String::as_str),
            Some("cloudflare")
        );
    }

    #[test]
    fn maps_listener_errors_with_protocol_and_listen() {
        // Wording must match the errors built in dns.rs listener binding.
        let in_use =
            command_error_from_message("udp listen address 127.0.0.1:53 is already in use");
        assert_eq!(in_use.code, "dns.listenerAddressInUse");
        assert_eq!(
            in_use.values.get("protocol").map(String::as_str),
            Some("udp")
        );
        assert_eq!(
            in_use.values.get("listen").map(String::as_str),
            Some("127.0.0.1:53")
        );

        let denied = command_error_from_message("tcp listen address 0.0.0.0:53 permission denied");
        assert_eq!(denied.code, "dns.listenerPermissionDenied");
        assert_eq!(
            denied.values.get("protocol").map(String::as_str),
            Some("tcp")
        );

        let bind_failed =
            command_error_from_message("udp listen address 10.0.0.1:53 bind failed: no route");
        assert_eq!(bind_failed.code, "dns.listenerBindFailed");
        assert_eq!(
            bind_failed.values.get("reason").map(String::as_str),
            Some("no route")
        );
    }

    #[test]
    fn maps_validate_error_from_its_source() {
        // End-to-end: the message produced by CoreConfig::validate must keep
        // mapping to its localization code.
        let mut core = crate::config::default_local_config();
        core.server.listen = String::new();
        let err = core.validate().expect_err("empty listen must be rejected");
        assert_eq!(code_of(&err.to_string()), "config.serverListenRequired");
    }
}

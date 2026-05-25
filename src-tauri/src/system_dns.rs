use crate::desktop::{SystemDnsAdapter, SystemDnsSettings, SystemDnsStatus};
use crate::store::ConfigStore;
use anyhow::{anyhow, Context, Result};
#[cfg(any(test, target_os = "windows"))]
use serde::Deserialize;
use std::collections::HashSet;
use std::net::SocketAddr;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use std::process::Command;
#[cfg(target_os = "windows")]
use std::process::Output;

pub fn status_from_adapters(
    store: &ConfigStore,
    listen: &str,
    mut adapters: Vec<SystemDnsAdapter>,
    last_error: Option<String>,
) -> Result<SystemDnsStatus> {
    let mut settings = store.load_system_dns_settings()?;
    let local_servers = local_dns_servers(listen);
    if settings.target_servers.is_empty() {
        settings.target_servers = local_servers.clone();
    }

    let selected: HashSet<String> = settings.selected_adapter_ids.iter().cloned().collect();
    let mut warnings = Vec::new();

    for adapter in &mut adapters {
        adapter.selected = selected.contains(&adapter.id);
        let saved = store.load_system_dns_adapter_state(&adapter.id)?;
        adapter.managed = saved.managed;
        adapter.original_dns = (!saved.original_dns.is_empty()).then_some(saved.original_dns);
        adapter.last_applied_at = saved.last_applied_at;
        adapter.last_restored_at = saved.last_restored_at;
        adapter.last_error = saved.last_error;
    }

    let active_count = adapters
        .iter()
        .filter(|adapter| adapter.status == "up")
        .count();
    if active_count > 1 {
        warnings.push(
            "检测到多个在线网络接口，建议只选择当前出口网络，避免影响 VPN 或虚拟网卡。".to_string(),
        );
    }
    if adapters.iter().any(|adapter| adapter.virtual_adapter) {
        warnings.push("列表包含虚拟或隧道接口，默认不会自动接管，请手动选择。".to_string());
    }
    if settings.enabled && settings.selected_adapter_ids.is_empty() {
        warnings.push("系统 DNS 接管已开启，但还没有选择任何网络接口。".to_string());
    }

    Ok(SystemDnsStatus {
        platform: std::env::consts::OS.to_string(),
        supported: platform_supported(),
        can_apply: platform_supported(),
        settings,
        local_servers,
        adapters,
        warnings,
        last_error,
    })
}

pub fn read_adapters() -> Result<Vec<SystemDnsAdapter>> {
    list_adapters()
}

pub fn save_settings(store: &ConfigStore, mut settings: SystemDnsSettings) -> Result<()> {
    settings.selected_adapter_ids.sort();
    settings.selected_adapter_ids.dedup();
    settings.target_servers = settings
        .target_servers
        .into_iter()
        .map(|server| server.trim().to_string())
        .filter(|server| !server.is_empty())
        .collect();
    store.save_system_dns_settings(settings)
}

pub fn apply(store: &ConfigStore, adapters: &mut [SystemDnsAdapter]) -> Result<()> {
    let settings = store.load_system_dns_settings()?;
    if !settings.enabled {
        return Err(anyhow!("system DNS takeover is disabled"));
    }
    if settings.selected_adapter_ids.is_empty() {
        return Err(anyhow!("no network adapter is selected"));
    }
    if settings.target_servers.is_empty() {
        return Err(anyhow!("target DNS server is empty"));
    }

    for adapter_id in settings.selected_adapter_ids {
        let Some(adapter) = adapters.iter_mut().find(|item| item.id == adapter_id) else {
            store.mark_system_dns_error(&adapter_id, "selected adapter no longer exists")?;
            continue;
        };
        if let Err(err) = apply_adapter(adapter, &settings.target_servers) {
            store.mark_system_dns_error(&adapter_id, &err.to_string())?;
            return Err(err).with_context(|| format!("apply system DNS: {}", adapter.name));
        }
        store.mark_system_dns_applied(&adapter_id, &adapter.dns_servers)?;
        adapter
            .original_dns
            .get_or_insert_with(|| adapter.dns_servers.clone());
        adapter.dns_servers = settings.target_servers.clone();
        adapter.managed = true;
        adapter.last_error = None;
    }
    Ok(())
}

pub fn restore(store: &ConfigStore, adapters: &mut [SystemDnsAdapter]) -> Result<()> {
    let settings = store.load_system_dns_settings()?;
    let mut adapter_ids = settings.selected_adapter_ids;
    adapter_ids.extend(
        adapters
            .iter()
            .filter_map(|adapter| {
                let state = store.load_system_dns_adapter_state(&adapter.id).ok()?;
                state.managed.then_some(adapter.id.clone())
            })
            .collect::<Vec<_>>(),
    );
    adapter_ids.sort();
    adapter_ids.dedup();

    for adapter_id in adapter_ids {
        let state = store.load_system_dns_adapter_state(&adapter_id)?;
        if !state.managed && state.original_dns.is_empty() {
            continue;
        }
        let Some(adapter) = adapters.iter_mut().find(|item| item.id == adapter_id) else {
            store.mark_system_dns_error(&adapter_id, "managed adapter no longer exists")?;
            continue;
        };
        if let Err(err) = restore_adapter(adapter, &state.original_dns) {
            store.mark_system_dns_error(&adapter_id, &err.to_string())?;
            return Err(err).with_context(|| format!("restore system DNS: {}", adapter.name));
        }
        store.mark_system_dns_restored(&adapter_id)?;
        adapter.dns_servers = state.original_dns;
        adapter.original_dns = None;
        adapter.managed = false;
        adapter.last_error = None;
    }
    Ok(())
}

fn local_dns_servers(listen: &str) -> Vec<String> {
    if let Ok(addr) = listen.parse::<SocketAddr>() {
        return vec![addr.ip().to_string()];
    }
    listen
        .split_once(':')
        .map(|(host, _)| host.trim().to_string())
        .filter(|host| !host.is_empty())
        .map(|host| vec![host])
        .unwrap_or_default()
}

fn platform_supported() -> bool {
    cfg!(target_os = "windows") || cfg!(target_os = "macos")
}

fn list_adapters() -> Result<Vec<SystemDnsAdapter>> {
    #[cfg(target_os = "windows")]
    {
        return windows::list_adapters();
    }

    #[cfg(target_os = "macos")]
    {
        return macos::list_adapters();
    }

    #[allow(unreachable_code)]
    Ok(Vec::new())
}

#[cfg_attr(
    not(any(target_os = "windows", target_os = "macos")),
    allow(unused_variables)
)]
fn apply_adapter(adapter: &SystemDnsAdapter, servers: &[String]) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        return windows::set_dns(adapter, servers);
    }

    #[cfg(target_os = "macos")]
    {
        return macos::set_dns(adapter, servers);
    }

    #[allow(unreachable_code)]
    Err(anyhow!(
        "system DNS takeover is not supported on this platform"
    ))
}

#[cfg_attr(
    not(any(target_os = "windows", target_os = "macos")),
    allow(unused_variables)
)]
fn restore_adapter(adapter: &SystemDnsAdapter, original_dns: &[String]) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        return windows::set_dns(adapter, original_dns);
    }

    #[cfg(target_os = "macos")]
    {
        return macos::set_dns(adapter, original_dns);
    }

    #[allow(unreachable_code)]
    Err(anyhow!(
        "system DNS restore is not supported on this platform"
    ))
}

#[cfg(any(test, target_os = "windows", target_os = "macos"))]
fn virtual_adapter(name: &str, description: &str) -> bool {
    let text = format!("{name} {description}").to_ascii_lowercase();
    [
        "vpn",
        "tun",
        "tap",
        "utun",
        "wireguard",
        "tailscale",
        "zerotier",
        "virtual",
        "vmware",
        "hyper-v",
        "loopback",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

#[cfg(any(test, target_os = "windows"))]
mod windows {
    use super::*;

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct WindowsDnsAdapter {
        interface_index: u32,
        interface_alias: String,
        interface_guid: Option<String>,
        address_family: u8,
        server_addresses: Option<Vec<String>>,
    }

    #[cfg(target_os = "windows")]
    pub fn list_adapters() -> Result<Vec<SystemDnsAdapter>> {
        let script = r#"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$OutputEncoding = [System.Text.UTF8Encoding]::new($false)
Get-DnsClientServerAddress |
  Where-Object { $_.AddressFamily -eq 2 } |
  Select-Object InterfaceIndex, InterfaceAlias, InterfaceGuid, AddressFamily, ServerAddresses |
  ConvertTo-Json -Compress
"#;
        let output = powershell_command()
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                script,
            ])
            .output()
            .context("run PowerShell Get-DnsClientServerAddress")?;
        parse_list_output(output)
    }

    #[cfg(target_os = "windows")]
    pub fn set_dns(adapter: &SystemDnsAdapter, servers: &[String]) -> Result<()> {
        let interface_index = adapter
            .interface_index
            .ok_or_else(|| anyhow!("Windows adapter is missing InterfaceIndex"))?;
        let script = set_dns_script(interface_index, servers)?;
        let output = powershell_command()
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &script,
            ])
            .output()
            .context("run PowerShell Set-DnsClientServerAddress")?;
        if !output.status.success() {
            return Err(anyhow!(
                "PowerShell Set-DnsClientServerAddress failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(())
    }

    #[cfg(target_os = "windows")]
    fn powershell_command() -> Command {
        use std::os::windows::process::CommandExt;

        const CREATE_NO_WINDOW: u32 = 0x08000000;

        let mut command = Command::new("powershell.exe");
        command.creation_flags(CREATE_NO_WINDOW);
        command
    }

    #[cfg(target_os = "windows")]
    fn parse_list_output(output: Output) -> Result<Vec<SystemDnsAdapter>> {
        if !output.status.success() {
            return Err(anyhow!(
                "PowerShell Get-DnsClientServerAddress failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        parse_adapter_json(
            &String::from_utf8(output.stdout).context("decode PowerShell JSON as UTF-8")?,
        )
    }

    fn parse_adapter_json(raw: &str) -> Result<Vec<SystemDnsAdapter>> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Ok(Vec::new());
        }
        let adapters: Vec<WindowsDnsAdapter> = if raw.starts_with('[') {
            serde_json::from_str(raw).context("parse Windows DNS adapter list")?
        } else {
            vec![serde_json::from_str(raw).context("parse Windows DNS adapter")?]
        };

        Ok(adapters
            .into_iter()
            .filter(|adapter| adapter.address_family == 2)
            .map(|adapter| {
                let id = adapter
                    .interface_guid
                    .clone()
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| adapter.interface_index.to_string());
                SystemDnsAdapter {
                    id,
                    name: adapter.interface_alias.clone(),
                    description: adapter.interface_alias.clone(),
                    status: "unknown".to_string(),
                    kind: "windows".to_string(),
                    dns_servers: adapter.server_addresses.unwrap_or_default(),
                    selected: false,
                    managed: false,
                    virtual_adapter: super::virtual_adapter(&adapter.interface_alias, ""),
                    interface_index: Some(adapter.interface_index),
                    original_dns: None,
                    last_applied_at: None,
                    last_restored_at: None,
                    last_error: None,
                }
            })
            .collect())
    }

    fn set_dns_script(interface_index: u32, servers: &[String]) -> Result<String> {
        if servers.is_empty() {
            return Ok(format!(
                r#"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$OutputEncoding = [System.Text.UTF8Encoding]::new($false)
Set-DnsClientServerAddress -InterfaceIndex {interface_index} -ResetServerAddresses
"#
            ));
        }

        let json = serde_json::to_string(servers).context("serialize Windows DNS servers")?;
        Ok(format!(
            r#"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$servers = ConvertFrom-Json -InputObject '{json}'
Set-DnsClientServerAddress -InterfaceIndex {interface_index} -ServerAddresses $servers
"#
        ))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn parses_single_adapter_with_chinese_alias() {
            let json = r#"{"InterfaceIndex":12,"InterfaceAlias":"以太网","InterfaceGuid":"{ABC}","AddressFamily":2,"ServerAddresses":["223.5.5.5","119.29.29.29"]}"#;

            let adapters = parse_adapter_json(json).expect("parse adapter");

            assert_eq!(adapters.len(), 1);
            assert_eq!(adapters[0].id, "{ABC}");
            assert_eq!(adapters[0].name, "以太网");
            assert_eq!(adapters[0].dns_servers, vec!["223.5.5.5", "119.29.29.29"]);
            assert_eq!(adapters[0].interface_index, Some(12));
        }

        #[test]
        fn parses_adapter_array_and_filters_ipv6_rows() {
            let json = r#"[
                {"InterfaceIndex":8,"InterfaceAlias":"WLAN","InterfaceGuid":"","AddressFamily":2,"ServerAddresses":null},
                {"InterfaceIndex":8,"InterfaceAlias":"WLAN","InterfaceGuid":"","AddressFamily":23,"ServerAddresses":["2400:3200::1"]}
            ]"#;

            let adapters = parse_adapter_json(json).expect("parse adapters");

            assert_eq!(adapters.len(), 1);
            assert_eq!(adapters[0].id, "8");
            assert!(adapters[0].dns_servers.is_empty());
        }

        #[test]
        fn builds_reset_script_without_server_addresses() {
            let script = set_dns_script(3, &[]).expect("build script");

            assert!(script.contains("-InterfaceIndex 3"));
            assert!(script.contains("-ResetServerAddresses"));
            assert!(!script.contains("-ServerAddresses"));
        }

        #[test]
        fn builds_utf8_json_server_script() {
            let servers = vec!["127.0.0.1".to_string(), "223.5.5.5".to_string()];
            let script = set_dns_script(5, &servers).expect("build script");

            assert!(script.contains("[System.Text.UTF8Encoding]"));
            assert!(script.contains("ConvertFrom-Json"));
            assert!(script.contains(r#"["127.0.0.1","223.5.5.5"]"#));
            assert!(script.contains("-InterfaceIndex 5"));
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;

    pub fn list_adapters() -> Result<Vec<SystemDnsAdapter>> {
        let services = network_services()?;
        let mut adapters = Vec::new();
        for service in services {
            let dns_servers = service_dns_servers(&service).unwrap_or_default();
            let status = service_status(&service).unwrap_or_else(|_| "unknown".to_string());
            adapters.push(SystemDnsAdapter {
                id: service.clone(),
                name: service.clone(),
                description: service.clone(),
                status,
                kind: "network-service".to_string(),
                dns_servers,
                selected: false,
                managed: false,
                virtual_adapter: super::virtual_adapter(&service, ""),
                interface_index: None,
                original_dns: None,
                last_applied_at: None,
                last_restored_at: None,
                last_error: None,
            });
        }
        Ok(adapters)
    }

    pub fn set_dns(adapter: &SystemDnsAdapter, servers: &[String]) -> Result<()> {
        let mut command = Command::new("networksetup");
        command.arg("-setdnsservers").arg(&adapter.id);
        if servers.is_empty() {
            command.arg("Empty");
        } else {
            command.args(servers);
        }
        let output = command
            .output()
            .with_context(|| format!("run networksetup -setdnsservers {}", adapter.id))?;
        if !output.status.success() {
            return Err(anyhow!(
                "networksetup -setdnsservers failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(())
    }

    fn network_services() -> Result<Vec<String>> {
        let output = Command::new("networksetup")
            .arg("-listallnetworkservices")
            .output()
            .context("run networksetup -listallnetworkservices")?;
        if !output.status.success() {
            return Err(anyhow!(
                "networksetup -listallnetworkservices failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(parse_network_services(&String::from_utf8_lossy(
            &output.stdout,
        )))
    }

    fn service_dns_servers(service: &str) -> Result<Vec<String>> {
        let output = Command::new("networksetup")
            .args(["-getdnsservers", service])
            .output()
            .with_context(|| format!("run networksetup -getdnsservers {service}"))?;
        if !output.status.success() {
            return Err(anyhow!(
                "networksetup -getdnsservers failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        Ok(parse_dns_servers(&text))
    }

    fn service_status(service: &str) -> Result<String> {
        let output = Command::new("networksetup")
            .args(["-getnetworkserviceenabled", service])
            .output()
            .with_context(|| format!("run networksetup -getnetworkserviceenabled {service}"))?;
        if !output.status.success() {
            return Err(anyhow!(
                "networksetup -getnetworkserviceenabled failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        Ok(if text.trim().eq_ignore_ascii_case("enabled") {
            "up".to_string()
        } else {
            "down".to_string()
        })
    }

    fn parse_network_services(raw: &str) -> Vec<String> {
        raw.lines()
            .skip_while(|line| line.starts_with("An asterisk"))
            .map(|line| line.trim_start_matches('*').trim().to_string())
            .filter(|line| !line.is_empty())
            .collect()
    }

    fn parse_dns_servers(raw: &str) -> Vec<String> {
        if raw.contains("There aren't any DNS Servers") {
            return Vec::new();
        }
        raw.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToString::to_string)
            .collect()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn parses_network_services_and_skips_header() {
            let raw = "An asterisk (*) denotes that a network service is disabled.\nWi-Fi\n*Thunderbolt Bridge\nUSB 10/100/1000 LAN\n";

            let services = parse_network_services(raw);

            assert_eq!(
                services,
                vec!["Wi-Fi", "Thunderbolt Bridge", "USB 10/100/1000 LAN"]
            );
        }

        #[test]
        fn parses_empty_dns_servers() {
            let servers = parse_dns_servers("There aren't any DNS Servers set on Wi-Fi.\n");

            assert!(servers.is_empty());
        }

        #[test]
        fn parses_dns_server_lines() {
            let servers = parse_dns_servers("223.5.5.5\n119.29.29.29\n\n");

            assert_eq!(servers, vec!["223.5.5.5", "119.29.29.29"]);
        }
    }
}

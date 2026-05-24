use crate::config::desktop_config_dir;
use crate::desktop::DesktopPreferences;
use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::PathBuf;

pub const CLOSE_BEHAVIOR_ASK: &str = "ask";
pub const CLOSE_BEHAVIOR_HIDE: &str = "hide";
pub const CLOSE_BEHAVIOR_QUIT: &str = "quit";

pub fn load_desktop_preferences() -> Result<DesktopPreferences> {
    let mut prefs = default_desktop_preferences();
    let path = desktop_preferences_path()?;
    let data = match fs::read(&path) {
        Ok(data) => data,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            prefs.start_at_login = start_at_login_enabled().unwrap_or(false);
            return Ok(prefs);
        }
        Err(err) => return Err(err).context("read desktop preferences"),
    };
    prefs = serde_json::from_slice(&data).context("parse desktop preferences")?;
    prefs = normalize_desktop_preferences(prefs);
    prefs.start_at_login = start_at_login_enabled().unwrap_or(prefs.start_at_login);
    Ok(prefs)
}

pub fn save_desktop_preferences(prefs: DesktopPreferences) -> Result<DesktopPreferences> {
    let current = load_desktop_preferences().unwrap_or_else(|_| default_desktop_preferences());
    let mut next = normalize_desktop_preferences(prefs);
    next.start_at_login_supported = current.start_at_login_supported;
    next.tray_supported = current.tray_supported;
    next.tray_message = current.tray_message;

    set_start_at_login(next.start_at_login)?;
    next.start_at_login = start_at_login_enabled().unwrap_or(next.start_at_login);

    let path = desktop_preferences_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("create preferences directory")?;
    }
    let data = serde_json::to_vec_pretty(&next).context("marshal desktop preferences")?;
    fs::write(&path, data).context("write desktop preferences")?;
    Ok(next)
}

fn default_desktop_preferences() -> DesktopPreferences {
    DesktopPreferences {
        close_behavior: CLOSE_BEHAVIOR_ASK.into(),
        start_at_login: false,
        start_at_login_supported: start_at_login_supported(),
        tray_supported: true,
        tray_message: "Tauri desktop keeps close-to-hide behavior in this port.".into(),
    }
}

fn normalize_desktop_preferences(mut prefs: DesktopPreferences) -> DesktopPreferences {
    let defaults = default_desktop_preferences();
    match prefs.close_behavior.as_str() {
        CLOSE_BEHAVIOR_ASK | CLOSE_BEHAVIOR_HIDE | CLOSE_BEHAVIOR_QUIT => {}
        _ => prefs.close_behavior = defaults.close_behavior,
    }
    prefs.start_at_login_supported = defaults.start_at_login_supported;
    prefs.tray_supported = defaults.tray_supported;
    prefs.tray_message = defaults.tray_message;
    prefs
}

fn desktop_preferences_path() -> Result<PathBuf> {
    Ok(desktop_config_dir()?.join("preferences.json"))
}

fn start_at_login_supported() -> bool {
    cfg!(any(
        target_os = "macos",
        target_os = "windows",
        target_os = "linux"
    ))
}

fn start_at_login_enabled() -> Result<bool> {
    platform_start_at_login_enabled()
}

fn set_start_at_login(enabled: bool) -> Result<()> {
    platform_set_start_at_login(enabled)
}

#[cfg(target_os = "macos")]
fn launch_agent_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("user home dir is not available"))?;
    Ok(home
        .join("Library")
        .join("LaunchAgents")
        .join("com.autodns.desktop.plist"))
}

#[cfg(target_os = "macos")]
fn platform_start_at_login_enabled() -> Result<bool> {
    Ok(launch_agent_path()?.exists())
}

#[cfg(target_os = "macos")]
fn platform_set_start_at_login(enabled: bool) -> Result<()> {
    let path = launch_agent_path()?;
    if !enabled {
        match fs::remove_file(path) {
            Ok(_) => return Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err).context("remove launch agent"),
        }
    }
    let exec_path = std::env::current_exe().context("resolve executable")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("create launch agents directory")?;
    }
    let data = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.autodns.desktop</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
</dict>
</plist>
"#,
        xml_escape(&exec_path.to_string_lossy())
    );
    fs::write(path, data).context("write launch agent")
}

#[cfg(target_os = "macos")]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(target_os = "linux")]
fn linux_autostart_path() -> Result<PathBuf> {
    let dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".config")))
        .ok_or_else(|| anyhow!("user config dir is not available"))?;
    Ok(dir.join("autostart").join("autodns.desktop"))
}

#[cfg(target_os = "linux")]
fn platform_start_at_login_enabled() -> Result<bool> {
    Ok(linux_autostart_path()?.exists())
}

#[cfg(target_os = "linux")]
fn platform_set_start_at_login(enabled: bool) -> Result<()> {
    let path = linux_autostart_path()?;
    if !enabled {
        match fs::remove_file(path) {
            Ok(_) => return Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err).context("remove autostart entry"),
        }
    }
    let exec_path = std::env::current_exe().context("resolve executable")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("create autostart directory")?;
    }
    let data = format!(
        "[Desktop Entry]\nType=Application\nName=autodns\nExec={}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
        desktop_escape(&exec_path.to_string_lossy())
    );
    fs::write(path, data).context("write autostart entry")
}

#[cfg(target_os = "linux")]
fn desktop_escape(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    if escaped.contains(char::is_whitespace) {
        format!("\"{escaped}\"")
    } else {
        escaped
    }
}

#[cfg(target_os = "windows")]
fn platform_start_at_login_enabled() -> Result<bool> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_QUERY_VALUE};
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu
        .open_subkey_with_flags(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Run",
            KEY_QUERY_VALUE,
        )
        .context("open startup registry key")?;
    let value: String = key.get_value("autodns").unwrap_or_default();
    Ok(!value.is_empty())
}

#[cfg(target_os = "windows")]
fn platform_set_start_at_login(enabled: bool) -> Result<()> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE};
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey_with_flags(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Run",
            KEY_SET_VALUE | KEY_QUERY_VALUE,
        )
        .context("open startup registry key")?;
    if !enabled {
        match key.delete_value("autodns") {
            Ok(_) => return Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err).context("delete startup registry value"),
        }
    }
    let exec_path = std::env::current_exe().context("resolve executable")?;
    key.set_value("autodns", &format!("\"{}\"", exec_path.to_string_lossy()))
        .context("write startup registry value")
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn platform_start_at_login_enabled() -> Result<bool> {
    Ok(false)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn platform_set_start_at_login(enabled: bool) -> Result<()> {
    if enabled {
        Err(anyhow!("start at login is not supported on this platform"))
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppEnvironment {
    Development,
    Production,
    Test,
}

pub fn current() -> AppEnvironment {
    if cfg!(test) {
        AppEnvironment::Test
    } else if cfg!(debug_assertions) {
        AppEnvironment::Development
    } else {
        AppEnvironment::Production
    }
}

pub fn app_dir_name() -> &'static str {
    match current() {
        AppEnvironment::Development => "autodns-dev",
        AppEnvironment::Production => "autodns",
        AppEnvironment::Test => "autodns-test",
    }
}

#[cfg(any(target_os = "macos", test))]
pub fn app_identifier() -> &'static str {
    match current() {
        AppEnvironment::Development => "com.autodns.desktop.dev",
        AppEnvironment::Production => "com.autodns.desktop",
        AppEnvironment::Test => "com.autodns.desktop.test",
    }
}

#[cfg(any(target_os = "linux", test))]
pub fn product_name() -> &'static str {
    match current() {
        AppEnvironment::Development => "autodns-dev",
        AppEnvironment::Production => "autodns",
        AppEnvironment::Test => "autodns-test",
    }
}

pub fn tray_id() -> &'static str {
    match current() {
        AppEnvironment::Development => "autodns-dev",
        AppEnvironment::Production => "autodns",
        AppEnvironment::Test => "autodns-test",
    }
}

pub fn default_listen_addr() -> &'static str {
    match current() {
        AppEnvironment::Development => "127.0.0.1:15453",
        AppEnvironment::Production => "127.0.0.1:15353",
        AppEnvironment::Test => "127.0.0.1:15453",
    }
}

pub fn autostart_entry_name() -> &'static str {
    match current() {
        AppEnvironment::Development => "autodns-dev",
        AppEnvironment::Production => "autodns",
        AppEnvironment::Test => "autodns-test",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_environment_uses_isolated_identity() {
        assert_eq!(current(), AppEnvironment::Test);
        assert_eq!(app_dir_name(), "autodns-test");
        assert_eq!(app_identifier(), "com.autodns.desktop.test");
        assert_eq!(product_name(), "autodns-test");
        assert_eq!(tray_id(), "autodns-test");
        assert_eq!(default_listen_addr(), "127.0.0.1:15453");
        assert_eq!(autostart_entry_name(), "autodns-test");
    }
}

use crate::config::desktop_config_dir;
use crate::desktop::{CertificateDefaults, GenerateCertificateRequest, GeneratedCertificate};
use anyhow::{anyhow, Context, Result};
use chrono::{Datelike, Utc};
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair,
    SanType, PKCS_ECDSA_P256_SHA256,
};
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

const DEFAULT_COMMON_NAME: &str = "autodns.local";
const DEFAULT_ORGANIZATION: &str = "autodns";
const DEFAULT_VALID_DAYS: u32 = 3650;
const DEFAULT_FILE_PREFIX: &str = "autodns-local";
const DEFAULT_DOMAINS: &[&str] = &["localhost", "autodns.local"];
const DEFAULT_IPS: &[&str] = &["127.0.0.1", "::1"];

pub fn certificate_defaults() -> Result<CertificateDefaults> {
    Ok(CertificateDefaults {
        common_name: DEFAULT_COMMON_NAME.to_string(),
        organization: DEFAULT_ORGANIZATION.to_string(),
        domains: DEFAULT_DOMAINS
            .iter()
            .map(|item| item.to_string())
            .collect(),
        ip_addresses: DEFAULT_IPS.iter().map(|item| item.to_string()).collect(),
        valid_days: DEFAULT_VALID_DAYS,
        output_dir: default_cert_dir()?.to_string_lossy().to_string(),
        file_prefix: DEFAULT_FILE_PREFIX.to_string(),
    })
}

pub fn generate_certificate(req: GenerateCertificateRequest) -> Result<GeneratedCertificate> {
    let common_name = normalized_or_default(&req.common_name, DEFAULT_COMMON_NAME);
    let organization = req.organization.trim().to_string();
    let valid_days = req.valid_days.unwrap_or(DEFAULT_VALID_DAYS).clamp(1, 8250);
    let output_dir = normalized_output_dir(&req.output_dir)?;
    let file_prefix = sanitize_file_prefix(&req.file_prefix);
    let domains = normalized_list(req.domains, DEFAULT_DOMAINS);
    let ip_addresses = normalized_list(req.ip_addresses, DEFAULT_IPS);
    let parsed_ips = parse_ip_addresses(&ip_addresses)?;

    fs::create_dir_all(&output_dir).context("create certificate directory")?;

    let suffix = next_available_suffix(&output_dir, &file_prefix)?;
    let ca_cert_file = output_dir.join(cert_file_name(&file_prefix, &suffix, "ca"));
    let ca_key_file = output_dir.join(key_file_name(&file_prefix, &suffix, "ca"));
    let cert_file = output_dir.join(cert_file_name(&file_prefix, &suffix, "server"));
    let key_file = output_dir.join(key_file_name(&file_prefix, &suffix, "server"));

    let ca = build_ca_certificate(&common_name, &organization, valid_days)?;
    let server = build_server_certificate(
        &common_name,
        &organization,
        valid_days,
        &domains,
        &parsed_ips,
    )?;

    let ca_pem = ca.serialize_pem().context("serialize ca certificate")?;
    let ca_key_pem = ca.serialize_private_key_pem();
    let cert_pem = server
        .serialize_pem_with_signer(&ca)
        .context("serialize server certificate")?;
    let key_pem = server.serialize_private_key_pem();

    write_new_file(&ca_cert_file, ca_pem.as_bytes(), false)?;
    write_new_file(&ca_key_file, ca_key_pem.as_bytes(), true)?;
    write_new_file(&cert_file, cert_pem.as_bytes(), false)?;
    write_new_file(&key_file, key_pem.as_bytes(), true)?;

    Ok(GeneratedCertificate {
        ca_cert_file: ca_cert_file.to_string_lossy().to_string(),
        ca_key_file: ca_key_file.to_string_lossy().to_string(),
        cert_file: cert_file.to_string_lossy().to_string(),
        key_file: key_file.to_string_lossy().to_string(),
    })
}

fn default_cert_dir() -> Result<PathBuf> {
    Ok(desktop_config_dir()?.join("certs"))
}

fn normalized_output_dir(output_dir: &str) -> Result<PathBuf> {
    let value = output_dir.trim();
    if value.is_empty() {
        return default_cert_dir();
    }
    Ok(PathBuf::from(value))
}

fn normalized_or_default(value: &str, default: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        default.to_string()
    } else {
        value.to_string()
    }
}

fn normalized_list(values: Vec<String>, defaults: &[&str]) -> Vec<String> {
    let mut out = values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if out.is_empty() {
        out = defaults.iter().map(|item| item.to_string()).collect();
    }
    out.sort();
    out.dedup();
    out
}

fn parse_ip_addresses(values: &[String]) -> Result<Vec<IpAddr>> {
    values
        .iter()
        .map(|value| {
            value
                .parse::<IpAddr>()
                .with_context(|| format!("invalid certificate IP address: {value}"))
        })
        .collect()
}

fn sanitize_file_prefix(value: &str) -> String {
    let out = value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if out.is_empty() {
        DEFAULT_FILE_PREFIX.to_string()
    } else {
        out
    }
}

fn next_available_suffix(output_dir: &Path, file_prefix: &str) -> Result<String> {
    if !certificate_files_exist(output_dir, file_prefix, "") {
        return Ok(String::new());
    }
    let timestamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    for index in 0..100 {
        let suffix = if index == 0 {
            timestamp.clone()
        } else {
            format!("{timestamp}-{index}")
        };
        if !certificate_files_exist(output_dir, file_prefix, &suffix) {
            return Ok(suffix);
        }
    }
    Err(anyhow!("could not find available certificate file names"))
}

fn certificate_files_exist(output_dir: &Path, file_prefix: &str, suffix: &str) -> bool {
    [
        cert_file_name(file_prefix, suffix, "ca"),
        key_file_name(file_prefix, suffix, "ca"),
        cert_file_name(file_prefix, suffix, "server"),
        key_file_name(file_prefix, suffix, "server"),
    ]
    .iter()
    .any(|name| output_dir.join(name).exists())
}

fn cert_file_name(file_prefix: &str, suffix: &str, kind: &str) -> String {
    suffixed_name(file_prefix, suffix, kind, "crt")
}

fn key_file_name(file_prefix: &str, suffix: &str, kind: &str) -> String {
    suffixed_name(file_prefix, suffix, kind, "key")
}

fn suffixed_name(file_prefix: &str, suffix: &str, kind: &str, ext: &str) -> String {
    if suffix.is_empty() {
        format!("{file_prefix}-{kind}.{ext}")
    } else {
        format!("{file_prefix}-{suffix}-{kind}.{ext}")
    }
}

fn build_ca_certificate(
    common_name: &str,
    organization: &str,
    valid_days: u32,
) -> Result<Certificate> {
    let mut params = CertificateParams::new(vec![common_name.to_string()]);
    params.distinguished_name = distinguished_name(common_name, organization);
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.not_after = cert_not_after(valid_days);
    params.key_pair = Some(KeyPair::generate(&PKCS_ECDSA_P256_SHA256)?);
    Certificate::from_params(params).context("build ca certificate")
}

fn build_server_certificate(
    common_name: &str,
    organization: &str,
    valid_days: u32,
    domains: &[String],
    ips: &[IpAddr],
) -> Result<Certificate> {
    let mut params = CertificateParams::new(domains.to_vec());
    params.distinguished_name = distinguished_name(common_name, organization);
    params
        .subject_alt_names
        .extend(ips.iter().copied().map(SanType::IpAddress));
    params.not_after = cert_not_after(valid_days);
    params.key_pair = Some(KeyPair::generate(&PKCS_ECDSA_P256_SHA256)?);
    Certificate::from_params(params).context("build server certificate")
}

fn cert_not_after(valid_days: u32) -> time::OffsetDateTime {
    let date = (Utc::now() + chrono::Duration::days(i64::from(valid_days))).date_naive();
    rcgen::date_time_ymd(date.year(), date.month() as u8, date.day() as u8)
}

fn distinguished_name(common_name: &str, organization: &str) -> DistinguishedName {
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, common_name);
    if !organization.trim().is_empty() {
        name.push(DnType::OrganizationName, organization.trim());
    }
    name
}

fn write_new_file(path: &Path, data: &[u8], private_key: bool) -> Result<()> {
    if path.exists() {
        return Err(anyhow!(
            "certificate file already exists: {}",
            path.display()
        ));
    }
    fs::write(path, data).with_context(|| format!("write certificate file: {}", path.display()))?;
    if private_key {
        secure_private_key_file(path)?;
    }
    Ok(())
}

#[cfg(unix)]
fn secure_private_key_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("set private key permissions: {}", path.display()))
}

#[cfg(not(unix))]
fn secure_private_key_file(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn certificate_files_use_suffix_when_default_names_exist() {
        let dir = std::env::temp_dir().join(format!(
            "autodns-cert-test-{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        fs::create_dir_all(&dir).expect("create temp cert dir");
        fs::write(dir.join("autodns-local-ca.crt"), b"exists").expect("write marker");

        let generated = generate_certificate(GenerateCertificateRequest {
            common_name: DEFAULT_COMMON_NAME.into(),
            organization: DEFAULT_ORGANIZATION.into(),
            domains: DEFAULT_DOMAINS
                .iter()
                .map(|item| item.to_string())
                .collect(),
            ip_addresses: DEFAULT_IPS.iter().map(|item| item.to_string()).collect(),
            valid_days: Some(30),
            output_dir: dir.to_string_lossy().to_string(),
            file_prefix: DEFAULT_FILE_PREFIX.into(),
        })
        .expect("generate certificate");

        assert!(generated.cert_file.contains("autodns-local-"));
        assert!(generated.cert_file.ends_with("-server.crt"));
        assert!(Path::new(&generated.ca_cert_file).exists());
        assert!(Path::new(&generated.ca_key_file).exists());
        assert!(Path::new(&generated.cert_file).exists());
        assert!(Path::new(&generated.key_file).exists());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn invalid_certificate_ip_is_rejected() {
        let err = generate_certificate(GenerateCertificateRequest {
            common_name: DEFAULT_COMMON_NAME.into(),
            organization: DEFAULT_ORGANIZATION.into(),
            domains: DEFAULT_DOMAINS
                .iter()
                .map(|item| item.to_string())
                .collect(),
            ip_addresses: vec!["not-an-ip".into()],
            valid_days: Some(30),
            output_dir: std::env::temp_dir().to_string_lossy().to_string(),
            file_prefix: format!(
                "autodns-invalid-ip-{}",
                Utc::now().timestamp_nanos_opt().unwrap_or_default()
            ),
        })
        .expect_err("invalid ip should fail");

        assert!(err.to_string().contains("invalid certificate IP address"));
    }
}

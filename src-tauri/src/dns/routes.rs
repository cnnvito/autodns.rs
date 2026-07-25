use super::*;
use anyhow::{anyhow, Context, Result};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::Arc;

#[derive(Clone, Default)]
pub(crate) struct Hosts {
    pub(crate) entries: Arc<HashMap<String, HostEntry>>,
}

#[derive(Clone)]
pub(crate) struct HostEntry {
    pub(crate) ipv4: Vec<[u8; 4]>,
    pub(crate) ipv6: Vec<[u8; 16]>,
}

#[derive(Clone, Default)]
pub(crate) struct Routes {
    exact: Arc<HashMap<String, RouteEntry>>,
    suffix: Arc<RouteTrie>,
    wildcard: Arc<RouteTrie>,
}

#[derive(Clone)]
pub(crate) struct RouteEntry {
    id: i32,
    domain: String,
    upstreams: Arc<[String]>,
}

#[derive(Clone, Default)]
pub(crate) struct RouteTrie {
    root: RouteTrieNode,
}

#[derive(Clone, Default)]
pub(crate) struct RouteTrieNode {
    route: Option<RouteEntry>,
    children: HashMap<String, RouteTrieNode>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MatchType {
    Exact,
    Suffix,
    Wildcard,
}

pub(crate) fn compile_hosts(raw_entries: &[String]) -> Result<Hosts> {
    let mut entries = HashMap::new();
    for (i, raw) in raw_entries.iter().enumerate() {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(anyhow!("hosts[{}] must not be empty", i));
        }
        let parts: Vec<_> = raw.split('=').collect();
        if parts.len() != 2 {
            return Err(anyhow!("hosts[{}] must contain exactly one '='", i));
        }
        let domain = normalize_domain(parts[0]).with_context(|| {
            format!("hosts[{}] contains invalid domain {:?}", i, parts[0].trim())
        })?;
        if domain.is_empty() {
            return Err(anyhow!("hosts[{}] domain must not be empty", i));
        }
        if entries.contains_key(&domain) {
            return Err(anyhow!("hosts[{}] duplicates domain {:?}", i, domain));
        }
        let mut entry = HostEntry {
            ipv4: Vec::new(),
            ipv6: Vec::new(),
        };
        let mut seen = HashSet::new();
        for part in parts[1].split(',') {
            let ip: IpAddr = part
                .trim()
                .parse()
                .with_context(|| format!("hosts[{}] contains invalid ip {:?}", i, part.trim()))?;
            if !seen.insert(ip) {
                return Err(anyhow!("hosts[{}] contains duplicate ip {:?}", i, ip));
            }
            match ip {
                IpAddr::V4(ip) => entry.ipv4.push(ip.octets()),
                IpAddr::V6(ip) => entry.ipv6.push(ip.octets()),
            }
        }
        if entry.ipv4.is_empty() && entry.ipv6.is_empty() {
            return Err(anyhow!("hosts[{}] must include at least one ip", i));
        }
        entries.insert(domain, entry);
    }
    Ok(Hosts {
        entries: Arc::new(entries),
    })
}

pub(crate) fn compile_routes(raw_rules: &[String], upstreams: &HashSet<String>) -> Result<Routes> {
    let mut exact = HashMap::new();
    let mut suffix = RouteTrie::default();
    let mut wildcard = RouteTrie::default();
    let mut unique = HashSet::new();
    for (i, raw) in raw_rules.iter().enumerate() {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(anyhow!("route[{}] must not be empty", i));
        }
        if raw.matches(':').count() != 1 {
            return Err(anyhow!("route[{}] must contain exactly one ':'", i));
        }
        if raw.matches('=').count() != 1 {
            return Err(anyhow!("route[{}] must contain exactly one '='", i));
        }
        let Some((match_part, rest)) = raw.split_once(':') else {
            return Err(anyhow!("route[{}] must contain exactly one ':'", i));
        };
        let Some((domain_part, upstream_part)) = rest.split_once('=') else {
            return Err(anyhow!("route[{}] must contain exactly one '='", i));
        };
        let match_type = match match_part.trim() {
            "exact" => MatchType::Exact,
            "suffix" => MatchType::Suffix,
            "wildcard" => MatchType::Wildcard,
            other => {
                return Err(anyhow!(
                    "route[{}] has unsupported match type {:?}",
                    i,
                    other
                ))
            }
        };
        let mut domain = normalize_domain(domain_part).with_context(|| {
            format!(
                "route[{}] contains invalid domain {:?}",
                i,
                domain_part.trim()
            )
        })?;
        if domain.is_empty() {
            return Err(anyhow!("route[{}] domain must not be empty", i));
        }
        if match_type == MatchType::Wildcard {
            if !domain.starts_with("*.") {
                return Err(anyhow!("route[{}] wildcard domain must start with '*.'", i));
            }
            domain = domain.trim_start_matches("*.").to_string();
            if domain.is_empty() || domain.contains('*') {
                return Err(anyhow!("route[{}] wildcard domain is invalid", i));
            }
        } else if domain.contains('*') {
            return Err(anyhow!(
                "route[{}] wildcard domain requires wildcard match type",
                i
            ));
        }
        let unique_key = (match_type as u8, domain.clone());
        if !unique.insert(unique_key) {
            return Err(anyhow!("duplicate route for {:?}", domain));
        }
        let mut names = Vec::new();
        let mut seen = HashSet::new();
        for upstream in upstream_part.split(',') {
            let name = upstream.trim();
            if name.is_empty() {
                return Err(anyhow!("route[{}] upstream name must not be empty", i));
            }
            if !upstreams.contains(name) {
                return Err(anyhow!(
                    "route[{}] references unknown upstream {:?}",
                    i,
                    name
                ));
            }
            if !seen.insert(name.to_string()) {
                return Err(anyhow!(
                    "route[{}] contains duplicate upstream {:?}",
                    i,
                    name
                ));
            }
            names.push(name.to_string());
        }
        let entry = RouteEntry {
            id: (i + 1) as i32,
            domain: domain.clone(),
            upstreams: names.into(),
        };
        match match_type {
            MatchType::Exact => {
                exact.insert(domain, entry);
            }
            MatchType::Suffix => suffix.insert(&domain, entry),
            MatchType::Wildcard => wildcard.insert(&domain, entry),
        }
    }
    Ok(Routes {
        exact: Arc::new(exact),
        suffix: Arc::new(suffix),
        wildcard: Arc::new(wildcard),
    })
}

impl Routes {
    // `domain` must already be normalized; parsing computes it once per query.
    pub(crate) fn select<'a>(
        &'a self,
        domain: &str,
        defaults: &'a [String],
    ) -> (i32, &'a [String]) {
        if let Some(entry) = self.exact.get(domain) {
            return (entry.id, &entry.upstreams);
        }
        let matched = self
            .suffix
            .longest_match(domain, true)
            .into_iter()
            .chain(self.wildcard.longest_match(domain, false))
            .max_by_key(|entry| entry.domain.len());
        if let Some(entry) = matched {
            (entry.id, &entry.upstreams)
        } else {
            (0, defaults)
        }
    }
}

impl RouteTrie {
    fn insert(&mut self, domain: &str, entry: RouteEntry) {
        let mut node = &mut self.root;
        for label in domain.rsplit('.') {
            node = node.children.entry(label.to_string()).or_default();
        }
        node.route = Some(entry);
    }

    fn longest_match(&self, domain: &str, include_exact: bool) -> Option<&RouteEntry> {
        let mut node = &self.root;
        let mut matched = None;
        let label_count = domain.split('.').count();
        for (depth, label) in domain.rsplit('.').enumerate() {
            let Some(next) = node.children.get(label) else {
                break;
            };
            node = next;
            if let Some(route) = &node.route {
                if include_exact || depth + 1 < label_count {
                    matched = Some(route);
                }
            }
        }
        matched
    }
}

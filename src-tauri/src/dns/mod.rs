use crate::config::{parse_bootstrap_dns_servers, parse_go_duration, CoreConfig};
use crate::desktop::DnsLookupResult;
use crate::history::{DnsHistoryEvent, DnsHistoryRecorder};
use crate::logging::LogBuffer;
use anyhow::{anyhow, Result};
use chrono::Utc;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, AtomicU8};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex as AsyncMutex;

mod cache;
mod health;
mod routes;
mod runtime;
#[cfg(test)]
mod tests;
mod upstream;
mod wire;

pub(crate) use cache::*;
pub(crate) use health::*;
pub(crate) use routes::*;
pub(crate) use runtime::*;
pub(crate) use upstream::*;
pub(crate) use wire::*;

pub(crate) const DEFAULT_RESOLVER_TIMEOUT: Duration = Duration::from_secs(5);
#[derive(Clone)]
pub(crate) struct Resolver {
    pub(crate) default_upstreams: Arc<[String]>,
    pub(crate) hosts: Hosts,
    pub(crate) routes: Routes,
    pub(crate) cache: DnsCache,
    pub(crate) history: DnsHistoryRecorder,
    pub(crate) clients: HashMap<String, UpstreamClient>,
    pub(crate) health: Arc<HealthMonitor>,
    pub(crate) timeout: Option<Duration>,
    pub(crate) ipv6_enabled: bool,
    pub(crate) logs: LogBuffer,
}

pub(crate) struct NegativeResponse {
    response: Vec<u8>,
    upstream_name: String,
    upstream_protocol: String,
    error: String,
}
pub(crate) fn build_resolver(
    cfg: &CoreConfig,
    logs: LogBuffer,
    history: DnsHistoryRecorder,
) -> Result<Resolver> {
    let bootstrap = {
        let servers = parse_bootstrap_dns_servers(&cfg.resolver.bootstrap_dns)?;
        (!servers.is_empty()).then(|| Arc::new(BootstrapResolver::new(servers)))
    };
    let proxies = cfg
        .resolver
        .proxies
        .iter()
        .map(|item| Ok((item.name.clone(), proxy_endpoint_from_raw(&item.endpoint)?)))
        .collect::<Result<HashMap<_, _>>>()?;
    let mut clients = HashMap::new();
    for item in &cfg.resolver.upstreams {
        let proxy_name = if item.proxy.is_empty() {
            &cfg.resolver.default_proxy
        } else {
            &item.proxy
        };
        let endpoint = endpoint_from_config(item)?;
        let proxy = if proxy_name.is_empty() {
            None
        } else {
            Some(
                proxies
                    .get(proxy_name)
                    .cloned()
                    .ok_or_else(|| anyhow!("proxy references unknown proxy {:?}", proxy_name))?,
            )
        };
        let http = if endpoint.scheme == "http" || endpoint.scheme == "https" {
            Some(build_http_client(proxy.as_ref(), bootstrap.as_ref())?)
        } else {
            None
        };
        let bootstrap = if proxy.is_none() {
            bootstrap.clone()
        } else {
            None
        };
        clients.insert(
            item.name.clone(),
            UpstreamClient {
                name: item.name.clone(),
                endpoint,
                proxy,
                bootstrap,
                http,
                state: Arc::new(AtomicU8::new(UpstreamPoolState::Cold as u8)),
                transport_failure_streak: Arc::new(AtomicU32::new(0)),
                connect_gate: Arc::new(AsyncMutex::new(())),
                resolve_gate: Arc::new(AsyncMutex::new(())),
                failure_threshold: cfg.healthcheck.failure_threshold.max(1),
                endpoint_addrs: Arc::new(Mutex::new(None)),
                udp: Arc::new(Mutex::new(None)),
                socks5_udp: Arc::new(Mutex::new(None)),
                tcp: Arc::new(Mutex::new(None)),
                dot: Arc::new(Mutex::new(None)),
                doq: Arc::new(Mutex::new(None)),
            },
        );
    }

    let health = HealthMonitor::new(
        cfg.healthcheck.enabled,
        cfg.healthcheck.failure_threshold,
        cfg.healthcheck.recovery_threshold,
        clients.keys().cloned().collect(),
    );

    Ok(Resolver {
        default_upstreams: cfg
            .resolver
            .upstreams
            .iter()
            .map(|item| item.name.clone())
            .collect::<Vec<_>>()
            .into(),
        hosts: compile_hosts(&cfg.resolver.hosts)?,
        routes: compile_routes(
            &cfg.resolver.routes,
            &cfg.resolver
                .upstreams
                .iter()
                .map(|item| item.name.clone())
                .collect::<HashSet<_>>(),
        )?,
        cache: DnsCache::new(cfg),
        history,
        clients,
        health,
        timeout: Some(if cfg.resolver.timeout.is_empty() {
            DEFAULT_RESOLVER_TIMEOUT
        } else {
            parse_go_duration(&cfg.resolver.timeout)?
        }),
        ipv6_enabled: cfg.resolver.ipv6_enabled,
        logs,
    })
}

impl Resolver {
    fn clear_cache(&self) -> usize {
        self.cache.clear()
    }

    pub(crate) async fn check_upstream_health(
        &self,
        upstream_name: &str,
        domain: &str,
        timeout: Duration,
    ) -> Result<bool> {
        let client = self
            .clients
            .get(upstream_name)
            .cloned()
            .ok_or_else(|| anyhow!("upstream not found: {upstream_name}"))?;
        let healthy = probe_upstream_health(client, self.health.clone(), domain, timeout).await;
        if healthy {
            // A user-initiated check that succeeds is decisive: restore the upstream to query
            // rotation immediately instead of waiting for `recovery_threshold` automatic probes.
            self.health.restore(upstream_name);
        }
        Ok(healthy)
    }

    pub(crate) async fn lookup(&self, domain: &str, record_type: &str) -> Result<DnsLookupResult> {
        let qtype = lookup_qtype(record_type)?;
        let domain = normalize_domain(domain)?;
        if domain.is_empty() {
            return Err(anyhow!("domain must not be empty"));
        }

        let req = build_query(&domain, qtype);
        let started = Instant::now();
        let resp = self
            .resolve(req)
            .await
            .map_err(|_| anyhow!("resolve failed"))?;
        let duration_ms = started.elapsed().as_millis();
        let response_code = response_code_label(rcode(&resp));
        let answer_count = answer_count(&resp).unwrap_or(0) as usize;
        let records = parse_lookup_records(&resp)?;

        Ok(DnsLookupResult {
            domain,
            record_type: qtype_label(qtype).to_string(),
            response_code,
            answer_count,
            duration_ms,
            records,
        })
    }

    async fn resolve(&self, req: Vec<u8>) -> std::result::Result<Vec<u8>, Vec<u8>> {
        let history_started_at = Utc::now().to_rfc3339();
        let history_started = Instant::now();
        let debug_logs = self.logs.debug_enabled();
        let question = match parse_question(&req) {
            Ok(question) => question,
            Err(err) => {
                if debug_logs {
                    self.logs
                        .push("debug", format!("dns query parse failed: {err}"));
                }
                return Err(req);
            }
        };
        let qname = question.normalized_qname.as_str();
        let qtype = qtype_label(question.qtype);
        if question.qtype == TYPE_AAAA && !self.ipv6_enabled {
            if debug_logs {
                self.logs.push(
                    "debug",
                    format!("dns query {qname} {qtype} answered locally because IPv6 is disabled"),
                );
            }
            let resp = empty_success_response(&question);
            self.record_history_response(
                &question,
                &history_started_at,
                history_started.elapsed(),
                "local",
                -1,
                "",
                "",
                0,
                &resp,
                "",
                Arc::from(""),
            );
            return Ok(resp);
        }
        if let Some(resp) = self.resolve_hosts(&question) {
            if debug_logs {
                self.logs.push(
                    "debug",
                    format!("dns query {qname} {qtype} answered from hosts"),
                );
            }
            self.record_history_response(
                &question,
                &history_started_at,
                history_started.elapsed(),
                "hosts",
                -1,
                "",
                "",
                0,
                &resp,
                "",
                summarize_answers(&resp),
            );
            return Ok(resp);
        }

        let (route_id, selected) = self
            .routes
            .select(&question.normalized_qname, &self.default_upstreams);
        if debug_logs {
            self.logs.push(
                "debug",
                format!(
                    "dns query {qname} {qtype} selected route {route_id} upstreams {}",
                    selected.join(",")
                ),
            );
        }
        let key = CacheKey {
            qname: question.normalized_qname.clone(),
            qtype: question.qtype,
            qclass: question.qclass,
            route_id,
        };
        if let Some(cached) = self.cache.get(&key, question.id) {
            if debug_logs {
                self.logs.push(
                    "debug",
                    format!("dns query {qname} {qtype} route {route_id} answered from cache"),
                );
            }
            self.record_history_response(
                &question,
                &history_started_at,
                history_started.elapsed(),
                "cache",
                route_id,
                &cached.upstream_name,
                &cached.upstream_protocol,
                0,
                &cached.response,
                "",
                cached.answers.clone(),
            );
            return Ok(cached.response);
        }

        // Upstreams are intentionally tried in configured order. A negative response
        // (NOERROR with no answers, or NXDOMAIN) is not final here: later upstreams may
        // still have an answer for split-horizon, geo, or policy-routed domains. Keep the
        // latest negative as a fallback and only return it after every selected upstream
        // has failed to produce an answer.
        let eligibility = self.health.query_eligibility(selected);
        let has_eligible_upstream = selected
            .iter()
            .zip(eligibility.iter())
            .any(|(name, eligible)| *eligible && self.clients.contains_key(name));
        let mut last_negative = None;
        let mut attempt_count = 0usize;
        let mut last_error = None;
        for (name, eligible) in selected.iter().zip(eligibility.iter()) {
            let Some(client) = self.clients.get(name) else {
                if debug_logs {
                    self.logs.push(
                        "debug",
                        format!("dns query {qname} {qtype} route {route_id} skipped missing upstream {name}"),
                    );
                }
                continue;
            };
            if !eligible {
                if debug_logs {
                    self.logs.push(
                        "debug",
                        format!("dns query {qname} {qtype} route {route_id} skipped unhealthy upstream {name}"),
                    );
                }
                last_error.get_or_insert_with(|| "all selected upstreams unhealthy".to_string());
                continue;
            }
            if has_eligible_upstream && client.should_skip_for_query() {
                if debug_logs {
                    self.logs.push(
                        "debug",
                        format!("dns query {qname} {qtype} route {route_id} skipped connecting upstream {name}"),
                    );
                }
                last_error.get_or_insert_with(|| {
                    "upstream connection is already being established".to_string()
                });
                continue;
            }
            attempt_count += 1;
            let started = Instant::now();
            if debug_logs {
                self.logs.push(
                    "debug",
                    format!(
                        "dns query {qname} {qtype} route {route_id} via upstream {} ({}://{})",
                        client.name, client.endpoint.scheme, client.endpoint.address
                    ),
                );
            }
            match client.exchange(&req, self.timeout).await {
                Ok(resp) => {
                    let resp = self.filter_response(resp);
                    match classify(&resp) {
                        ResponseClass::Answer => {
                            self.health.record_query_success(name, started.elapsed());
                            self.cache.set(
                                key.clone(),
                                &resp,
                                false,
                                &client.name,
                                &client.endpoint.scheme,
                            );
                            if debug_logs {
                                let duration_ms = started.elapsed().as_millis();
                                let rcode_label = response_code_label(rcode(&resp));
                                let answers = answer_count(&resp).unwrap_or(0);
                                self.logs.push(
                                    "debug",
                                    format!(
                                        "dns query {qname} {qtype} upstream {} answered in {duration_ms}ms rcode {rcode_label} answers {answers}",
                                        client.name
                                    ),
                                );
                            }
                            self.record_history_response(
                                &question,
                                &history_started_at,
                                history_started.elapsed(),
                                "upstream",
                                route_id,
                                &client.name,
                                &client.endpoint.scheme,
                                attempt_count,
                                &resp,
                                "",
                                summarize_answers(&resp),
                            );
                            return Ok(resp);
                        }
                        ResponseClass::Negative => {
                            let error = "upstream returned no answer";
                            self.health
                                .record_negative_response(name, started.elapsed());
                            let history_error = if rcode(&resp) == Some(RCODE_NAME_ERROR) {
                                String::new()
                            } else {
                                error.to_string()
                            };
                            if debug_logs {
                                let duration_ms = started.elapsed().as_millis();
                                let rcode_label = response_code_label(rcode(&resp));
                                self.logs.push(
                                    "debug",
                                    format!(
                                        "dns query {qname} {qtype} upstream {} returned no answer in {duration_ms}ms rcode {rcode_label}",
                                        client.name
                                    ),
                                );
                            }
                            // Preserve this empty/NXDOMAIN result as the fallback, but keep
                            // walking the upstream list so a later upstream can override it
                            // with an actual answer.
                            last_negative = Some(NegativeResponse {
                                response: resp,
                                upstream_name: client.name.clone(),
                                upstream_protocol: client.endpoint.scheme.clone(),
                                error: history_error,
                            });
                        }
                        ResponseClass::Retry => {
                            let error = "upstream returned retryable response";
                            self.health
                                .record_failure(name, FailureKind::RetryableResponse, error);
                            last_error = Some(error.to_string());
                            if debug_logs {
                                let duration_ms = started.elapsed().as_millis();
                                let rcode_label = response_code_label(rcode(&resp));
                                self.logs.push(
                                    "debug",
                                    format!(
                                        "dns query {qname} {qtype} upstream {} returned retryable response in {duration_ms}ms rcode {rcode_label}",
                                        client.name
                                    ),
                                );
                            }
                        }
                    }
                }
                Err(err) => {
                    last_error = Some(err.to_string());
                    let connecting = is_upstream_connecting_error(&err);
                    if debug_logs {
                        let duration_ms = started.elapsed().as_millis();
                        self.logs.push(
                            "debug",
                            format!(
                                "dns query {qname} {qtype} upstream {} failed in {duration_ms}ms: {err}",
                                client.name
                            ),
                        );
                    }
                    if !connecting {
                        self.health
                            .record_failure(name, FailureKind::Transport, err.to_string());
                    }
                }
            }
        }

        if let Some(negative) = last_negative {
            // No upstream returned an answer. At this point the best response is the last
            // negative result we saw, which preserves the upstream's real RCODE instead of
            // converting an empty result into SERVFAIL.
            self.cache.set(
                key,
                &negative.response,
                true,
                &negative.upstream_name,
                &negative.upstream_protocol,
            );
            if debug_logs {
                self.logs.push(
                    "debug",
                    format!("dns query {qname} {qtype} returning last negative response"),
                );
            }
            self.record_history_response(
                &question,
                &history_started_at,
                history_started.elapsed(),
                "upstream",
                route_id,
                &negative.upstream_name,
                &negative.upstream_protocol,
                attempt_count,
                &negative.response,
                &negative.error,
                summarize_answers(&negative.response),
            );
            return Ok(negative.response);
        }
        if debug_logs {
            self.logs.push(
                "debug",
                format!("dns query {qname} {qtype} returning SERVFAIL"),
            );
        }
        let resp = servfail_response(&req);
        self.record_history_response(
            &question,
            &history_started_at,
            history_started.elapsed(),
            "error",
            route_id,
            "",
            "",
            attempt_count,
            &resp,
            last_error.as_deref().unwrap_or("all upstreams failed"),
            Arc::from(""),
        );
        Ok(resp)
    }

    fn record_history_response(
        &self,
        question: &Question,
        started_at: &str,
        duration: Duration,
        source: &str,
        route_id: i32,
        upstream_name: &str,
        upstream_protocol: &str,
        attempt_count: usize,
        resp: &[u8],
        error: &str,
        answers: Arc<str>,
    ) {
        self.history.record(DnsHistoryEvent {
            started_at: started_at.to_string(),
            domain: question.normalized_qname.clone(),
            record_type: qtype_label(question.qtype).to_string(),
            qclass: question.qclass,
            source: source.to_string(),
            route_id,
            upstream_name: upstream_name.to_string(),
            upstream_protocol: upstream_protocol.to_string(),
            duration_ms: duration.as_millis(),
            attempt_count,
            response_code: response_code_label(rcode(resp)),
            min_ttl: answer_min_ttl(resp).ok().flatten(),
            answers,
            error: error.to_string(),
        });
    }

    fn filter_response(&self, resp: Vec<u8>) -> Vec<u8> {
        if self.ipv6_enabled {
            return resp;
        }
        // Keep this response-level filter even for non-AAAA questions. Upstreams may include
        // AAAA records alongside CNAME chains, HTTPS/SVCB answers, or additional records; when
        // IPv6 is disabled, those embedded IPv6 addresses must be stripped before replying.
        strip_aaaa_records(&resp).unwrap_or(resp)
    }

    fn resolve_hosts(&self, question: &Question) -> Option<Vec<u8>> {
        let entry = self.hosts.entries.get(&question.normalized_qname)?;
        match question.qtype {
            TYPE_A if !entry.ipv4.is_empty() => Some(hosts_response(question, &entry.ipv4, &[])),
            TYPE_AAAA if !entry.ipv6.is_empty() => Some(hosts_response(question, &[], &entry.ipv6)),
            _ => None,
        }
    }
}

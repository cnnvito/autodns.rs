use super::*;
use crate::config::CoreConfig;
use crate::desktop::{localized_error_message, HealthState, ProxyHealth, UpstreamHealth};
use chrono::Utc;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{watch, OwnedSemaphorePermit, Semaphore};

pub(crate) const MAX_CONCURRENT_HEALTHCHECKS: usize = 4;
pub(crate) const HEALTHCHECK_STAGGER_STEP: Duration = Duration::from_millis(200);
pub(crate) const HEALTHCHECK_FAILURE_RETRY_BASE_DELAY: Duration = Duration::from_secs(1);
pub(crate) const HEALTHCHECK_FAILURE_RETRY_MAX_DELAY: Duration = Duration::from_secs(30);
pub(crate) const HEALTHCHECK_RECOVERY_CONFIRM_DELAY: Duration = Duration::from_millis(200);
pub(crate) type HealthListener = Arc<dyn Fn() + Send + Sync>;
#[derive(Clone)]
pub struct HealthMonitor {
    states: Arc<Mutex<HashMap<String, HealthRecord>>>,
    listener: Arc<Mutex<Option<HealthListener>>>,
    enabled: bool,
    failure_threshold: u32,
    recovery_threshold: u32,
}

#[derive(Clone)]
pub(crate) struct HealthRecord {
    healthy: bool,
    degraded: bool,
    failure_streak: u32,
    recovery_streak: u32,
    // Consecutive failures from any source (real queries and probes alike), reset by any
    // success. Drives the probe backoff delay, unlike `failure_streak` which only tracks
    // failures of a healthy upstream to decide when to display it as unhealthy.
    consecutive_failures: u32,
    failure_count: u64,
    last_error: Option<String>,
    last_success_at: Option<String>,
    last_query_success_at: Option<Instant>,
    latency_ms: Option<u128>,
}

pub(crate) struct HealthSnapshot {
    enabled: bool,
    states: HashMap<String, HealthRecord>,
}

#[derive(Clone, Default)]
pub(crate) struct UpstreamDiagnostics {
    failure_count: u64,
    last_error: Option<String>,
    last_success_at: Option<String>,
    latency_ms: Option<u128>,
}

#[derive(Clone, Copy)]
pub(crate) enum FailureKind {
    Transport,
    RetryableResponse,
}

pub(crate) async fn run_health_loop(
    client: UpstreamClient,
    health: Arc<HealthMonitor>,
    domain: String,
    interval: Duration,
    timeout: Duration,
    initial_delay: Duration,
    probe_limit: Arc<Semaphore>,
    stop: &mut watch::Receiver<bool>,
) {
    if !initial_delay.is_zero() {
        tokio::select! {
            _ = stop.changed() => return,
            _ = tokio::time::sleep(initial_delay) => {}
        }
    }
    loop {
        if health.recent_healthy_query_success(&client.name, interval) {
            tokio::select! {
                _ = stop.changed() => break,
                _ = tokio::time::sleep(health.probe_delay(&client.name, interval)) => {}
            }
            continue;
        }
        let permit = wait_for_healthcheck_permit(&probe_limit, stop).await;
        let Some(permit) = permit else {
            break;
        };
        probe_upstream_health(client.clone(), health.clone(), &domain, timeout).await;
        drop(permit);
        let delay = health.probe_delay(&client.name, interval);
        tokio::select! {
            _ = stop.changed() => break,
            _ = tokio::time::sleep(delay) => {}
        }
    }
}

pub(crate) async fn probe_upstream_health(
    client: UpstreamClient,
    health: Arc<HealthMonitor>,
    domain: &str,
    timeout: Duration,
) -> bool {
    let qtype = if domain == "." { TYPE_NS } else { TYPE_A };
    let req = build_query(domain, qtype);
    let started = Instant::now();
    match client.exchange(&req, Some(timeout)).await {
        Ok(resp) => match classify_probe_response(&resp) {
            ProbeOutcome::Healthy => {
                health.record_probe_success(&client.name, started.elapsed());
                true
            }
            ProbeOutcome::InvalidResponse => {
                health.record_probe_failure(&client.name, "healthcheck returned invalid response");
                false
            }
            ProbeOutcome::RetryableResponse => {
                health
                    .record_probe_failure(&client.name, "healthcheck returned retryable response");
                false
            }
        },
        Err(err) if is_upstream_connecting_error(&err) => false,
        Err(err) => {
            health.record_probe_failure(&client.name, err.to_string());
            false
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProbeOutcome {
    Healthy,
    InvalidResponse,
    RetryableResponse,
}

/// A probe only proves health when the upstream returned a parseable DNS header whose RCODE is
/// not SERVFAIL/REFUSED. Truncated or garbage responses must not count as healthy: an upstream
/// replying bytes we cannot parse is not usable for real queries either.
pub(crate) fn classify_probe_response(resp: &[u8]) -> ProbeOutcome {
    match rcode(resp) {
        None => ProbeOutcome::InvalidResponse,
        Some(RCODE_SERVER_FAILURE | RCODE_REFUSED) => ProbeOutcome::RetryableResponse,
        Some(_) => ProbeOutcome::Healthy,
    }
}

pub(crate) async fn wait_for_healthcheck_permit(
    probe_limit: &Arc<Semaphore>,
    stop: &mut watch::Receiver<bool>,
) -> Option<OwnedSemaphorePermit> {
    tokio::select! {
        _ = stop.changed() => None,
        permit = probe_limit.clone().acquire_owned() => permit.ok(),
    }
}

pub(crate) fn healthcheck_failure_backoff_delay(
    upstream_name: &str,
    failure_streak: u32,
    interval: Duration,
) -> Duration {
    let max_delay = interval.min(HEALTHCHECK_FAILURE_RETRY_MAX_DELAY);
    let base_delay = HEALTHCHECK_FAILURE_RETRY_BASE_DELAY.min(max_delay);
    if base_delay.is_zero() {
        return Duration::ZERO;
    }
    let exponent = failure_streak.saturating_sub(1).min(16);
    let multiplier = 1u32 << exponent;
    let delay = base_delay.saturating_mul(multiplier).min(max_delay);
    with_stable_jitter(delay, upstream_name, failure_streak).min(max_delay)
}

pub(crate) fn with_stable_jitter(
    delay: Duration,
    upstream_name: &str,
    failure_streak: u32,
) -> Duration {
    let delay_ms = delay.as_millis();
    let jitter_window = delay_ms / 5;
    if jitter_window == 0 {
        return delay;
    }
    let mut hash = u64::from(failure_streak);
    for byte in upstream_name.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(u64::from(byte));
    }
    let span = (jitter_window * 2 + 1) as u64;
    let offset = u128::from(hash % span);
    let jittered_ms = delay_ms + offset;
    let jittered_ms = jittered_ms.saturating_sub(jitter_window);
    Duration::from_millis(jittered_ms.min(u128::from(u64::MAX)) as u64)
}

impl HealthMonitor {
    pub(crate) fn new(
        enabled: bool,
        failure_threshold: u32,
        recovery_threshold: u32,
        names: Vec<String>,
    ) -> Arc<Self> {
        let states = names
            .into_iter()
            .map(|name| {
                (
                    name,
                    HealthRecord {
                        healthy: true,
                        degraded: false,
                        failure_streak: 0,
                        recovery_streak: 0,
                        consecutive_failures: 0,
                        failure_count: 0,
                        last_error: None,
                        last_success_at: None,
                        last_query_success_at: None,
                        latency_ms: None,
                    },
                )
            })
            .collect();
        Arc::new(Self {
            states: Arc::new(Mutex::new(states)),
            listener: Arc::new(Mutex::new(None)),
            enabled,
            failure_threshold: failure_threshold.max(1),
            recovery_threshold: recovery_threshold.max(1),
        })
    }

    pub(crate) fn set_listener(&self, listener: HealthListener) {
        *self.listener.lock() = Some(listener);
    }

    pub(crate) fn notify_listener(&self) {
        let listener = self.listener.lock().clone();
        if let Some(listener) = listener {
            listener();
        }
    }

    pub(crate) fn snapshot(&self) -> HealthSnapshot {
        HealthSnapshot {
            enabled: self.enabled,
            states: self.states.lock().clone(),
        }
    }

    pub(crate) fn query_eligibility(&self, names: &[String]) -> Vec<bool> {
        if !self.enabled {
            return vec![true; names.len()];
        }
        let states = self.states.lock();
        names
            .iter()
            .map(|name| {
                states
                    .get(name)
                    .map(|state| state.healthy && !state.degraded)
                    .unwrap_or(false)
            })
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn recent_query_success(&self, name: &str, window: Duration) -> bool {
        self.states
            .lock()
            .get(name)
            .and_then(|state| state.last_query_success_at)
            .map(|last_success| last_success.elapsed() < window)
            .unwrap_or(false)
    }

    pub(crate) fn recent_healthy_query_success(&self, name: &str, window: Duration) -> bool {
        self.states
            .lock()
            .get(name)
            .filter(|state| state.healthy && !state.degraded)
            .and_then(|state| state.last_query_success_at)
            .map(|last_success| last_success.elapsed() < window)
            .unwrap_or(false)
    }

    pub(crate) fn probe_delay(&self, name: &str, interval: Duration) -> Duration {
        let Some(state) = self.states.lock().get(name).cloned() else {
            return interval;
        };
        if state.degraded && state.recovery_streak > 0 {
            return HEALTHCHECK_RECOVERY_CONFIRM_DELAY;
        }
        if state.degraded {
            return healthcheck_failure_backoff_delay(name, state.consecutive_failures, interval);
        }
        interval
    }

    pub(crate) fn record_query_success(&self, name: &str, latency: Duration) {
        self.record_success(name, latency, true);
    }

    pub(crate) fn record_probe_success(&self, name: &str, latency: Duration) {
        self.record_success(name, latency, false);
    }

    pub(crate) fn record_probe_failure(&self, name: &str, err: impl Into<String>) {
        self.record_failure(name, FailureKind::Transport, err);
    }

    pub(crate) fn record_negative_response(&self, name: &str, latency: Duration) {
        self.record_success(name, latency, true);
    }

    pub(crate) fn record_success(&self, name: &str, latency: Duration, real_query: bool) {
        {
            let mut states = self.states.lock();
            let Some(state) = states.get_mut(name) else {
                return;
            };
            state.last_error = None;
            state.last_success_at = Some(Utc::now().to_rfc3339());
            if real_query {
                state.last_query_success_at = Some(Instant::now());
            }
            state.latency_ms = Some(latency.as_millis());
            state.consecutive_failures = 0;
            if self.enabled {
                state.failure_streak = 0;
                if state.degraded {
                    state.recovery_streak += 1;
                    if state.recovery_streak >= self.recovery_threshold {
                        state.healthy = true;
                        state.degraded = false;
                        state.recovery_streak = 0;
                    }
                } else {
                    state.recovery_streak = 0;
                }
            } else {
                state.degraded = false;
            }
        }
        self.notify_listener();
    }

    pub(crate) fn record_failure(&self, name: &str, _kind: FailureKind, err: impl Into<String>) {
        {
            let mut states = self.states.lock();
            let Some(state) = states.get_mut(name) else {
                return;
            };
            state.failure_count += 1;
            state.last_error = Some(err.into());
            state.consecutive_failures = state.consecutive_failures.saturating_add(1);
            if self.enabled {
                state.degraded = true;
                state.recovery_streak = 0;
                if state.healthy {
                    state.failure_streak += 1;
                    if state.failure_streak >= self.failure_threshold {
                        state.healthy = false;
                        state.failure_streak = 0;
                    }
                }
            }
        }
        self.notify_listener();
    }

    /// Immediately return an upstream to query rotation. Used after a successful
    /// user-initiated health check: one on-demand probe the user just watched succeed is
    /// decisive evidence, unlike automatic recovery which waits for `recovery_threshold`
    /// consecutive probe successes.
    pub(crate) fn restore(&self, name: &str) {
        {
            let mut states = self.states.lock();
            let Some(state) = states.get_mut(name) else {
                return;
            };
            state.healthy = true;
            state.degraded = false;
            state.failure_streak = 0;
            state.recovery_streak = 0;
            state.consecutive_failures = 0;
        }
        self.notify_listener();
    }
}
impl HealthSnapshot {
    pub(crate) fn healthy(&self, name: &str) -> bool {
        if !self.enabled {
            return true;
        }
        self.states
            .get(name)
            .map(|state| state.healthy)
            .unwrap_or(false)
    }

    pub(crate) fn diagnostics(&self, name: &str) -> UpstreamDiagnostics {
        self.states
            .get(name)
            .map(|state| UpstreamDiagnostics {
                failure_count: state.failure_count,
                last_error: state.last_error.clone(),
                last_success_at: state.last_success_at.clone(),
                latency_ms: state.latency_ms,
            })
            .unwrap_or_default()
    }
}
pub fn build_upstream_health(
    cfg: &CoreConfig,
    health: Option<&Arc<HealthMonitor>>,
) -> Vec<UpstreamHealth> {
    let snapshot = health.map(|h| h.snapshot());
    cfg.resolver
        .upstreams
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let proxy = if item.proxy.is_empty() {
                cfg.resolver.default_proxy.clone()
            } else {
                item.proxy.clone()
            };
            let diagnostics = snapshot
                .as_ref()
                .map(|h| h.diagnostics(&item.name))
                .unwrap_or_default();
            let last_error_message = diagnostics
                .last_error
                .as_deref()
                .map(localized_error_message);
            UpstreamHealth {
                name: item.name.clone(),
                endpoint: item.endpoint.clone(),
                protocol: item.endpoint.split(':').next().unwrap_or("").to_string(),
                proxy,
                order: index + 1,
                health: snapshot
                    .as_ref()
                    .map(|h| {
                        if h.healthy(&item.name) {
                            HealthState::Healthy
                        } else {
                            HealthState::Unhealthy
                        }
                    })
                    .unwrap_or(HealthState::Unknown),
                failure_count: diagnostics.failure_count,
                last_error: diagnostics.last_error,
                last_error_message,
                last_success_at: diagnostics.last_success_at,
                latency_ms: diagnostics.latency_ms,
            }
        })
        .collect()
}

pub fn build_proxy_health(
    cfg: &CoreConfig,
    health: Option<&Arc<HealthMonitor>>,
) -> Vec<ProxyHealth> {
    let snapshot = health.map(|h| h.snapshot());
    let mut upstreams_by_proxy: HashMap<String, Vec<String>> = HashMap::new();
    for item in &cfg.resolver.upstreams {
        let proxy_name = if item.proxy.is_empty() {
            &cfg.resolver.default_proxy
        } else {
            &item.proxy
        };
        if !proxy_name.is_empty() {
            upstreams_by_proxy
                .entry(proxy_name.clone())
                .or_default()
                .push(item.name.clone());
        }
    }
    cfg.resolver
        .proxies
        .iter()
        .map(|proxy| {
            let upstreams = upstreams_by_proxy
                .get(&proxy.name)
                .cloned()
                .unwrap_or_default();
            let state = if upstreams.is_empty() {
                HealthState::Unused
            } else if let Some(health) = &snapshot {
                if upstreams.iter().any(|name| health.healthy(name)) {
                    HealthState::Healthy
                } else {
                    HealthState::Unhealthy
                }
            } else {
                HealthState::Unknown
            };
            ProxyHealth {
                name: proxy.name.clone(),
                endpoint: proxy.endpoint.clone(),
                health: state,
                upstreams,
            }
        })
        .collect()
}

pub fn mark_upstreams_unknown(items: &[UpstreamHealth]) -> Vec<UpstreamHealth> {
    items
        .iter()
        .cloned()
        .map(|mut item| {
            item.health = HealthState::Unknown;
            item
        })
        .collect()
}

pub fn mark_proxies_unknown(items: &[ProxyHealth]) -> Vec<ProxyHealth> {
    items
        .iter()
        .cloned()
        .map(|mut item| {
            item.health = HealthState::Unknown;
            item
        })
        .collect()
}

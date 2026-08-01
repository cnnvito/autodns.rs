use super::*;
use crate::config::CoreUpstreamConfig;
use hickory_proto::op::{
    Message as DnsMessage, MessageType as DnsMessageType, OpCode as DnsOpCode, Query as DnsQuery,
};
use hickory_proto::rr::{DNSClass, Name as DnsName, RecordType as DnsRecordType};
use std::io;
use std::io::ErrorKind;
use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use tokio::net::UdpSocket;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use url::Url;

fn test_upstream_client() -> UpstreamClient {
    UpstreamClient {
        name: "upstream".to_string(),
        endpoint: endpoint_from_url(&Url::parse("udp://192.0.2.53:53").unwrap(), "")
            .expect("endpoint"),
        proxy: None,
        bootstrap: None,
        http: None,
        state: Arc::new(AtomicU8::new(UpstreamPoolState::Cold as u8)),
        transport_failure_streak: Arc::new(AtomicU32::new(0)),
        connect_gate: Arc::new(AsyncMutex::new(())),
        resolve_gate: Arc::new(AsyncMutex::new(())),
        failure_threshold: 2,
        endpoint_addrs: Arc::new(Mutex::new(None)),
        udp: Arc::new(Mutex::new(None)),
        socks5_udp: Arc::new(Mutex::new(None)),
        tcp: Arc::new(Mutex::new(None)),
        dot: Arc::new(Mutex::new(None)),
        doq: Arc::new(Mutex::new(None)),
    }
}

fn assert_duration_between(value: Duration, min: Duration, max: Duration) {
    assert!(
        value >= min && value <= max,
        "expected {value:?} between {min:?} and {max:?}",
    );
}

#[test]
fn listener_bind_error_explains_port_conflict() {
    let err = listener_bind_error("UDP", "127.0.0.1:53", io::Error::from(ErrorKind::AddrInUse))
        .to_string();

    assert!(err.contains("already in use"));
    assert!(err.contains("127.0.0.1:53"));
}

#[test]
fn listener_bind_error_explains_privileged_port_permission() {
    let err = listener_bind_error(
        "UDP",
        "127.0.0.1:53",
        io::Error::from(ErrorKind::PermissionDenied),
    )
    .to_string();

    assert!(err.contains("permission denied"));
}

#[test]
fn socks5_udp_packet_wraps_domain_target() {
    let payload = [0x12, 0x34, 0x01, 0x00];

    let packet = socks5_udp_packet("dns.example:53", &payload).expect("build packet");

    assert_eq!(&packet[..5], &[0x00, 0x00, 0x00, 0x03, 11]);
    assert_eq!(&packet[5..16], b"dns.example");
    assert_eq!(&packet[16..18], &53u16.to_be_bytes());
    assert_eq!(&packet[18..], &payload);
}

#[test]
fn parse_socks5_udp_payload_extracts_dns_message() {
    let payload = [0xab, 0xcd, 0x80, 0x00];
    let mut packet = vec![0x00, 0x00, 0x00, 0x01, 1, 1, 1, 1];
    packet.extend_from_slice(&53u16.to_be_bytes());
    packet.extend_from_slice(&payload);

    let parsed = parse_socks5_udp_payload(&packet).expect("parse payload");

    assert_eq!(parsed, payload);
}

#[test]
fn parse_socks5_udp_payload_rejects_fragmented_packets() {
    let packet = [0x00, 0x00, 0x01, 0x01, 1, 1, 1, 1, 0, 53];

    let err = parse_socks5_udp_payload(&packet).expect_err("fragmented packet must fail");

    assert!(err.to_string().contains("fragmentation"));
}

#[test]
fn len_prefixed_dns_message_round_trips() {
    let payload = [0xde, 0xad, 0xbe, 0xef];

    let framed = len_prefixed_dns_message(&payload).expect("frame message");
    let parsed = parse_len_prefixed_dns_message(&framed).expect("parse message");

    assert_eq!(&framed[..2], &4u16.to_be_bytes());
    assert_eq!(parsed, payload);
}

#[test]
fn parse_len_prefixed_dns_message_rejects_truncated_payload() {
    let raw = [0x00, 0x04, 0xaa, 0xbb];

    let err = parse_len_prefixed_dns_message(&raw).expect_err("truncated payload must fail");

    assert!(err.to_string().contains("truncated"));
}

#[test]
fn dns_message_id_reads_and_rewrites_wire_id() {
    let mut query = build_query("id.example", TYPE_A);

    let original_id = dns_message_id(&query).expect("query id");
    set_id(&mut query, 0xbeef);

    assert_eq!(original_id, 0x1234);
    assert_eq!(dns_message_id(&query), Some(0xbeef));
}

#[test]
fn parse_question_reads_header_and_question_without_full_message_decode() {
    let mut msg = DnsMessage::new(0xbeef, DnsMessageType::Query, DnsOpCode::Status);
    msg.metadata.recursion_desired = true;
    msg.metadata.checking_disabled = true;
    let name = DnsName::from_ascii("Case.Example").expect("name");
    let mut query = DnsQuery::query(name, DnsRecordType::from(TYPE_TXT));
    query.set_query_class(DNSClass::CH);
    msg.add_query(query);
    let raw = msg.to_vec().expect("encode query");

    let parsed = parse_question(&raw).expect("parse question");

    assert_eq!(parsed.id, 0xbeef);
    assert_eq!(parsed.normalized_qname, "case.example");
    assert_eq!(parsed.qtype, TYPE_TXT);
    assert_eq!(parsed.qclass, 3);
    assert_eq!(parsed.op_code, DnsOpCode::Status);
    assert!(parsed.recursion_desired);
    assert!(parsed.checking_disabled);
    assert_eq!(parsed.query.query_class(), DNSClass::CH);
}

#[test]
fn parse_question_rejects_multiple_questions() {
    let mut raw = build_query("multi.example", TYPE_A);
    raw[4..6].copy_from_slice(&2u16.to_be_bytes());

    let err = match parse_question(&raw) {
        Ok(_) => panic!("multiple questions must fail"),
        Err(err) => err,
    };

    assert!(err.to_string().contains("single-question"));
}

#[test]
fn parse_question_rejects_truncated_question() {
    let mut raw = build_query("truncated.example", TYPE_A);
    raw.truncate(raw.len() - 1);

    let err = match parse_question(&raw) {
        Ok(_) => panic!("truncated question must fail"),
        Err(err) => err,
    };

    assert!(err.to_string().contains("truncated"));
}

#[test]
fn parse_question_name_reads_compressed_labels() {
    let raw = [
        0x00, 0x00, 0x00, 0x00, 0x03, b'w', b'w', b'w', 0xc0, 0x0b, 0x00, 0x07, b'e', b'x', b'a',
        b'm', b'p', b'l', b'e', 0x03, b'c', b'o', b'm', 0x00,
    ];

    let (name, next_offset) = parse_question_name(&raw, 4).expect("parse compressed name");

    assert_eq!(name, "www.example.com");
    assert_eq!(next_offset, 10);
}

#[test]
fn strip_aaaa_records_preserves_cname_and_a_records() {
    let query = build_query("www.example.com", TYPE_A);
    let mut resp = query[..12].to_vec();
    resp[2] = 0x81;
    resp[3] = 0x80;
    resp[6..8].copy_from_slice(&3u16.to_be_bytes());
    resp[10..12].copy_from_slice(&1u16.to_be_bytes());
    resp.extend_from_slice(&query[12..]);
    write_cname_rr(&mut resp);
    write_a_rr(&mut resp);
    write_aaaa_rr(&mut resp);
    write_aaaa_rr(&mut resp);

    let filtered = strip_aaaa_records(&resp).expect("filter response");

    assert_eq!(answer_count(&filtered), Some(2));
    assert_eq!(u16::from_be_bytes([filtered[10], filtered[11]]), 0);
    assert!(filtered.len() < resp.len());
    assert!(filtered.windows(4).any(|window| window == [192, 0, 2, 10]));
}

#[test]
fn route_trie_prefers_exact_then_longest_suffix() {
    let upstreams = ["a", "b", "c", "d"]
        .into_iter()
        .map(ToString::to_string)
        .collect();
    let routes = compile_routes(
        &[
            "suffix:example.com=a".to_string(),
            "suffix:api.example.com=b".to_string(),
            "exact:www.example.com=c".to_string(),
            "wildcard:*.example.net=d".to_string(),
        ],
        &upstreams,
    )
    .expect("compile routes");

    assert_route(&routes, "www.example.com", &[], 3, &["c"]);
    assert_route(&routes, "v1.api.example.com", &[], 2, &["b"]);
    assert_route(&routes, "example.com", &[], 1, &["a"]);
    assert_route(&routes, "sub.example.net", &[], 4, &["d"]);
    assert_route(&routes, "example.net", &["a".to_string()], 0, &["a"]);
}

#[test]
fn idna_domains_match_hosts_and_routes() {
    let hosts = compile_hosts(&["bücher.test=192.0.2.20".to_string()]).expect("compile hosts");
    assert!(hosts.entries.contains_key("xn--bcher-kva.test"));

    let upstreams = ["cn"].into_iter().map(ToString::to_string).collect();
    let routes =
        compile_routes(&["suffix:bücher.test=cn".to_string()], &upstreams).expect("compile routes");
    assert_route(&routes, "www.xn--bcher-kva.test", &[], 1, &["cn"]);
    assert_route(
        &routes,
        &normalize_domain_lossy("www.bücher.test"),
        &[],
        1,
        &["cn"],
    );
}

#[test]
fn dns_cache_respects_max_entries() {
    let mut cfg = CoreConfig::default();
    cfg.cache.enabled = true;
    cfg.cache.max_entries = 2;
    cfg.cache.max_entry_size = 512;
    cfg.cache.min_ttl = 1;
    cfg.cache.max_ttl = 60;
    let cache = DnsCache::new(&cfg);
    let key_a = cache_key("a.example");
    let key_b = cache_key("b.example");
    let key_c = cache_key("c.example");
    let key_d = cache_key("d.example");
    let resp = success_response_with_answer(1);

    cache.set(key_a.clone(), &resp, false, "", "");
    cache.set(key_b.clone(), &resp, false, "", "");
    assert!(cache.get(&key_a, 2).is_some());
    cache.set(key_c.clone(), &resp, false, "", "");
    cache.set(key_d.clone(), &resp, false, "", "");

    cache.inner.run_pending_tasks();
    assert!(cache.inner.entry_count() <= 2);
}

#[test]
fn dns_cache_rewrites_ttl_from_remaining_lifetime() {
    let mut cfg = CoreConfig::default();
    cfg.cache.enabled = true;
    cfg.cache.max_entries = 2;
    cfg.cache.max_entry_size = 512;
    cfg.cache.min_ttl = 0;
    cfg.cache.max_ttl = 120;
    let cache = DnsCache::new(&cfg);
    let key = cache_key("ttl.example");
    let mut resp = build_query("ttl.example", TYPE_A);
    resp[2] = 0x81;
    resp[3] = 0x80;
    resp[6..8].copy_from_slice(&2u16.to_be_bytes());
    write_a_rr_with_ttl(&mut resp, 30);
    write_a_rr_with_ttl(&mut resp, 90);

    cache.set(key.clone(), &resp, false, "", "");
    let cached = cache.get(&key, 9).expect("cache hit");
    let ttls = rr_ttls(&cached.response).expect("parse cached ttl");

    assert_eq!(
        u16::from_be_bytes([cached.response[0], cached.response[1]]),
        9
    );
    assert_eq!(ttls.len(), 2);
    assert!(ttls[0] <= 30);
    assert!(ttls[1] <= 30);
}

#[test]
fn answer_min_ttl_ignores_additional_records() {
    let mut resp = build_query("ttl.example", TYPE_A);
    resp[2] = 0x81;
    resp[3] = 0x80;
    resp[6..8].copy_from_slice(&1u16.to_be_bytes());
    resp[10..12].copy_from_slice(&1u16.to_be_bytes());
    write_a_rr_with_ttl(&mut resp, 45);
    write_a_rr_with_ttl(&mut resp, 5);

    assert_eq!(answer_min_ttl(&resp).expect("parse answer ttl"), Some(45));
}

#[test]
fn probe_response_classification_rejects_unparseable_responses() {
    // Header too short to carry an RCODE: must not count as a healthy probe.
    assert_eq!(classify_probe_response(&[]), ProbeOutcome::InvalidResponse);
    assert_eq!(
        classify_probe_response(&[0x12, 0x34, 0x80]),
        ProbeOutcome::InvalidResponse
    );
    // SERVFAIL / REFUSED are retryable failures.
    assert_eq!(
        classify_probe_response(&[0x12, 0x34, 0x80, RCODE_SERVER_FAILURE]),
        ProbeOutcome::RetryableResponse
    );
    assert_eq!(
        classify_probe_response(&[0x12, 0x34, 0x80, RCODE_REFUSED]),
        ProbeOutcome::RetryableResponse
    );
    // NOERROR and NXDOMAIN both prove the upstream is reachable and answering.
    assert_eq!(
        classify_probe_response(&[0x12, 0x34, 0x80, RCODE_SUCCESS]),
        ProbeOutcome::Healthy
    );
    assert_eq!(
        classify_probe_response(&[0x12, 0x34, 0x80, RCODE_NAME_ERROR]),
        ProbeOutcome::Healthy
    );
}

#[test]
fn health_restore_returns_upstream_to_query_pool_immediately() {
    let health = HealthMonitor::new(true, 2, 3, vec!["upstream".to_string()]);
    let names = vec!["upstream".to_string()];

    // Degraded but still displayed healthy: one failure ejects from the pool.
    health.record_failure("upstream", FailureKind::Transport, "failure");
    assert_eq!(health.query_eligibility(&names), vec![false]);
    health.restore("upstream");
    assert_eq!(health.query_eligibility(&names), vec![true]);
    assert!(health.snapshot().healthy("upstream"));

    // Fully unhealthy: enough failures to flip the healthy flag. A restore must clear
    // both flags at once instead of waiting for recovery_threshold successes.
    health.record_failure("upstream", FailureKind::Transport, "failure");
    health.record_failure("upstream", FailureKind::Transport, "failure");
    assert!(!health.snapshot().healthy("upstream"));
    health.restore("upstream");
    assert_eq!(health.query_eligibility(&names), vec![true]);
    assert!(health.snapshot().healthy("upstream"));
}

#[test]
fn health_recent_query_success_ignores_probe_success() {
    let health = HealthMonitor::new(true, 3, 2, vec!["upstream".to_string()]);

    health.record_probe_success("upstream", Duration::from_millis(12));
    assert!(!health.recent_query_success("upstream", Duration::from_secs(10)));

    health.record_query_success("upstream", Duration::from_millis(10));
    assert!(health.recent_query_success("upstream", Duration::from_secs(10)));
}

#[test]
fn health_recent_query_success_does_not_pause_recovery() {
    let health = HealthMonitor::new(true, 1, 2, vec!["upstream".to_string()]);

    health.record_failure("upstream", FailureKind::Transport, "failure");
    health.record_query_success("upstream", Duration::from_millis(10));

    assert!(health.recent_query_success("upstream", Duration::from_secs(10)));
    assert!(!health.recent_healthy_query_success("upstream", Duration::from_secs(10)));
}

#[test]
fn health_degraded_upstream_ignores_recent_query_success_for_probe_skip() {
    let health = HealthMonitor::new(true, 3, 2, vec!["upstream".to_string()]);

    health.record_query_success("upstream", Duration::from_millis(10));
    assert!(health.recent_healthy_query_success("upstream", Duration::from_secs(10)));

    health.record_failure("upstream", FailureKind::Transport, "failure");

    assert!(health.recent_query_success("upstream", Duration::from_secs(10)));
    assert!(!health.recent_healthy_query_success("upstream", Duration::from_secs(10)));
}

#[test]
fn health_probe_delay_backs_off_after_unhealthy() {
    let health = HealthMonitor::new(true, 2, 1, vec!["upstream".to_string()]);
    let interval = Duration::from_secs(30);

    health.record_probe_failure("upstream", "first failure");
    assert_duration_between(
        health.probe_delay("upstream", interval),
        Duration::from_millis(800),
        Duration::from_millis(1200),
    );

    health.record_probe_failure("upstream", "second failure");
    assert_duration_between(
        health.probe_delay("upstream", interval),
        Duration::from_millis(1600),
        Duration::from_millis(2400),
    );

    health.record_probe_failure("upstream", "third failure");
    assert_duration_between(
        health.probe_delay("upstream", interval),
        Duration::from_millis(3200),
        Duration::from_millis(4800),
    );

    health.record_probe_success("upstream", Duration::from_millis(8));
    assert_eq!(health.probe_delay("upstream", interval), interval);
}

#[test]
fn health_probe_delay_caps_failure_backoff() {
    let health = HealthMonitor::new(true, 1, 1, vec!["upstream".to_string()]);
    let interval = Duration::from_secs(60);

    for index in 0..10 {
        health.record_probe_failure("upstream", format!("failure {index}"));
    }

    assert_duration_between(
        health.probe_delay("upstream", interval),
        Duration::from_secs(24),
        HEALTHCHECK_FAILURE_RETRY_MAX_DELAY,
    );
}

#[test]
fn health_recovery_probe_uses_short_confirm_delay() {
    let health = HealthMonitor::new(true, 1, 2, vec!["upstream".to_string()]);
    let interval = Duration::from_secs(30);

    health.record_failure("upstream", FailureKind::Transport, "failure");
    assert_duration_between(
        health.probe_delay("upstream", interval),
        Duration::from_millis(800),
        Duration::from_millis(1200),
    );

    health.record_probe_success("upstream", Duration::from_millis(8));
    assert_eq!(
        health.probe_delay("upstream", interval),
        HEALTHCHECK_RECOVERY_CONFIRM_DELAY
    );

    health.record_probe_success("upstream", Duration::from_millis(9));
    assert_eq!(health.probe_delay("upstream", interval), interval);
}

#[test]
fn health_negative_response_does_not_degrade_upstream() {
    let health = HealthMonitor::new(true, 1, 1, vec!["upstream".to_string()]);
    let names = vec!["upstream".to_string()];

    health.record_failure("upstream", FailureKind::Transport, "first failure");
    assert_eq!(health.query_eligibility(&names), vec![false]);

    health.record_negative_response("upstream", Duration::from_millis(9));

    let snapshot = health.snapshot();
    assert!(snapshot.healthy("upstream"));
    assert_eq!(health.query_eligibility(&names), vec![true]);
}

#[test]
fn health_transport_failure_degrades_and_success_recovers_upstream() {
    let health = HealthMonitor::new(true, 2, 1, vec!["upstream".to_string()]);
    let names = vec!["upstream".to_string()];

    health.record_failure("upstream", FailureKind::Transport, "first failure");
    assert!(health.snapshot().healthy("upstream"));
    assert_eq!(health.query_eligibility(&names), vec![false]);

    health.record_failure("upstream", FailureKind::Transport, "second failure");
    assert!(!health.snapshot().healthy("upstream"));
    assert_eq!(health.query_eligibility(&names), vec![false]);

    health.record_probe_success("upstream", Duration::from_millis(8));
    let snapshot = health.snapshot();
    assert!(snapshot.healthy("upstream"));
    assert_eq!(health.query_eligibility(&names), vec![true]);
}

#[test]
fn health_requires_configured_recovery_successes() {
    let health = HealthMonitor::new(true, 1, 2, vec!["upstream".to_string()]);
    let names = vec!["upstream".to_string()];

    health.record_failure("upstream", FailureKind::Transport, "failure");
    assert!(!health.snapshot().healthy("upstream"));
    assert_eq!(health.query_eligibility(&names), vec![false]);

    health.record_probe_success("upstream", Duration::from_millis(8));
    assert!(!health.snapshot().healthy("upstream"));
    assert_eq!(health.query_eligibility(&names), vec![false]);

    health.record_probe_success("upstream", Duration::from_millis(9));
    assert!(health.snapshot().healthy("upstream"));
    assert_eq!(health.query_eligibility(&names), vec![true]);
}

#[test]
fn health_ejects_from_query_pool_before_displaying_unhealthy() {
    let health = HealthMonitor::new(true, 3, 2, vec!["upstream".to_string()]);
    let names = vec!["upstream".to_string()];

    health.record_failure("upstream", FailureKind::Transport, "first failure");

    assert!(health.snapshot().healthy("upstream"));
    assert_eq!(health.query_eligibility(&names), vec![false]);

    health.record_probe_success("upstream", Duration::from_millis(8));
    assert_eq!(health.query_eligibility(&names), vec![false]);

    health.record_probe_success("upstream", Duration::from_millis(9));
    assert!(health.snapshot().healthy("upstream"));
    assert_eq!(health.query_eligibility(&names), vec![true]);
}

#[test]
fn health_recovery_probe_does_not_wait_full_interval() {
    let health = HealthMonitor::new(true, 1, 2, vec!["upstream".to_string()]);
    let interval = Duration::from_secs(30);

    health.record_failure("upstream", FailureKind::Transport, "failure");
    assert_duration_between(
        health.probe_delay("upstream", interval),
        Duration::from_millis(800),
        Duration::from_millis(1200),
    );

    health.record_probe_success("upstream", Duration::from_millis(8));
    assert_eq!(
        health.probe_delay("upstream", interval),
        HEALTHCHECK_RECOVERY_CONFIRM_DELAY
    );

    health.record_probe_success("upstream", Duration::from_millis(9));
    assert_eq!(health.probe_delay("upstream", interval), interval);
}

#[test]
fn health_retryable_response_removes_upstream_from_query_pool() {
    let health = HealthMonitor::new(true, 1, 1, vec!["upstream".to_string()]);
    let names = vec!["upstream".to_string()];

    health.record_failure(
        "upstream",
        FailureKind::RetryableResponse,
        "retryable response",
    );

    let snapshot = health.snapshot();
    assert!(!snapshot.healthy("upstream"));
    assert_eq!(health.query_eligibility(&names), vec![false]);
}

#[test]
fn health_disabled_keeps_all_upstreams_query_eligible() {
    let health = HealthMonitor::new(false, 1, 1, vec!["upstream".to_string()]);
    let names = vec!["upstream".to_string(), "missing".to_string()];

    health.record_failure("upstream", FailureKind::Transport, "failure");

    assert_eq!(health.query_eligibility(&names), vec![true, true]);
}

#[test]
fn upstream_pool_transport_failure_degrades_after_threshold() {
    let client = test_upstream_client();

    assert!(!client.should_skip_for_query());
    client.mark_transport_failure();
    assert_eq!(client.pool_state(), UpstreamPoolState::Cold);
    assert!(!client.should_skip_for_query());

    client.mark_transport_failure();
    assert_eq!(client.pool_state(), UpstreamPoolState::Degraded);
    assert!(!client.should_skip_for_query());

    client.mark_ready();
    assert_eq!(client.pool_state(), UpstreamPoolState::Ready);
    assert!(!client.should_skip_for_query());
}

#[test]
fn upstream_pool_only_connecting_skips_without_health_state() {
    let client = test_upstream_client();
    client.store_pool_state(UpstreamPoolState::Degraded);
    assert!(!client.should_skip_for_query());

    client.store_pool_state(UpstreamPoolState::Recovering);
    assert!(!client.should_skip_for_query());

    client.store_pool_state(UpstreamPoolState::Connecting);
    assert!(client.should_skip_for_query());
}

#[tokio::test]
async fn upstream_pool_transport_failure_drops_cached_stream_client() {
    let client = test_upstream_client();
    let (stream, _peer) = tokio::io::duplex(64);
    *client.tcp.lock() = Some(LenPrefixedUpstreamClient::new(Box::new(stream)));

    assert!(client.tcp.lock().is_some());

    client.mark_transport_failure();

    assert!(client.tcp.lock().is_none());
}

#[tokio::test]
async fn upstream_timeout_drops_cached_stream_client() {
    let mut client = test_upstream_client();
    client.endpoint =
        endpoint_from_url(&Url::parse("tcp://192.0.2.53:53").unwrap(), "").expect("tcp endpoint");
    let (stream, _peer) = tokio::io::duplex(64);
    *client.tcp.lock() = Some(LenPrefixedUpstreamClient::new(Box::new(stream)));
    *client.endpoint_addrs.lock() = Some(vec!["192.0.2.53:53".parse().unwrap()].into());

    let err = client
        .exchange(
            &build_query("timeout.example", TYPE_A),
            Some(Duration::from_millis(10)),
        )
        .await
        .expect_err("silent upstream should time out");

    assert!(err.to_string().contains("upstream timeout"));
    assert!(client.tcp.lock().is_none());
    assert!(client.endpoint_addrs.lock().is_none());
    assert_eq!(client.transport_failure_streak.load(Ordering::Acquire), 1);
}

#[test]
fn build_resolver_defaults_empty_timeout() {
    let mut cfg = crate::config::default_local_config();
    cfg.resolver.timeout.clear();

    let resolver =
        build_resolver(&cfg, LogBuffer::new(1), DnsHistoryRecorder::disabled()).expect("resolver");

    assert_eq!(resolver.timeout, Some(DEFAULT_RESOLVER_TIMEOUT));
}

#[tokio::test]
async fn resolver_does_not_query_unhealthy_upstream() {
    let mut cfg = crate::config::default_local_config();
    cfg.resolver.timeout = "500ms".into();
    cfg.healthcheck.failure_threshold = 1;
    cfg.resolver.upstreams = vec![CoreUpstreamConfig {
        name: "unhealthy".into(),
        endpoint: "udp://192.0.2.53:53".into(),
        ..Default::default()
    }];
    let resolver =
        build_resolver(&cfg, LogBuffer::new(1), DnsHistoryRecorder::disabled()).expect("resolver");
    resolver
        .health
        .record_failure("unhealthy", FailureKind::Transport, "healthcheck failed");
    let client = resolver.clients.get("unhealthy").expect("upstream");

    let started = Instant::now();
    let resp = resolver
        .resolve(build_query("unhealthy.example", TYPE_A))
        .await
        .expect("servfail response");

    assert_eq!(rcode(&resp), Some(RCODE_SERVER_FAILURE));
    assert!(started.elapsed() < Duration::from_millis(100));
    assert_eq!(client.transport_failure_streak.load(Ordering::Acquire), 0);
}

#[tokio::test]
#[ignore = "binds local UDP sockets for end-to-end resolver verification"]
async fn resolver_skips_unhealthy_first_upstream_and_queries_second_local_dns() {
    let (bad_endpoint, bad_queries, bad_task) =
        spawn_counting_udp_answer_upstream([192, 0, 2, 10]).await;
    let (good_endpoint, good_queries, good_task) =
        spawn_counting_udp_answer_upstream([192, 0, 2, 55]).await;
    let mut cfg = crate::config::default_local_config();
    cfg.resolver.timeout = "500ms".into();
    cfg.healthcheck.failure_threshold = 1;
    cfg.resolver.upstreams = vec![
        CoreUpstreamConfig {
            name: "bad".into(),
            endpoint: bad_endpoint,
            ..Default::default()
        },
        CoreUpstreamConfig {
            name: "good".into(),
            endpoint: good_endpoint,
            ..Default::default()
        },
    ];
    let resolver =
        build_resolver(&cfg, LogBuffer::new(1), DnsHistoryRecorder::disabled()).expect("resolver");
    resolver
        .health
        .record_failure("bad", FailureKind::Transport, "healthcheck failed");

    let resp = resolver
        .resolve(build_query("local-e2e.example", TYPE_A))
        .await
        .expect("response");

    assert_eq!(rcode(&resp), Some(RCODE_SUCCESS));
    assert_eq!(answer_count(&resp), Some(1));
    assert!(resp.windows(4).any(|window| window == [192, 0, 2, 55]));
    assert_eq!(bad_queries.load(Ordering::Acquire), 0);
    assert_eq!(good_queries.load(Ordering::Acquire), 1);
    good_task.await.expect("good upstream task");
    bad_task.abort();
}

#[tokio::test]
#[ignore = "binds local UDP sockets and runs a long healthcheck failover scenario"]
async fn resolver_moves_traffic_away_and_back_after_upstream_restart() {
    const QUERY_COUNT: usize = 1000;

    let first_queries = Arc::new(AtomicU32::new(0));
    let second_queries = Arc::new(AtomicU32::new(0));
    let (first_endpoint, first_addr, mut first_task) =
        spawn_looping_udp_answer_upstream("127.0.0.1:0", [192, 0, 2, 10], first_queries.clone())
            .await;
    let (second_endpoint, _second_addr, second_task) =
        spawn_looping_udp_answer_upstream("127.0.0.1:0", [192, 0, 2, 55], second_queries.clone())
            .await;

    let mut cfg = crate::config::default_local_config();
    cfg.cache.enabled = false;
    cfg.resolver.timeout = "100ms".into();
    cfg.healthcheck.interval = "20ms".into();
    cfg.healthcheck.timeout = "20ms".into();
    cfg.healthcheck.domain = "healthcheck.local".into();
    cfg.healthcheck.failure_threshold = 1;
    cfg.healthcheck.recovery_threshold = 1;
    cfg.resolver.upstreams = vec![
        CoreUpstreamConfig {
            name: "first".into(),
            endpoint: first_endpoint,
            ..Default::default()
        },
        CoreUpstreamConfig {
            name: "second".into(),
            endpoint: second_endpoint,
            ..Default::default()
        },
    ];
    let resolver =
        build_resolver(&cfg, LogBuffer::new(1), DnsHistoryRecorder::disabled()).expect("resolver");
    let (stop_tx, _) = watch::channel(false);
    let health_tasks = spawn_health_tasks(&cfg, &resolver, &stop_tx);
    let upstream_names = vec!["first".to_string(), "second".to_string()];

    resolve_many(&resolver, "before-restart", QUERY_COUNT).await;
    assert_eq!(first_queries.load(Ordering::Acquire), QUERY_COUNT as u32);
    assert_eq!(second_queries.load(Ordering::Acquire), 0);

    first_task.abort();
    let _ = (&mut first_task).await;
    wait_for_query_eligibility(&resolver.health, &upstream_names, &[false, true]).await;

    resolve_many(&resolver, "while-first-down", QUERY_COUNT).await;
    assert_eq!(first_queries.load(Ordering::Acquire), QUERY_COUNT as u32);
    assert_eq!(second_queries.load(Ordering::Acquire), QUERY_COUNT as u32);

    let (_first_endpoint, _first_addr, restarted_first_task) =
        spawn_looping_udp_answer_upstream(first_addr, [192, 0, 2, 10], first_queries.clone()).await;
    first_task = restarted_first_task;
    wait_for_query_eligibility(&resolver.health, &upstream_names, &[true, true]).await;

    resolve_many(&resolver, "after-first-restart", QUERY_COUNT).await;
    assert_eq!(
        first_queries.load(Ordering::Acquire),
        (QUERY_COUNT * 2) as u32
    );
    assert_eq!(second_queries.load(Ordering::Acquire), QUERY_COUNT as u32);

    let _ = stop_tx.send(true);
    for task in health_tasks {
        let _ = task.await;
    }
    first_task.abort();
    second_task.abort();
}

#[test]
fn upstream_pool_singleflight_marks_concurrent_connecting() {
    let client = test_upstream_client();

    let guard = client.begin_connect().expect("begin connect");
    assert_eq!(client.pool_state(), UpstreamPoolState::Connecting);
    assert!(client.should_skip_for_query());

    let err = client.begin_connect().expect_err("second connect is gated");
    assert!(is_upstream_connecting_error(&err));

    drop(guard);
    client.mark_ready();
    assert!(!client.should_skip_for_query());
}

#[test]
fn parse_lookup_records_extracts_a_answer() {
    let query = build_query("answer.example", TYPE_A);
    let question = parse_question(&query).expect("parse question");
    let response = hosts_response(&question, &[[192, 0, 2, 44]], &[]);
    let records = parse_lookup_records(&response).expect("parse records");

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].name, "answer.example");
    assert_eq!(records[0].record_type, "A");
    assert_eq!(records[0].ttl, 60);
    assert_eq!(records[0].value, "192.0.2.44");
}

#[test]
fn parse_lookup_records_extracts_compressed_cname() {
    let query = build_query("alias.example", TYPE_CNAME);
    let mut response = query.clone();
    response[2] = 0x81;
    response[3] = 0x80;
    response[6..8].copy_from_slice(&1u16.to_be_bytes());
    write_rr_header(&mut response, TYPE_CNAME, CLASS_IN, 300, 9);
    response.extend_from_slice(&[6]);
    response.extend_from_slice(b"target");
    response.extend_from_slice(&[0xc0, 0x12]);

    let records = parse_lookup_records(&response).expect("parse records");

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].name, "alias.example");
    assert_eq!(records[0].record_type, "CNAME");
    assert_eq!(records[0].ttl, 300);
    assert_eq!(records[0].value, "target.example");
}

fn cache_key(qname: &str) -> CacheKey {
    CacheKey {
        qname: qname.to_string(),
        qtype: TYPE_A,
        qclass: CLASS_IN,
        route_id: 0,
    }
}

fn assert_route(
    routes: &Routes,
    domain: &str,
    defaults: &[String],
    expected_id: i32,
    expected_upstreams: &[&str],
) {
    let (route_id, upstreams) = routes.select(domain, defaults);
    let expected = expected_upstreams
        .iter()
        .copied()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    assert_eq!(route_id, expected_id);
    assert_eq!(upstreams, expected.as_slice());
}

fn success_response_with_answer(id: u16) -> Vec<u8> {
    let mut resp = build_query("cache.example", TYPE_A);
    resp[0..2].copy_from_slice(&id.to_be_bytes());
    resp[2] = 0x81;
    resp[3] = 0x80;
    resp[6..8].copy_from_slice(&1u16.to_be_bytes());
    write_a_rr(&mut resp);
    resp
}

fn write_cname_rr(resp: &mut Vec<u8>) {
    resp.extend_from_slice(&[0xc0, 0x0c]);
    resp.extend_from_slice(&5u16.to_be_bytes());
    resp.extend_from_slice(&CLASS_IN.to_be_bytes());
    resp.extend_from_slice(&60u32.to_be_bytes());
    let cname = dns_name("target.example.com");
    resp.extend_from_slice(&(cname.len() as u16).to_be_bytes());
    resp.extend_from_slice(&cname);
}

fn write_a_rr(resp: &mut Vec<u8>) {
    write_a_rr_with_ttl(resp, 60);
}

fn write_a_rr_with_ttl(resp: &mut Vec<u8>, ttl: u32) {
    write_rr_header(resp, TYPE_A, CLASS_IN, ttl, 4);
    resp.extend_from_slice(&[192, 0, 2, 10]);
}

fn write_aaaa_rr(resp: &mut Vec<u8>) {
    write_rr_header(resp, TYPE_AAAA, CLASS_IN, 60, 16);
    resp.extend_from_slice(&[0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
}

fn dns_name(domain: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for label in domain.split('.') {
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
    out
}

async fn spawn_counting_udp_answer_upstream(
    ip: [u8; 4],
) -> (String, Arc<AtomicU32>, JoinHandle<()>) {
    let socket = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind test upstream");
    let addr = socket.local_addr().expect("test upstream address");
    let queries = Arc::new(AtomicU32::new(0));
    let task_queries = queries.clone();
    let task = tokio::spawn(async move {
        let mut buf = [0u8; 512];
        let (len, peer) = socket.recv_from(&mut buf).await.expect("receive query");
        task_queries.fetch_add(1, Ordering::AcqRel);
        let question = parse_question(&buf[..len]).expect("parse query");
        let resp = hosts_response(&question, &[ip], &[]);
        socket.send_to(&resp, peer).await.expect("send response");
    });
    (format!("udp://{addr}"), queries, task)
}

async fn spawn_looping_udp_answer_upstream(
    bind_addr: impl tokio::net::ToSocketAddrs,
    ip: [u8; 4],
    queries: Arc<AtomicU32>,
) -> (String, SocketAddr, JoinHandle<()>) {
    let socket = UdpSocket::bind(bind_addr)
        .await
        .expect("bind test upstream");
    let addr = socket.local_addr().expect("test upstream address");
    let task = tokio::spawn(async move {
        let mut buf = [0u8; 512];
        loop {
            let Ok((len, peer)) = socket.recv_from(&mut buf).await else {
                break;
            };
            let question = parse_question(&buf[..len]).expect("parse query");
            if question.normalized_qname != "healthcheck.local" {
                queries.fetch_add(1, Ordering::AcqRel);
            }
            let resp = hosts_response(&question, &[ip], &[]);
            let _ = socket.send_to(&resp, peer).await;
        }
    });
    (format!("udp://{addr}"), addr, task)
}

async fn resolve_many(resolver: &Resolver, prefix: &str, count: usize) {
    for index in 0..count {
        let resp = resolver
            .resolve(build_query(&format!("{prefix}-{index}.example"), TYPE_A))
            .await
            .expect("response");
        assert_eq!(rcode(&resp), Some(RCODE_SUCCESS));
        assert_eq!(answer_count(&resp), Some(1));
    }
}

async fn wait_for_query_eligibility(
    health: &Arc<HealthMonitor>,
    names: &[String],
    expected: &[bool],
) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let current = health.query_eligibility(names);
        if current == expected {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for eligibility {expected:?}, got {current:?}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

use crate::config::{
    parse_bootstrap_dns_servers, parse_go_duration, parse_upstream_endpoint, CoreConfig,
    CoreUpstreamConfig,
};
use crate::desktop::{DnsLookupRecord, DnsLookupResult, HealthState, ProxyHealth, UpstreamHealth};
use crate::history::{DnsHistoryEvent, DnsHistoryRecorder};
use crate::logging::LogBuffer;
use anyhow::{anyhow, Context, Result};
use arc_swap::ArcSwap;
use base64::Engine;
use chrono::Utc;
use hickory_proto::op::{
    Message as DnsMessage, MessageType as DnsMessageType, OpCode as DnsOpCode, Query as DnsQuery,
    ResponseCode as DnsResponseCode,
};
use hickory_proto::rr::rdata::{A as DnsA, AAAA as DnsAaaa};
use hickory_proto::rr::{
    DNSClass, Name as DnsName, RData as DnsRData, Record as DnsRecord, RecordType as DnsRecordType,
};
use moka::sync::Cache;
use parking_lot::Mutex;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use std::collections::{HashMap, HashSet};
use std::error::Error as StdError;
use std::fs::File;
use std::io::BufReader;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::io::{split, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::{oneshot, watch, Mutex as AsyncMutex, OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinHandle;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{
    ClientConfig as RustlsClientConfig, RootCertStore, ServerConfig as RustlsServerConfig,
};
use tokio_rustls::{TlsAcceptor, TlsConnector};
use url::Url;

const TYPE_A: u16 = 1;
const TYPE_NS: u16 = 2;
const TYPE_CNAME: u16 = 5;
const TYPE_SOA: u16 = 6;
const TYPE_MX: u16 = 15;
const TYPE_TXT: u16 = 16;
const TYPE_AAAA: u16 = 28;
const TYPE_HTTPS: u16 = 65;
#[cfg(test)]
const CLASS_IN: u16 = 1;
const RCODE_SUCCESS: u8 = 0;
const RCODE_NAME_ERROR: u8 = 3;
const RCODE_SERVER_FAILURE: u8 = 2;
const RCODE_REFUSED: u8 = 5;
const DNS_WIRE_LIMIT: usize = 65535;
const MAX_CONCURRENT_REQUESTS: usize = 256;
const MAX_CONCURRENT_HEALTHCHECKS: usize = 4;
const HEALTHCHECK_STAGGER_STEP: Duration = Duration::from_millis(200);
const HEALTHCHECK_MAX_BACKOFF: Duration = Duration::from_secs(300);
const BOOTSTRAP_LOOKUP_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct RuntimeView {
    pub config: CoreConfig,
    pub health: Arc<HealthMonitor>,
}

pub struct RunningRuntime {
    state: Arc<RuntimeState>,
    stop_tx: watch::Sender<bool>,
    listener: JoinHandle<()>,
    health_tasks: Vec<JoinHandle<()>>,
}

impl RunningRuntime {
    pub fn view(&self) -> RuntimeView {
        self.state.view()
    }

    pub fn can_reload(&self, cfg: &CoreConfig) -> bool {
        let view = self.view();
        same_server_identity(&view.config, cfg)
    }

    pub async fn reload(&mut self, cfg: CoreConfig, logs: LogBuffer) -> Result<()> {
        if !self.can_reload(&cfg) {
            return Err(anyhow!("listener settings changed"));
        }
        let history = self.state.resolver().history.clone();
        let resolver = build_resolver(&cfg, logs.clone(), history)?;
        let view = RuntimeView {
            config: cfg.clone(),
            health: resolver.health.clone(),
        };

        for task in self.health_tasks.drain(..) {
            task.abort();
            let _ = task.await;
        }
        self.health_tasks = spawn_health_tasks(&cfg, &resolver, &self.stop_tx);
        self.state.replace(resolver, view);
        logs.push("info", "desktop runtime resolver reloaded");
        Ok(())
    }

    pub fn clear_cache(&self) -> usize {
        self.state.resolver().clear_cache()
    }

    pub(crate) fn resolver(&self) -> Arc<Resolver> {
        self.state.resolver()
    }

    pub(crate) fn set_health_listener(&self, listener: HealthListener) {
        self.state.resolver().health.set_listener(listener);
    }

    pub async fn stop(self) {
        let _ = self.stop_tx.send(true);
        let _ = self.listener.await;
        for task in self.health_tasks {
            let _ = task.await;
        }
    }
}

struct RuntimeState {
    resolver: ArcSwap<Resolver>,
    view: Mutex<RuntimeView>,
}

impl RuntimeState {
    fn new(resolver: Resolver, view: RuntimeView) -> Arc<Self> {
        Arc::new(Self {
            resolver: ArcSwap::from_pointee(resolver),
            view: Mutex::new(view),
        })
    }

    fn resolver(&self) -> Arc<Resolver> {
        self.resolver.load_full()
    }

    fn view(&self) -> RuntimeView {
        self.view.lock().clone()
    }

    fn replace(&self, resolver: Resolver, view: RuntimeView) {
        self.resolver.store(Arc::new(resolver));
        *self.view.lock() = view;
    }
}

#[derive(Clone)]
pub(crate) struct Resolver {
    default_upstreams: Arc<[String]>,
    hosts: Hosts,
    routes: Routes,
    cache: DnsCache,
    history: DnsHistoryRecorder,
    clients: HashMap<String, UpstreamClient>,
    health: Arc<HealthMonitor>,
    timeout: Option<Duration>,
    ipv6_enabled: bool,
    logs: LogBuffer,
}

#[derive(Clone)]
struct UpstreamClient {
    name: String,
    endpoint: Endpoint,
    proxy: Option<ProxyEndpoint>,
    bootstrap: Option<Arc<BootstrapResolver>>,
    http: Option<reqwest::Client>,
    udp: Arc<Mutex<Option<Arc<UdpUpstreamClient>>>>,
    socks5_udp: Arc<Mutex<Option<Arc<Socks5UdpUpstreamClient>>>>,
    tcp: Arc<Mutex<Option<Arc<LenPrefixedUpstreamClient>>>>,
    dot: Arc<Mutex<Option<Arc<LenPrefixedUpstreamClient>>>>,
    doq: Arc<Mutex<Option<Arc<DoqUpstreamClient>>>>,
}

#[derive(Clone)]
struct Endpoint {
    scheme: String,
    host: String,
    port: u16,
    address: String,
    url: String,
    server_name: String,
}

#[derive(Clone)]
struct BootstrapResolver {
    servers: Arc<[SocketAddr]>,
}

#[derive(Clone)]
struct ProxyEndpoint {
    address: String,
    username: String,
    password: String,
}

type PendingUdpResponses = Arc<Mutex<HashMap<u16, oneshot::Sender<Vec<u8>>>>>;
type PendingStreamResponses = Arc<Mutex<HashMap<u16, oneshot::Sender<Vec<u8>>>>>;
type BoxedAsyncIo = Box<dyn AsyncIo>;
pub(crate) type HealthListener = Arc<dyn Fn() + Send + Sync>;

trait AsyncIo: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T> AsyncIo for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

struct UdpUpstreamClient {
    socket: Arc<UdpSocket>,
    pending: PendingUdpResponses,
    next_id: Mutex<u16>,
    stop_tx: watch::Sender<bool>,
}

struct Socks5UdpUpstreamClient {
    _control: TcpStream,
    socket: Arc<UdpSocket>,
    relay_address: String,
    pending: PendingUdpResponses,
    next_id: Mutex<u16>,
    stop_tx: watch::Sender<bool>,
}

struct LenPrefixedUpstreamClient {
    writer: AsyncMutex<WriteHalf<BoxedAsyncIo>>,
    pending: PendingStreamResponses,
    next_id: Mutex<u16>,
}

struct DoqUpstreamClient {
    _endpoint: quinn::Endpoint,
    connection: quinn::Connection,
}

#[derive(Clone, Default)]
pub(crate) struct Hosts {
    entries: Arc<HashMap<String, HostEntry>>,
}

#[derive(Clone)]
struct HostEntry {
    ipv4: Vec<[u8; 4]>,
    ipv6: Vec<[u8; 16]>,
}

#[derive(Clone, Default)]
pub(crate) struct Routes {
    exact: Arc<HashMap<String, RouteEntry>>,
    suffix: Arc<RouteTrie>,
    wildcard: Arc<RouteTrie>,
}

#[derive(Clone)]
struct RouteEntry {
    id: i32,
    domain: String,
    upstreams: Arc<[String]>,
}

#[derive(Clone, Default)]
struct RouteTrie {
    root: RouteTrieNode,
}

#[derive(Clone, Default)]
struct RouteTrieNode {
    route: Option<RouteEntry>,
    children: HashMap<String, RouteTrieNode>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MatchType {
    Exact,
    Suffix,
    Wildcard,
}

#[derive(Clone)]
struct DnsCache {
    enabled: bool,
    max_entry_size: usize,
    min_ttl: u32,
    max_ttl: u32,
    negative_ttl: u32,
    inner: Cache<CacheKey, CacheEntry>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct CacheKey {
    qname: String,
    qtype: u16,
    qclass: u16,
    route_id: i32,
}

#[derive(Clone)]
struct CacheEntry {
    response: Vec<u8>,
    expires_at: Instant,
    original_ttls: Vec<u32>,
    ttl_offsets: Vec<usize>,
    upstream_name: String,
    upstream_protocol: String,
}

struct CachedResponse {
    response: Vec<u8>,
    upstream_name: String,
    upstream_protocol: String,
}

#[derive(Clone)]
pub struct HealthMonitor {
    states: Arc<Mutex<HashMap<String, HealthRecord>>>,
    listener: Arc<Mutex<Option<HealthListener>>>,
    enabled: bool,
    failure_threshold: u32,
    recovery_threshold: u32,
}

#[derive(Clone)]
struct HealthRecord {
    healthy: bool,
    failure_streak: u32,
    recovery_streak: u32,
    probe_failure_streak: u32,
    failure_count: u64,
    last_error: Option<String>,
    last_success_at: Option<String>,
    last_query_success_at: Option<Instant>,
    latency_ms: Option<u128>,
}

struct HealthSnapshot {
    enabled: bool,
    states: HashMap<String, HealthRecord>,
}

#[derive(Clone, Default)]
struct UpstreamDiagnostics {
    failure_count: u64,
    last_error: Option<String>,
    last_success_at: Option<String>,
    latency_ms: Option<u128>,
}

#[derive(Clone)]
struct Question {
    id: u16,
    normalized_qname: String,
    qtype: u16,
    qclass: u16,
    query: DnsQuery,
    op_code: DnsOpCode,
    recursion_desired: bool,
    checking_disabled: bool,
}

struct NegativeResponse {
    response: Vec<u8>,
    upstream_name: String,
    upstream_protocol: String,
    error: String,
}

pub async fn start_runtime(
    cfg: CoreConfig,
    logs: LogBuffer,
    history: DnsHistoryRecorder,
) -> Result<RunningRuntime> {
    let resolver = build_resolver(&cfg, logs.clone(), history)?;
    let view = RuntimeView {
        config: cfg.clone(),
        health: resolver.health.clone(),
    };
    let state = RuntimeState::new(resolver.clone(), view);
    let (stop_tx, stop_rx) = watch::channel(false);
    let listener_state = state.clone();
    let listener_logs = logs.clone();
    let request_limit = Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS));
    let listener = match cfg.server.mode.as_str() {
        "udp" => {
            let socket = UdpSocket::bind(&cfg.server.listen)
                .await
                .with_context(|| format!("bind udp listener: {}", cfg.server.listen))?;
            tokio::spawn(run_udp_listener(
                cfg.server.listen.clone(),
                socket,
                listener_state,
                stop_rx,
                request_limit.clone(),
                listener_logs,
            ))
        }
        "tcp" => {
            let listener = TcpListener::bind(&cfg.server.listen)
                .await
                .with_context(|| format!("bind tcp listener: {}", cfg.server.listen))?;
            tokio::spawn(run_tcp_listener(
                cfg.server.listen.clone(),
                listener,
                listener_state,
                stop_rx,
                request_limit.clone(),
                listener_logs,
            ))
        }
        "dot" => {
            let tls = load_server_tls(&cfg.server.cert_file, &cfg.server.key_file)?;
            let listener = TcpListener::bind(&cfg.server.listen)
                .await
                .with_context(|| format!("bind dot listener: {}", cfg.server.listen))?;
            tokio::spawn(run_dot_listener(
                cfg.server.listen.clone(),
                listener,
                tls,
                listener_state,
                stop_rx,
                request_limit.clone(),
                listener_logs,
            ))
        }
        "doh" => {
            let tls = load_server_tls(&cfg.server.cert_file, &cfg.server.key_file)?;
            let listener = TcpListener::bind(&cfg.server.listen)
                .await
                .with_context(|| format!("bind doh listener: {}", cfg.server.listen))?;
            tokio::spawn(run_doh_listener(
                cfg.server.listen.clone(),
                listener,
                cfg.server.path.clone(),
                tls,
                listener_state,
                stop_rx,
                request_limit.clone(),
                listener_logs,
            ))
        }
        _ => return Err(anyhow!("unsupported server mode")),
    };

    let health_tasks = spawn_health_tasks(&cfg, &resolver, &stop_tx);

    Ok(RunningRuntime {
        state,
        stop_tx,
        listener,
        health_tasks,
    })
}

fn spawn_health_tasks(
    cfg: &CoreConfig,
    resolver: &Resolver,
    stop_tx: &watch::Sender<bool>,
) -> Vec<JoinHandle<()>> {
    let mut health_tasks = Vec::new();
    if cfg.healthcheck.enabled {
        let interval =
            parse_go_duration(&cfg.healthcheck.interval).unwrap_or(Duration::from_secs(30));
        let timeout = parse_go_duration(&cfg.healthcheck.timeout).unwrap_or(Duration::from_secs(2));
        let domain = if cfg.healthcheck.domain.is_empty() {
            ".".to_string()
        } else {
            cfg.healthcheck.domain.clone()
        };
        let probe_limit = Arc::new(Semaphore::new(MAX_CONCURRENT_HEALTHCHECKS));
        for (index, client) in resolver.clients.values().cloned().enumerate() {
            let health = resolver.health.clone();
            let mut stop = stop_tx.subscribe();
            let domain = domain.clone();
            let probe_limit = probe_limit.clone();
            let initial_delay = HEALTHCHECK_STAGGER_STEP * index as u32;
            health_tasks.push(tokio::spawn(async move {
                run_health_loop(
                    client,
                    health,
                    domain,
                    interval,
                    timeout,
                    initial_delay,
                    probe_limit,
                    &mut stop,
                )
                .await;
            }));
        }
    }
    health_tasks
}

fn same_server_identity(a: &CoreConfig, b: &CoreConfig) -> bool {
    a.server.mode == b.server.mode
        && a.server.listen == b.server.listen
        && a.server.cert_file == b.server.cert_file
        && a.server.key_file == b.server.key_file
        && a.server.path == b.server.path
}

fn build_resolver(
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
        timeout: if cfg.resolver.timeout.is_empty() {
            None
        } else {
            Some(parse_go_duration(&cfg.resolver.timeout)?)
        },
        ipv6_enabled: cfg.resolver.ipv6_enabled,
        logs,
    })
}

fn endpoint_from_config(item: &CoreUpstreamConfig) -> Result<Endpoint> {
    let url = parse_upstream_endpoint(&item.endpoint)?;
    endpoint_from_url(&url, &item.server_name)
}

fn endpoint_from_url(url: &Url, server_name: &str) -> Result<Endpoint> {
    let scheme = url.scheme().to_string();
    let port = url.port().unwrap_or(match scheme.as_str() {
        "udp" | "tcp" => 53,
        "dot" | "doq" | "quic" => 853,
        "http" => 80,
        "https" => 443,
        _ => 53,
    });
    let path = if (scheme == "http" || scheme == "https") && url.path().is_empty() {
        "/dns-query".to_string()
    } else if scheme == "http" || scheme == "https" {
        url.path().to_string()
    } else {
        String::new()
    };
    let host = url.host_str().unwrap_or_default().to_string();
    let address = format_host_port(&host, port);
    let doh_url = if scheme == "http" || scheme == "https" {
        format!("{scheme}://{address}{path}")
    } else {
        String::new()
    };
    Ok(Endpoint {
        scheme,
        host: host.clone(),
        port,
        address,
        url: doh_url,
        server_name: if server_name.trim().is_empty() {
            host
        } else {
            server_name.trim().to_string()
        },
    })
}

fn format_host_port(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

fn proxy_endpoint_from_raw(raw: &str) -> Result<ProxyEndpoint> {
    let url = Url::parse(raw).with_context(|| format!("invalid proxy endpoint {:?}", raw))?;
    if url.scheme() != "socks5" {
        return Err(anyhow!(
            "unsupported proxy scheme {:?}; only socks5 is supported",
            url.scheme()
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("proxy endpoint {:?} is missing host", raw))?;
    let address = format!("{}:{}", host, url.port().unwrap_or(1080));
    Ok(ProxyEndpoint {
        address,
        username: url.username().to_string(),
        password: url.password().unwrap_or("").to_string(),
    })
}

fn build_http_client(
    proxy: Option<&ProxyEndpoint>,
    bootstrap: Option<&Arc<BootstrapResolver>>,
) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder();
    if let Some(proxy) = proxy {
        let url = if proxy.username.is_empty() {
            format!("socks5h://{}", proxy.address)
        } else {
            format!(
                "socks5h://{}:{}@{}",
                proxy.username, proxy.password, proxy.address
            )
        };
        builder = builder.proxy(reqwest::Proxy::all(url)?);
    } else if let Some(bootstrap) = bootstrap {
        builder = builder.dns_resolver(bootstrap.clone());
    }
    Ok(builder.build()?)
}

fn load_server_tls(cert_file: &str, key_file: &str) -> Result<Arc<RustlsServerConfig>> {
    let certs = {
        let file =
            File::open(cert_file).with_context(|| format!("open certificate file: {cert_file}"))?;
        rustls_pemfile::certs(&mut BufReader::new(file))
            .collect::<std::result::Result<Vec<_>, _>>()?
    };
    let key = {
        let file =
            File::open(key_file).with_context(|| format!("open private key file: {key_file}"))?;
        rustls_pemfile::private_key(&mut BufReader::new(file))?
            .ok_or_else(|| anyhow!("private key file does not contain a supported key"))?
    };
    Ok(Arc::new(
        RustlsServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .context("build server tls config")?,
    ))
}

fn client_tls_config() -> Arc<RustlsClientConfig> {
    static CLIENT_TLS_CONFIG: OnceLock<Arc<RustlsClientConfig>> = OnceLock::new();
    CLIENT_TLS_CONFIG
        .get_or_init(|| {
            let mut roots = RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            Arc::new(
                RustlsClientConfig::builder()
                    .with_root_certificates(roots)
                    .with_no_client_auth(),
            )
        })
        .clone()
}

async fn run_udp_listener(
    addr: String,
    socket: UdpSocket,
    state: Arc<RuntimeState>,
    mut stop: watch::Receiver<bool>,
    request_limit: Arc<Semaphore>,
    logs: LogBuffer,
) {
    let socket = Arc::new(socket);
    logs.push("info", format!("udp listener started on {addr}"));
    let mut buf = vec![0u8; 65535];
    loop {
        tokio::select! {
            _ = stop.changed() => break,
            recv = socket.recv_from(&mut buf) => {
                let Ok((len, peer)) = recv else {
                    continue;
                };
                let permit = wait_for_request_permit(&request_limit, &mut stop).await;
                let Some(permit) = permit else {
                    break;
                };
                let req = buf[..len].to_vec();
                let socket = socket.clone();
                let resolver = state.resolver();
                tokio::spawn(async move {
                    let _permit = permit;
                    let resp = resolver.resolve(req).await.unwrap_or_else(|req| servfail_response(&req));
                    let _ = socket.send_to(&resp, peer).await;
                });
            }
        }
    }
    logs.push("info", "udp listener stopped");
}

async fn run_tcp_listener(
    addr: String,
    listener: TcpListener,
    state: Arc<RuntimeState>,
    mut stop: watch::Receiver<bool>,
    request_limit: Arc<Semaphore>,
    logs: LogBuffer,
) {
    logs.push("info", format!("tcp listener started on {addr}"));
    loop {
        tokio::select! {
            _ = stop.changed() => break,
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else {
                    continue;
                };
                let permit = wait_for_request_permit(&request_limit, &mut stop).await;
                let Some(permit) = permit else {
                    break;
                };
                let resolver = state.resolver();
                tokio::spawn(async move {
                    let _permit = permit;
                    let _ = handle_tcp_client(stream, resolver).await;
                });
            }
        }
    }
    logs.push("info", "tcp listener stopped");
}

async fn handle_tcp_client(mut stream: TcpStream, resolver: Arc<Resolver>) -> Result<()> {
    handle_dns_stream(&mut stream, resolver).await
}

async fn run_dot_listener(
    addr: String,
    listener: TcpListener,
    tls: Arc<RustlsServerConfig>,
    state: Arc<RuntimeState>,
    mut stop: watch::Receiver<bool>,
    request_limit: Arc<Semaphore>,
    logs: LogBuffer,
) {
    let acceptor = TlsAcceptor::from(tls);
    logs.push("info", format!("dot listener started on {addr}"));
    loop {
        tokio::select! {
            _ = stop.changed() => break,
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else {
                    continue;
                };
                let permit = wait_for_request_permit(&request_limit, &mut stop).await;
                let Some(permit) = permit else {
                    break;
                };
                let acceptor = acceptor.clone();
                let resolver = state.resolver();
                tokio::spawn(async move {
                    let _permit = permit;
                    if let Ok(mut tls_stream) = acceptor.accept(stream).await {
                        let _ = handle_dns_stream(&mut tls_stream, resolver).await;
                    }
                });
            }
        }
    }
    logs.push("info", "dot listener stopped");
}

async fn run_doh_listener(
    addr: String,
    listener: TcpListener,
    path: String,
    tls: Arc<RustlsServerConfig>,
    state: Arc<RuntimeState>,
    mut stop: watch::Receiver<bool>,
    request_limit: Arc<Semaphore>,
    logs: LogBuffer,
) {
    let acceptor = TlsAcceptor::from(tls);
    let path = if path.is_empty() {
        "/dns-query".to_string()
    } else {
        path
    };
    logs.push("info", format!("doh listener started on {addr}{path}"));
    loop {
        tokio::select! {
            _ = stop.changed() => break,
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else {
                    continue;
                };
                let permit = wait_for_request_permit(&request_limit, &mut stop).await;
                let Some(permit) = permit else {
                    break;
                };
                let acceptor = acceptor.clone();
                let resolver = state.resolver();
                let path = path.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    if let Ok(mut tls_stream) = acceptor.accept(stream).await {
                        let _ = handle_doh_client(&mut tls_stream, &path, resolver).await;
                    }
                });
            }
        }
    }
    logs.push("info", "doh listener stopped");
}

async fn wait_for_request_permit(
    request_limit: &Arc<Semaphore>,
    stop: &mut watch::Receiver<bool>,
) -> Option<OwnedSemaphorePermit> {
    tokio::select! {
        _ = stop.changed() => None,
        permit = request_limit.clone().acquire_owned() => permit.ok(),
    }
}

async fn handle_dns_stream<S>(stream: &mut S, resolver: Arc<Resolver>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    loop {
        let mut len_buf = [0u8; 2];
        if stream.read_exact(&mut len_buf).await.is_err() {
            return Ok(());
        }
        let len = u16::from_be_bytes(len_buf) as usize;
        let mut req = vec![0u8; len];
        stream.read_exact(&mut req).await?;
        let resp = resolver
            .resolve(req)
            .await
            .unwrap_or_else(|req| servfail_response(&req));
        stream.write_all(&(resp.len() as u16).to_be_bytes()).await?;
        stream.write_all(&resp).await?;
    }
}

async fn handle_doh_client<S>(
    stream: &mut S,
    expected_path: &str,
    resolver: Arc<Resolver>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 2048];
    let header_end;
    loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        buffer.extend_from_slice(&chunk[..n]);
        if buffer.len() > DNS_WIRE_LIMIT + 8192 {
            write_http_response(
                stream,
                413,
                "Payload Too Large",
                b"request entity too large",
            )
            .await?;
            return Ok(());
        }
        if let Some(pos) = find_header_end(&buffer) {
            header_end = pos;
            break;
        }
    }

    let header_text = String::from_utf8_lossy(&buffer[..header_end]);
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    let content_length = lines
        .filter_map(|line| line.split_once(':'))
        .find_map(|(name, value)| {
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);

    let payload = match method {
        "GET" => doh_get_payload(target, expected_path),
        "POST" => {
            if target.split('?').next().unwrap_or_default() != expected_path {
                None
            } else {
                let body_start = header_end + 4;
                while buffer.len() < body_start + content_length {
                    let n = stream.read(&mut chunk).await?;
                    if n == 0 {
                        break;
                    }
                    buffer.extend_from_slice(&chunk[..n]);
                }
                Some(buffer[body_start..buffer.len().min(body_start + content_length)].to_vec())
            }
        }
        _ => {
            write_http_response(stream, 405, "Method Not Allowed", b"method not allowed").await?;
            return Ok(());
        }
    };

    let Some(payload) = payload else {
        write_http_response(stream, 400, "Bad Request", b"bad request").await?;
        return Ok(());
    };
    if payload.len() > DNS_WIRE_LIMIT {
        write_http_response(
            stream,
            413,
            "Payload Too Large",
            b"request entity too large",
        )
        .await?;
        return Ok(());
    }

    let resp = resolver
        .resolve(payload)
        .await
        .unwrap_or_else(|req| servfail_response(&req));
    write_doh_response(stream, &resp).await?;
    Ok(())
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn doh_get_payload(target: &str, expected_path: &str) -> Option<Vec<u8>> {
    let (path, query) = target.split_once('?')?;
    if path != expected_path {
        return None;
    }
    for part in query.split('&') {
        let (name, value) = part.split_once('=')?;
        if name == "dns" {
            return base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(value)
                .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(value))
                .ok();
        }
    }
    None
}

async fn write_doh_response<S>(stream: &mut S, body: &[u8]) -> Result<()>
where
    S: AsyncWrite + Unpin,
{
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/dns-message\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(body).await?;
    Ok(())
}

async fn write_http_response<S>(
    stream: &mut S,
    status: u16,
    reason: &str,
    body: &[u8],
) -> Result<()>
where
    S: AsyncWrite + Unpin,
{
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(body).await?;
    Ok(())
}

impl Resolver {
    fn clear_cache(&self) -> usize {
        self.cache.clear()
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
            );
            return Ok(cached.response);
        }

        // Upstreams are intentionally tried in configured order. A negative response
        // (NOERROR with no answers, or NXDOMAIN) is not final here: later upstreams may
        // still have an answer for split-horizon, geo, or policy-routed domains. Keep the
        // latest negative as a fallback and only return it after every selected upstream
        // has failed to produce an answer.
        let mut last_negative = None;
        let mut attempt_count = 0usize;
        let mut last_error = None;
        for name in selected.iter() {
            let Some(client) = self.clients.get(name) else {
                if debug_logs {
                    self.logs.push(
                        "debug",
                        format!("dns query {qname} {qtype} route {route_id} skipped missing upstream {name}"),
                    );
                }
                continue;
            };
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
                            );
                            return Ok(resp);
                        }
                        ResponseClass::Negative => {
                            let error = "upstream returned no answer";
                            self.health.record_failure(name, error);
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
                            self.health.record_failure(name, error);
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
                    self.health.record_failure(name, err.to_string());
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

impl UpstreamClient {
    async fn exchange(&self, req: &[u8], timeout: Option<Duration>) -> Result<Vec<u8>> {
        let fut = async {
            match self.endpoint.scheme.as_str() {
                "udp" => {
                    if self.proxy.is_some() {
                        self.exchange_socks5_udp(req).await
                    } else {
                        self.exchange_udp(req).await
                    }
                }
                "tcp" => self.exchange_tcp(req).await,
                "dot" => self.exchange_dot(req).await,
                "doq" | "quic" => self.exchange_doq(req).await,
                "http" | "https" => exchange_doh(&self.endpoint, self.http.as_ref(), req).await,
                _ => Err(anyhow!(
                    "unsupported upstream protocol {}",
                    self.endpoint.scheme
                )),
            }
        };
        if let Some(timeout) = timeout {
            tokio::time::timeout(timeout, fut)
                .await
                .context("upstream timeout")?
        } else {
            fut.await
        }
    }

    async fn exchange_udp(&self, req: &[u8]) -> Result<Vec<u8>> {
        let client = self.udp_client().await?;
        let target = self.first_endpoint_addr().await?;
        client.exchange(target, req).await
    }

    async fn udp_client(&self) -> Result<Arc<UdpUpstreamClient>> {
        if let Some(client) = self.udp.lock().clone() {
            return Ok(client);
        }
        let client = UdpUpstreamClient::new().await?;
        let mut udp = self.udp.lock();
        if let Some(existing) = udp.as_ref() {
            return Ok(existing.clone());
        }
        *udp = Some(client.clone());
        Ok(client)
    }

    async fn exchange_socks5_udp(&self, req: &[u8]) -> Result<Vec<u8>> {
        let client = self.socks5_udp_client().await?;
        let result = client.exchange(&self.endpoint.address, req).await;
        if result.is_err() {
            let mut socks5_udp = self.socks5_udp.lock();
            if socks5_udp
                .as_ref()
                .is_some_and(|cached| Arc::ptr_eq(cached, &client))
            {
                *socks5_udp = None;
            }
        }
        result
    }

    async fn socks5_udp_client(&self) -> Result<Arc<Socks5UdpUpstreamClient>> {
        if let Some(client) = self.socks5_udp.lock().clone() {
            return Ok(client);
        }
        let proxy = self
            .proxy
            .as_ref()
            .ok_or_else(|| anyhow!("SOCKS5 UDP proxy is not configured"))?;
        let client = Socks5UdpUpstreamClient::new(proxy).await?;
        let mut socks5_udp = self.socks5_udp.lock();
        if let Some(existing) = socks5_udp.as_ref() {
            return Ok(existing.clone());
        }
        *socks5_udp = Some(client.clone());
        Ok(client)
    }

    async fn exchange_tcp(&self, req: &[u8]) -> Result<Vec<u8>> {
        let client = self.tcp_client().await?;
        let result = client.exchange(req).await;
        if result.is_err() {
            let mut tcp = self.tcp.lock();
            if tcp
                .as_ref()
                .is_some_and(|cached| Arc::ptr_eq(cached, &client))
            {
                *tcp = None;
            }
        }
        result
    }

    async fn tcp_client(&self) -> Result<Arc<LenPrefixedUpstreamClient>> {
        if let Some(client) = self.tcp.lock().clone() {
            return Ok(client);
        }
        let stream: BoxedAsyncIo = if let Some(proxy) = &self.proxy {
            Box::new(socks5_connect(proxy, &self.endpoint.address).await?)
        } else {
            Box::new(self.connect_direct().await?)
        };
        let mut tcp = self.tcp.lock();
        if let Some(existing) = tcp.as_ref() {
            return Ok(existing.clone());
        }
        let client = LenPrefixedUpstreamClient::new(stream);
        *tcp = Some(client.clone());
        Ok(client)
    }

    async fn exchange_dot(&self, req: &[u8]) -> Result<Vec<u8>> {
        let client = self.dot_client().await?;
        let result = client.exchange(req).await;
        if result.is_err() {
            let mut dot = self.dot.lock();
            if dot
                .as_ref()
                .is_some_and(|cached| Arc::ptr_eq(cached, &client))
            {
                *dot = None;
            }
        }
        result
    }

    async fn dot_client(&self) -> Result<Arc<LenPrefixedUpstreamClient>> {
        if let Some(client) = self.dot.lock().clone() {
            return Ok(client);
        }
        let connector = TlsConnector::from(client_tls_config());
        let server_name = ServerName::try_from(self.endpoint.server_name.clone())
            .context("invalid DoT server name")?;
        let stream: BoxedAsyncIo = if let Some(proxy) = &self.proxy {
            let stream = socks5_connect(proxy, &self.endpoint.address).await?;
            Box::new(connector.connect(server_name, stream).await?)
        } else {
            let stream = self.connect_direct().await?;
            Box::new(connector.connect(server_name, stream).await?)
        };
        let mut dot = self.dot.lock();
        if let Some(existing) = dot.as_ref() {
            return Ok(existing.clone());
        }
        let client = LenPrefixedUpstreamClient::new(stream);
        *dot = Some(client.clone());
        Ok(client)
    }

    async fn exchange_doq(&self, req: &[u8]) -> Result<Vec<u8>> {
        if self.proxy.is_some() {
            return Err(anyhow!("DoQ over SOCKS5 is not supported yet"));
        }
        let client = self.doq_client().await?;
        let result = client.exchange(req).await;
        if result.is_err() {
            let mut doq = self.doq.lock();
            if doq
                .as_ref()
                .is_some_and(|cached| Arc::ptr_eq(cached, &client))
            {
                *doq = None;
            }
        }
        result
    }

    async fn doq_client(&self) -> Result<Arc<DoqUpstreamClient>> {
        if let Some(client) = self.doq.lock().clone() {
            return Ok(client);
        }
        let client = DoqUpstreamClient::connect(&self.endpoint, self.bootstrap.as_deref()).await?;
        let mut doq = self.doq.lock();
        if let Some(existing) = doq.as_ref() {
            return Ok(existing.clone());
        }
        *doq = Some(client.clone());
        Ok(client)
    }

    async fn connect_direct(&self) -> Result<TcpStream> {
        let addrs = self.endpoint_addrs().await?;
        connect_socket_addrs(&addrs, &self.endpoint.address).await
    }

    async fn first_endpoint_addr(&self) -> Result<SocketAddr> {
        self.endpoint_addrs()
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| {
                anyhow!(
                    "resolve upstream {} returned no addresses",
                    self.endpoint.host
                )
            })
    }

    async fn endpoint_addrs(&self) -> Result<Vec<SocketAddr>> {
        if let Some(bootstrap) = &self.bootstrap {
            return bootstrap
                .resolve_socket_addrs(&self.endpoint.host, self.endpoint.port)
                .await;
        }
        if let Ok(ip) = self.endpoint.host.parse::<IpAddr>() {
            return Ok(vec![SocketAddr::new(ip, self.endpoint.port)]);
        }
        Ok(tokio::net::lookup_host(&self.endpoint.address)
            .await
            .with_context(|| format!("resolve upstream {}", self.endpoint.address))?
            .collect())
    }
}

async fn connect_socket_addrs(addrs: &[SocketAddr], label: &str) -> Result<TcpStream> {
    let mut last_error = None;
    for addr in addrs {
        match TcpStream::connect(addr).await {
            Ok(stream) => return Ok(stream),
            Err(err) => last_error = Some(err),
        }
    }
    Err(last_error
        .map(|err| anyhow!(err).context(format!("connect upstream {label}")))
        .unwrap_or_else(|| anyhow!("resolve upstream {label} returned no addresses")))
}

impl BootstrapResolver {
    fn new(servers: Vec<SocketAddr>) -> Self {
        Self {
            servers: servers.into(),
        }
    }

    async fn resolve_socket_addrs(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>> {
        if let Ok(ip) = host.parse::<IpAddr>() {
            return Ok(vec![SocketAddr::new(ip, port)]);
        }
        let domain = normalize_domain(host)?;
        let mut last_error = None;
        for server in self.servers.iter() {
            let mut ips = Vec::new();
            for qtype in [TYPE_A, TYPE_AAAA] {
                match bootstrap_lookup(server, &domain, qtype).await {
                    Ok(mut found) => ips.append(&mut found),
                    Err(err) => last_error = Some(err),
                }
            }
            if !ips.is_empty() {
                return Ok(ips
                    .into_iter()
                    .map(|ip| SocketAddr::new(ip, port))
                    .collect());
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow!("bootstrap DNS returned no addresses for {host}")))
    }
}

impl reqwest::dns::Resolve for BootstrapResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let resolver = self.clone();
        let host = name.as_str().to_string();
        Box::pin(async move {
            let addrs = resolver
                .resolve_socket_addrs(&host, 0)
                .await
                .map_err(|err| -> Box<dyn StdError + Send + Sync> { err.into() })?;
            let addrs: reqwest::dns::Addrs = Box::new(addrs.into_iter());
            Ok(addrs)
        })
    }
}

async fn bootstrap_lookup(server: &SocketAddr, domain: &str, qtype: u16) -> Result<Vec<IpAddr>> {
    let bind_addr = if server.is_ipv4() {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)
    } else {
        SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0)
    };
    let socket = UdpSocket::bind(bind_addr).await?;
    let req = build_query(domain, qtype);
    socket.send_to(&req, server).await?;
    let mut buf = [0u8; 4096];
    let (len, _) = tokio::time::timeout(BOOTSTRAP_LOOKUP_TIMEOUT, socket.recv_from(&mut buf))
        .await
        .context("bootstrap DNS timeout")??;
    bootstrap_response_ips(&buf[..len], qtype)
}

fn bootstrap_response_ips(resp: &[u8], qtype: u16) -> Result<Vec<IpAddr>> {
    let msg = DnsMessage::from_vec(resp).context("decode bootstrap DNS response")?;
    if !matches!(rcode(resp), Some(RCODE_SUCCESS)) {
        return Ok(Vec::new());
    }
    Ok(msg
        .answers
        .iter()
        .filter(|record| u16::from(record.record_type()) == qtype)
        .filter_map(|record| format_hickory_rdata(&record.data).parse::<IpAddr>().ok())
        .collect())
}

impl UdpUpstreamClient {
    async fn new() -> Result<Arc<Self>> {
        let socket = Arc::new(UdpSocket::bind("0.0.0.0:0").await?);
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let (stop_tx, stop_rx) = watch::channel(false);
        let client = Arc::new(Self {
            socket,
            pending,
            next_id: Mutex::new(0),
            stop_tx,
        });
        tokio::spawn(run_udp_upstream_recv_loop(
            client.socket.clone(),
            client.pending.clone(),
            stop_rx,
        ));
        Ok(client)
    }

    async fn exchange(&self, addr: SocketAddr, req: &[u8]) -> Result<Vec<u8>> {
        let original_id = dns_message_id(req).ok_or_else(|| anyhow!("dns query is too short"))?;
        let local_id = self.allocate_id();
        let mut upstream_req = req.to_vec();
        set_id(&mut upstream_req, local_id);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(local_id, tx);
        let pending_guard = PendingUdpResponseGuard {
            pending: self.pending.clone(),
            id: local_id,
            active: true,
        };

        if let Err(err) = self.socket.send_to(&upstream_req, addr).await {
            return Err(err.into());
        }

        let result = match rx.await {
            Ok(mut resp) => {
                set_id(&mut resp, original_id);
                Ok(resp)
            }
            Err(_) => Err(anyhow!("udp upstream response receiver closed")),
        };
        pending_guard.disarm();
        result
    }

    fn allocate_id(&self) -> u16 {
        let mut next_id = self.next_id.lock();
        loop {
            *next_id = next_id.wrapping_add(1);
            let id = *next_id;
            if !self.pending.lock().contains_key(&id) {
                return id;
            }
        }
    }
}

impl Drop for UdpUpstreamClient {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(true);
        self.pending.lock().clear();
    }
}

impl Socks5UdpUpstreamClient {
    async fn new(proxy: &ProxyEndpoint) -> Result<Arc<Self>> {
        let (control, relay_address) = socks5_udp_associate(proxy).await?;
        let socket = Arc::new(UdpSocket::bind("0.0.0.0:0").await?);
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let (stop_tx, stop_rx) = watch::channel(false);
        let client = Arc::new(Self {
            _control: control,
            socket,
            relay_address,
            pending,
            next_id: Mutex::new(0),
            stop_tx,
        });
        tokio::spawn(run_socks5_udp_recv_loop(
            client.socket.clone(),
            client.pending.clone(),
            stop_rx,
        ));
        Ok(client)
    }

    async fn exchange(&self, target: &str, req: &[u8]) -> Result<Vec<u8>> {
        let original_id = dns_message_id(req).ok_or_else(|| anyhow!("dns query is too short"))?;
        let local_id = self.allocate_id();
        let mut upstream_req = req.to_vec();
        set_id(&mut upstream_req, local_id);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(local_id, tx);
        let pending_guard = PendingUdpResponseGuard {
            pending: self.pending.clone(),
            id: local_id,
            active: true,
        };

        let packet = socks5_udp_packet(target, &upstream_req)?;
        if let Err(err) = self.socket.send_to(&packet, &self.relay_address).await {
            return Err(err.into());
        }

        let result = match rx.await {
            Ok(mut resp) => {
                set_id(&mut resp, original_id);
                Ok(resp)
            }
            Err(_) => Err(anyhow!("SOCKS5 UDP response receiver closed")),
        };
        pending_guard.disarm();
        result
    }

    fn allocate_id(&self) -> u16 {
        let mut next_id = self.next_id.lock();
        loop {
            *next_id = next_id.wrapping_add(1);
            let id = *next_id;
            if !self.pending.lock().contains_key(&id) {
                return id;
            }
        }
    }
}

impl Drop for Socks5UdpUpstreamClient {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(true);
        self.pending.lock().clear();
    }
}

impl LenPrefixedUpstreamClient {
    fn new(stream: BoxedAsyncIo) -> Arc<Self> {
        let (reader, writer) = split(stream);
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let client = Arc::new(Self {
            writer: AsyncMutex::new(writer),
            pending,
            next_id: Mutex::new(0),
        });
        tokio::spawn(run_len_prefixed_recv_loop(reader, client.pending.clone()));
        client
    }

    async fn exchange(&self, req: &[u8]) -> Result<Vec<u8>> {
        let original_id = dns_message_id(req).ok_or_else(|| anyhow!("dns query is too short"))?;
        let local_id = self.allocate_id();
        let mut upstream_req = req.to_vec();
        set_id(&mut upstream_req, local_id);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(local_id, tx);
        let pending_guard = PendingUdpResponseGuard {
            pending: self.pending.clone(),
            id: local_id,
            active: true,
        };

        let query = len_prefixed_dns_message(&upstream_req)?;
        {
            let mut writer = self.writer.lock().await;
            if let Err(err) = writer.write_all(&query).await {
                return Err(err.into());
            }
        }

        let result = match rx.await {
            Ok(mut resp) => {
                set_id(&mut resp, original_id);
                Ok(resp)
            }
            Err(_) => Err(anyhow!("stream upstream response receiver closed")),
        };
        pending_guard.disarm();
        result
    }

    fn allocate_id(&self) -> u16 {
        let mut next_id = self.next_id.lock();
        loop {
            *next_id = next_id.wrapping_add(1);
            let id = *next_id;
            if !self.pending.lock().contains_key(&id) {
                return id;
            }
        }
    }
}

impl DoqUpstreamClient {
    async fn connect(
        endpoint: &Endpoint,
        bootstrap: Option<&BootstrapResolver>,
    ) -> Result<Arc<Self>> {
        let remote = if let Some(bootstrap) = bootstrap {
            bootstrap
                .resolve_socket_addrs(&endpoint.host, endpoint.port)
                .await?
                .into_iter()
                .next()
                .ok_or_else(|| {
                    anyhow!(
                        "resolve DoQ upstream {} returned no addresses",
                        endpoint.host
                    )
                })?
        } else {
            endpoint
                .address
                .parse()
                .with_context(|| format!("parse DoQ remote address {}", endpoint.address))?
        };
        let mut quic_endpoint = quinn::Endpoint::client("0.0.0.0:0".parse()?)?;
        quic_endpoint.set_default_client_config(doq_client_config()?);
        let connection = quic_endpoint
            .connect(remote, &endpoint.server_name)?
            .await
            .with_context(|| format!("connect DoQ upstream {}", endpoint.address))?;
        Ok(Arc::new(Self {
            _endpoint: quic_endpoint,
            connection,
        }))
    }

    async fn exchange(&self, req: &[u8]) -> Result<Vec<u8>> {
        let (mut send, mut recv) = self.connection.open_bi().await.context("open DoQ stream")?;
        let query = len_prefixed_dns_message(req)?;
        send.write_all(&query).await.context("write DoQ query")?;
        send.finish().context("finish DoQ query stream")?;
        let response = recv
            .read_to_end(DNS_WIRE_LIMIT + 2)
            .await
            .context("read DoQ response")?;
        parse_len_prefixed_dns_message(&response)
    }
}

struct PendingUdpResponseGuard {
    pending: PendingUdpResponses,
    id: u16,
    active: bool,
}

impl PendingUdpResponseGuard {
    fn disarm(mut self) {
        self.active = false;
    }
}

impl Drop for PendingUdpResponseGuard {
    fn drop(&mut self) {
        if self.active {
            self.pending.lock().remove(&self.id);
        }
    }
}

async fn run_udp_upstream_recv_loop(
    socket: Arc<UdpSocket>,
    pending: PendingUdpResponses,
    mut stop: watch::Receiver<bool>,
) {
    let mut buf = vec![0u8; DNS_WIRE_LIMIT];
    loop {
        let recv = tokio::select! {
            _ = stop.changed() => break,
            recv = socket.recv_from(&mut buf) => recv,
        };
        let Ok((len, _)) = recv else {
            break;
        };
        let resp = buf[..len].to_vec();
        let Some(id) = dns_message_id(&resp) else {
            continue;
        };
        if let Some(tx) = pending.lock().remove(&id) {
            let _ = tx.send(resp);
        }
    }
}

async fn run_socks5_udp_recv_loop(
    socket: Arc<UdpSocket>,
    pending: PendingUdpResponses,
    mut stop: watch::Receiver<bool>,
) {
    let mut buf = vec![0u8; DNS_WIRE_LIMIT + 512];
    loop {
        let recv = tokio::select! {
            _ = stop.changed() => break,
            recv = socket.recv_from(&mut buf) => recv,
        };
        let Ok((len, _)) = recv else {
            break;
        };
        let Ok(resp) = parse_socks5_udp_payload(&buf[..len]) else {
            continue;
        };
        let Some(id) = dns_message_id(&resp) else {
            continue;
        };
        if let Some(tx) = pending.lock().remove(&id) {
            let _ = tx.send(resp);
        }
    }
    pending.lock().clear();
}

async fn run_len_prefixed_recv_loop(
    mut reader: ReadHalf<BoxedAsyncIo>,
    pending: PendingStreamResponses,
) {
    loop {
        let mut len_buf = [0u8; 2];
        if reader.read_exact(&mut len_buf).await.is_err() {
            break;
        }
        let len = u16::from_be_bytes(len_buf) as usize;
        let mut resp = vec![0u8; len];
        if reader.read_exact(&mut resp).await.is_err() {
            break;
        }
        let Some(id) = dns_message_id(&resp) else {
            continue;
        };
        if let Some(tx) = pending.lock().remove(&id) {
            let _ = tx.send(resp);
        }
    }
    pending.lock().clear();
}

async fn socks5_connect(proxy: &ProxyEndpoint, target: &str) -> Result<TcpStream> {
    let mut stream = TcpStream::connect(&proxy.address)
        .await
        .with_context(|| format!("connect SOCKS5 proxy {}", proxy.address))?;

    socks5_negotiate(&mut stream, proxy).await?;
    socks5_send_request(&mut stream, 0x01, target).await?;
    Ok(stream)
}

async fn socks5_udp_associate(proxy: &ProxyEndpoint) -> Result<(TcpStream, String)> {
    let mut stream = TcpStream::connect(&proxy.address)
        .await
        .with_context(|| format!("connect SOCKS5 proxy {}", proxy.address))?;

    socks5_negotiate(&mut stream, proxy).await?;
    let relay = socks5_send_request(&mut stream, 0x03, "0.0.0.0:0").await?;
    Ok((stream, relay))
}

async fn socks5_negotiate(stream: &mut TcpStream, proxy: &ProxyEndpoint) -> Result<()> {
    if proxy.username.is_empty() {
        stream.write_all(&[0x05, 0x01, 0x00]).await?;
    } else {
        stream.write_all(&[0x05, 0x02, 0x00, 0x02]).await?;
    }
    let mut method = [0u8; 2];
    stream.read_exact(&mut method).await?;
    if method[0] != 0x05 || method[1] == 0xff {
        return Err(anyhow!("SOCKS5 proxy rejected authentication methods"));
    }

    if method[1] == 0x02 {
        write_socks5_auth(stream, &proxy.username, &proxy.password).await?;
    } else if method[1] != 0x00 {
        return Err(anyhow!(
            "SOCKS5 proxy selected unsupported authentication method {}",
            method[1]
        ));
    }
    Ok(())
}

async fn socks5_send_request(stream: &mut TcpStream, command: u8, target: &str) -> Result<String> {
    let (host, port) = split_host_port(target)?;
    let host_bytes = host.as_bytes();
    if host_bytes.len() > u8::MAX as usize {
        return Err(anyhow!("SOCKS5 target host is too long"));
    }
    let mut request = Vec::with_capacity(7 + host_bytes.len());
    request.extend_from_slice(&[0x05, command, 0x00, 0x03, host_bytes.len() as u8]);
    request.extend_from_slice(host_bytes);
    request.extend_from_slice(&port.to_be_bytes());
    stream.write_all(&request).await?;

    let mut header = [0u8; 4];
    stream.read_exact(&mut header).await?;
    if header[0] != 0x05 {
        return Err(anyhow!("invalid SOCKS5 response"));
    }
    if header[1] != 0x00 {
        return Err(anyhow!(
            "SOCKS5 request failed with reply code {}",
            header[1]
        ));
    }
    read_socks5_address(stream, header[3]).await
}

async fn read_socks5_address(stream: &mut TcpStream, atyp: u8) -> Result<String> {
    match atyp {
        0x01 => {
            let mut addr = [0u8; 4];
            stream.read_exact(&mut addr).await?;
            let mut port = [0u8; 2];
            stream.read_exact(&mut port).await?;
            Ok(format!(
                "{}.{}.{}.{}:{}",
                addr[0],
                addr[1],
                addr[2],
                addr[3],
                u16::from_be_bytes(port)
            ))
        }
        0x03 => {
            let mut len = [0u8; 1];
            stream.read_exact(&mut len).await?;
            let mut host = vec![0u8; len[0] as usize];
            stream.read_exact(&mut host).await?;
            let mut port = [0u8; 2];
            stream.read_exact(&mut port).await?;
            Ok(format!(
                "{}:{}",
                String::from_utf8_lossy(&host),
                u16::from_be_bytes(port)
            ))
        }
        0x04 => {
            let mut addr = [0u8; 16];
            stream.read_exact(&mut addr).await?;
            let mut port = [0u8; 2];
            stream.read_exact(&mut port).await?;
            Ok(format!(
                "[{}]:{}",
                std::net::Ipv6Addr::from(addr),
                u16::from_be_bytes(port)
            ))
        }
        other => {
            return Err(anyhow!(
                "SOCKS5 response has unsupported address type {}",
                other
            ))
        }
    }
}

async fn write_socks5_auth(stream: &mut TcpStream, username: &str, password: &str) -> Result<()> {
    let username = username.as_bytes();
    let password = password.as_bytes();
    if username.len() > u8::MAX as usize || password.len() > u8::MAX as usize {
        return Err(anyhow!("SOCKS5 credentials are too long"));
    }
    let mut request = Vec::with_capacity(3 + username.len() + password.len());
    request.push(0x01);
    request.push(username.len() as u8);
    request.extend_from_slice(username);
    request.push(password.len() as u8);
    request.extend_from_slice(password);
    stream.write_all(&request).await?;
    let mut response = [0u8; 2];
    stream.read_exact(&mut response).await?;
    if response != [0x01, 0x00] {
        return Err(anyhow!("SOCKS5 username/password authentication failed"));
    }
    Ok(())
}

fn split_host_port(address: &str) -> Result<(&str, u16)> {
    let (host, port) = address
        .rsplit_once(':')
        .ok_or_else(|| anyhow!("address {:?} is missing port", address))?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let port = port
        .parse::<u16>()
        .with_context(|| format!("parse port in {address:?}"))?;
    if host.is_empty() {
        return Err(anyhow!("address {:?} is missing host", address));
    }
    Ok((host, port))
}

fn socks5_udp_packet(target: &str, payload: &[u8]) -> Result<Vec<u8>> {
    let (host, port) = split_host_port(target)?;
    let host_bytes = host.as_bytes();
    if host_bytes.len() > u8::MAX as usize {
        return Err(anyhow!("SOCKS5 UDP target host is too long"));
    }
    let mut packet = Vec::with_capacity(7 + host_bytes.len() + payload.len());
    packet.extend_from_slice(&[0x00, 0x00, 0x00, 0x03, host_bytes.len() as u8]);
    packet.extend_from_slice(host_bytes);
    packet.extend_from_slice(&port.to_be_bytes());
    packet.extend_from_slice(payload);
    Ok(packet)
}

fn parse_socks5_udp_payload(packet: &[u8]) -> Result<Vec<u8>> {
    if packet.len() < 7 {
        return Err(anyhow!("SOCKS5 UDP response is too short"));
    }
    if packet[0] != 0 || packet[1] != 0 || packet[2] != 0 {
        return Err(anyhow!("SOCKS5 UDP fragmentation is not supported"));
    }
    let mut offset = 4;
    match packet[3] {
        0x01 => offset += 4,
        0x03 => {
            let len = *packet
                .get(offset)
                .ok_or_else(|| anyhow!("SOCKS5 UDP domain length is missing"))?
                as usize;
            offset += 1 + len;
        }
        0x04 => offset += 16,
        other => {
            return Err(anyhow!(
                "SOCKS5 UDP response has unsupported address type {}",
                other
            ))
        }
    }
    offset += 2;
    if offset > packet.len() {
        return Err(anyhow!("SOCKS5 UDP response address is truncated"));
    }
    Ok(packet[offset..].to_vec())
}

fn doq_client_config() -> Result<quinn::ClientConfig> {
    use quinn::crypto::rustls::QuicClientConfig;

    let mut roots = quinn::rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let mut tls = quinn::rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    tls.alpn_protocols = vec![b"doq".to_vec()];
    Ok(quinn::ClientConfig::new(Arc::new(
        QuicClientConfig::try_from(tls)?,
    )))
}

fn len_prefixed_dns_message(req: &[u8]) -> Result<Vec<u8>> {
    if req.len() > u16::MAX as usize {
        return Err(anyhow!("DNS message is too large"));
    }
    let mut out = Vec::with_capacity(req.len() + 2);
    out.extend_from_slice(&(req.len() as u16).to_be_bytes());
    out.extend_from_slice(req);
    Ok(out)
}

fn parse_len_prefixed_dns_message(raw: &[u8]) -> Result<Vec<u8>> {
    if raw.len() < 2 {
        return Err(anyhow!("length-prefixed DNS response is too short"));
    }
    let len = u16::from_be_bytes([raw[0], raw[1]]) as usize;
    if raw.len() < len + 2 {
        return Err(anyhow!("length-prefixed DNS response is truncated"));
    }
    Ok(raw[2..2 + len].to_vec())
}

async fn exchange_doh(
    endpoint: &Endpoint,
    http: Option<&reqwest::Client>,
    req: &[u8],
) -> Result<Vec<u8>> {
    let client = http.ok_or_else(|| anyhow!("DoH HTTP client is not initialized"))?;
    let resp = client
        .post(&endpoint.url)
        .header(CONTENT_TYPE, "application/dns-message")
        .header(ACCEPT, "application/dns-message")
        .body(req.to_vec())
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(anyhow!("unexpected DoH status {}", status.as_u16()));
    }
    Ok(resp.bytes().await?.to_vec())
}

async fn run_health_loop(
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
        if health.recent_query_success(&client.name, interval) {
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
        let qtype = if domain == "." { 2 } else { TYPE_A };
        let req = build_query(&domain, qtype);
        let started = Instant::now();
        match client.exchange(&req, Some(timeout)).await {
            Ok(resp) if !matches!(rcode(&resp), Some(RCODE_SERVER_FAILURE | RCODE_REFUSED)) => {
                health.record_probe_success(&client.name, started.elapsed())
            }
            Ok(_) => {
                health.record_probe_failure(&client.name, "healthcheck returned retryable response")
            }
            Err(err) => health.record_probe_failure(&client.name, err.to_string()),
        }
        drop(permit);
        let delay = health.probe_delay(&client.name, interval);
        tokio::select! {
            _ = stop.changed() => break,
            _ = tokio::time::sleep(delay) => {}
        }
    }
}

async fn wait_for_healthcheck_permit(
    probe_limit: &Arc<Semaphore>,
    stop: &mut watch::Receiver<bool>,
) -> Option<OwnedSemaphorePermit> {
    tokio::select! {
        _ = stop.changed() => None,
        permit = probe_limit.clone().acquire_owned() => permit.ok(),
    }
}

impl HealthMonitor {
    fn new(
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
                        failure_streak: 0,
                        recovery_streak: 0,
                        probe_failure_streak: 0,
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

    fn set_listener(&self, listener: HealthListener) {
        *self.listener.lock() = Some(listener);
    }

    fn notify_listener(&self) {
        let listener = self.listener.lock().clone();
        if let Some(listener) = listener {
            listener();
        }
    }

    fn snapshot(&self) -> HealthSnapshot {
        HealthSnapshot {
            enabled: self.enabled,
            states: self.states.lock().clone(),
        }
    }

    fn recent_query_success(&self, name: &str, window: Duration) -> bool {
        self.states
            .lock()
            .get(name)
            .and_then(|state| state.last_query_success_at)
            .map(|last_success| last_success.elapsed() < window)
            .unwrap_or(false)
    }

    fn probe_delay(&self, name: &str, interval: Duration) -> Duration {
        let Some(state) = self.states.lock().get(name).cloned() else {
            return interval;
        };
        if state.healthy || state.probe_failure_streak == 0 {
            return interval;
        }
        let multiplier = 1u32 << state.probe_failure_streak.saturating_sub(1).min(3);
        interval
            .saturating_mul(multiplier)
            .min(HEALTHCHECK_MAX_BACKOFF)
    }

    fn record_query_success(&self, name: &str, latency: Duration) {
        self.record_success(name, latency, true);
    }

    fn record_probe_success(&self, name: &str, latency: Duration) {
        self.record_success(name, latency, false);
    }

    fn record_probe_failure(&self, name: &str, err: impl Into<String>) {
        self.record_failure(name, err);
    }

    fn record_success(&self, name: &str, latency: Duration, real_query: bool) {
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
            state.probe_failure_streak = 0;
            if self.enabled {
                state.failure_streak = 0;
                if state.healthy {
                    state.recovery_streak = 0;
                } else {
                    state.recovery_streak += 1;
                    if state.recovery_streak >= self.recovery_threshold {
                        state.healthy = true;
                        state.recovery_streak = 0;
                    }
                }
            }
        }
        self.notify_listener();
    }

    fn record_failure(&self, name: &str, err: impl Into<String>) {
        {
            let mut states = self.states.lock();
            let Some(state) = states.get_mut(name) else {
                return;
            };
            state.failure_count += 1;
            state.last_error = Some(err.into());
            state.probe_failure_streak = state.probe_failure_streak.saturating_add(1);
            if self.enabled {
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
}

impl DnsCache {
    fn new(cfg: &CoreConfig) -> Self {
        Self {
            enabled: cfg.cache.enabled,
            max_entry_size: cfg.cache.max_entry_size,
            min_ttl: cfg.cache.min_ttl,
            max_ttl: cfg.cache.max_ttl,
            negative_ttl: cfg.cache.negative_ttl,
            inner: Cache::builder()
                .max_capacity(cfg.cache.max_entries.max(1) as u64)
                .build(),
        }
    }

    fn clear(&self) -> usize {
        self.inner.run_pending_tasks();
        let len = self.inner.entry_count() as usize;
        self.inner.invalidate_all();
        self.inner.run_pending_tasks();
        len
    }

    fn get(&self, key: &CacheKey, id: u16) -> Option<CachedResponse> {
        if !self.enabled {
            return None;
        }
        let now = Instant::now();
        let entry = self.inner.get(key)?;
        if now >= entry.expires_at {
            self.inner.invalidate(key);
            return None;
        }
        let mut resp = entry.response;
        set_id(&mut resp, id);
        let remaining = entry
            .expires_at
            .saturating_duration_since(now)
            .as_secs()
            .min(u32::MAX as u64) as u32;
        if !entry.original_ttls.is_empty() {
            let _ = rewrite_ttls(
                &mut resp,
                &entry.original_ttls,
                &entry.ttl_offsets,
                remaining,
            );
        }
        Some(CachedResponse {
            response: resp,
            upstream_name: entry.upstream_name,
            upstream_protocol: entry.upstream_protocol,
        })
    }

    fn set(
        &self,
        key: CacheKey,
        resp: &[u8],
        negative: bool,
        upstream_name: &str,
        upstream_protocol: &str,
    ) {
        if !self.enabled || resp.len() > self.max_entry_size {
            return;
        }
        let (original_ttls, ttl_offsets) = if negative {
            (Vec::new(), Vec::new())
        } else {
            rr_ttls_with_offsets(resp).unwrap_or_default()
        };
        let ttl = if negative && self.negative_ttl > 0 {
            self.negative_ttl
        } else {
            clamp_ttl(
                min_ttl_from_values(&original_ttls),
                self.min_ttl,
                self.max_ttl,
            )
        };
        if ttl == 0 {
            return;
        }
        self.inner.insert(
            key,
            CacheEntry {
                response: resp.to_vec(),
                expires_at: Instant::now() + Duration::from_secs(ttl as u64),
                original_ttls,
                ttl_offsets,
                upstream_name: upstream_name.to_string(),
                upstream_protocol: upstream_protocol.to_string(),
            },
        );
    }
}

impl HealthSnapshot {
    fn healthy(&self, name: &str) -> bool {
        if !self.enabled {
            return true;
        }
        self.states
            .get(name)
            .map(|state| state.healthy)
            .unwrap_or(false)
    }

    fn diagnostics(&self, name: &str) -> UpstreamDiagnostics {
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

fn parse_question(req: &[u8]) -> Result<Question> {
    if req.len() < 12 {
        return Err(anyhow!("dns query is too short"));
    }
    let qdcount = u16::from_be_bytes([req[4], req[5]]);
    if qdcount != 1 {
        return Err(anyhow!("resolver only supports single-question requests"));
    }
    let id = u16::from_be_bytes([req[0], req[1]]);
    let flags = u16::from_be_bytes([req[2], req[3]]);
    let (qname, offset) = parse_question_name(req, 12)?;
    if offset + 4 > req.len() {
        return Err(anyhow!("dns question is truncated"));
    }
    let qtype = u16::from_be_bytes([req[offset], req[offset + 1]]);
    let qclass = u16::from_be_bytes([req[offset + 2], req[offset + 3]]);
    let name = if qname.is_empty() {
        DnsName::root()
    } else {
        DnsName::from_ascii(&qname).context("decode dns question name")?
    };
    let mut query = DnsQuery::query(name, DnsRecordType::from(qtype));
    query.set_query_class(DNSClass::from(qclass));
    Ok(Question {
        id,
        normalized_qname: normalize_domain_lossy(&qname),
        qtype,
        qclass,
        query,
        op_code: DnsOpCode::from_u8(((flags >> 11) & 0x0f) as u8),
        recursion_desired: flags & 0x0100 != 0,
        checking_disabled: flags & 0x0010 != 0,
    })
}

fn parse_question_name(req: &[u8], offset: usize) -> Result<(String, usize)> {
    let mut name = String::new();
    let mut pos = offset;
    let mut next_offset = None;
    let mut jumps = 0usize;

    loop {
        let len = *req
            .get(pos)
            .ok_or_else(|| anyhow!("dns name is truncated"))?;
        if len & 0b1100_0000 == 0b1100_0000 {
            let next = *req
                .get(pos + 1)
                .ok_or_else(|| anyhow!("dns compression pointer is truncated"))?;
            let pointer = (((len & 0b0011_1111) as usize) << 8) | next as usize;
            if pointer >= req.len() {
                return Err(anyhow!("dns compression pointer is out of bounds"));
            }
            next_offset.get_or_insert(pos + 2);
            pos = pointer;
            jumps += 1;
            if jumps > req.len() {
                return Err(anyhow!("dns compression pointer loop detected"));
            }
            continue;
        }
        if len & 0b1100_0000 != 0 {
            return Err(anyhow!("unsupported dns name label type"));
        }
        pos += 1;
        if len == 0 {
            return Ok((name, next_offset.unwrap_or(pos)));
        }
        let end = pos + len as usize;
        if end > req.len() {
            return Err(anyhow!("dns name label is truncated"));
        }
        let label =
            std::str::from_utf8(&req[pos..end]).context("dns name label is not valid utf-8")?;
        if !name.is_empty() {
            name.push('.');
        }
        name.push_str(label);
        pos = end;
    }
}

fn hosts_response(question: &Question, ipv4: &[[u8; 4]], ipv6: &[[u8; 16]]) -> Vec<u8> {
    let name = DnsName::from_ascii(&question.normalized_qname).unwrap_or_else(|_| DnsName::root());
    let mut resp = response_for_question(question, DnsResponseCode::NoError);
    for ip in ipv4 {
        resp.add_answer(DnsRecord::from_rdata(
            name.clone(),
            60,
            DnsRData::A(DnsA::new(ip[0], ip[1], ip[2], ip[3])),
        ));
    }
    for ip in ipv6 {
        resp.add_answer(DnsRecord::from_rdata(
            name.clone(),
            60,
            DnsRData::AAAA(DnsAaaa::from(std::net::Ipv6Addr::from(*ip))),
        ));
    }
    resp.to_vec()
        .unwrap_or_else(|_| servfail_response_for_question(question))
}

#[cfg(test)]
fn write_rr_header(resp: &mut Vec<u8>, qtype: u16, qclass: u16, ttl: u32, rdlen: u16) {
    resp.extend_from_slice(&[0xc0, 0x0c]);
    resp.extend_from_slice(&qtype.to_be_bytes());
    resp.extend_from_slice(&qclass.to_be_bytes());
    resp.extend_from_slice(&ttl.to_be_bytes());
    resp.extend_from_slice(&rdlen.to_be_bytes());
}

fn servfail_response(req: &[u8]) -> Vec<u8> {
    response_for_query(req, DnsResponseCode::ServFail)
        .to_vec()
        .unwrap_or_else(|_| req.to_vec())
}

fn servfail_response_for_question(question: &Question) -> Vec<u8> {
    response_for_question(question, DnsResponseCode::ServFail)
        .to_vec()
        .unwrap_or_default()
}

fn empty_success_response(question: &Question) -> Vec<u8> {
    response_for_question(question, DnsResponseCode::NoError)
        .to_vec()
        .unwrap_or_else(|_| servfail_response_for_question(question))
}

fn build_query(domain: &str, qtype: u16) -> Vec<u8> {
    let domain = normalize_domain_lossy(domain);
    let name = if domain.is_empty() {
        DnsName::root()
    } else {
        DnsName::from_ascii(&domain).unwrap_or_else(|_| DnsName::root())
    };
    let mut msg = DnsMessage::new(0x1234, DnsMessageType::Query, DnsOpCode::Query);
    msg.metadata.recursion_desired = true;
    msg.add_query(DnsQuery::query(name, DnsRecordType::from(qtype)));
    msg.to_vec().unwrap_or_default()
}

fn lookup_qtype(record_type: &str) -> Result<u16> {
    match record_type.trim().to_ascii_uppercase().as_str() {
        "A" => Ok(TYPE_A),
        "AAAA" => Ok(TYPE_AAAA),
        "CNAME" => Ok(TYPE_CNAME),
        "MX" => Ok(TYPE_MX),
        "TXT" => Ok(TYPE_TXT),
        "NS" => Ok(TYPE_NS),
        "SOA" => Ok(TYPE_SOA),
        "HTTPS" => Ok(TYPE_HTTPS),
        value => Err(anyhow!("unsupported DNS record type {value:?}")),
    }
}

fn qtype_label(qtype: u16) -> &'static str {
    match qtype {
        TYPE_A => "A",
        TYPE_AAAA => "AAAA",
        TYPE_CNAME => "CNAME",
        TYPE_MX => "MX",
        TYPE_TXT => "TXT",
        TYPE_NS => "NS",
        TYPE_SOA => "SOA",
        TYPE_HTTPS => "HTTPS",
        _ => "UNKNOWN",
    }
}

fn response_code_label(code: Option<u8>) -> String {
    match code {
        Some(RCODE_SUCCESS) => "NOERROR".into(),
        Some(RCODE_NAME_ERROR) => "NXDOMAIN".into(),
        Some(RCODE_SERVER_FAILURE) => "SERVFAIL".into(),
        Some(RCODE_REFUSED) => "REFUSED".into(),
        Some(value) => format!("RCODE {value}"),
        None => "INVALID".into(),
    }
}

fn parse_lookup_records(resp: &[u8]) -> Result<Vec<DnsLookupRecord>> {
    let msg = DnsMessage::from_vec(resp).context("decode dns response")?;
    Ok(msg
        .answers
        .iter()
        .map(|record| {
            let rr_type: u16 = record.record_type().into();
            DnsLookupRecord {
                name: normalize_domain_lossy(&record.name.to_ascii()),
                record_type: qtype_label(rr_type).to_string(),
                ttl: record.ttl,
                value: format_hickory_rdata(&record.data),
            }
        })
        .collect())
}

fn format_hickory_rdata(data: &DnsRData) -> String {
    match data {
        DnsRData::SOA(soa) => {
            let mname = normalize_domain_lossy(&soa.mname.to_ascii());
            let rname = normalize_domain_lossy(&soa.rname.to_ascii());
            format!("{mname} {rname} serial={}", soa.serial)
        }
        _ => normalize_domain_lossy(&data.to_string()),
    }
}

fn response_for_query(req: &[u8], code: DnsResponseCode) -> DnsMessage {
    match DnsMessage::from_vec(req) {
        Ok(query) => {
            let mut response = DnsMessage::response(query.metadata.id, query.metadata.op_code);
            response.metadata.recursion_desired = query.metadata.recursion_desired;
            response.metadata.recursion_available = true;
            response.metadata.checking_disabled = query.metadata.checking_disabled;
            response.metadata.response_code = code;
            response.add_queries(query.queries);
            response
        }
        Err(_) => {
            let id = dns_message_id(req).unwrap_or(0);
            let mut response = DnsMessage::response(id, DnsOpCode::Query);
            response.metadata.response_code = code;
            response
        }
    }
}

fn response_for_question(question: &Question, code: DnsResponseCode) -> DnsMessage {
    let mut response = DnsMessage::response(question.id, question.op_code);
    response.metadata.recursion_desired = question.recursion_desired;
    response.metadata.recursion_available = true;
    response.metadata.checking_disabled = question.checking_disabled;
    response.metadata.response_code = code;
    response.add_query(question.query.clone());
    response
}

enum ResponseClass {
    Answer,
    Negative,
    Retry,
}

fn classify(resp: &[u8]) -> ResponseClass {
    // Negative responses deliberately mean "try the next upstream, but remember this
    // response as a fallback." This keeps ordered fallback semantics for domains that
    // only exist on a later upstream.
    match (rcode(resp), answer_count(resp)) {
        (Some(RCODE_SUCCESS), Some(count)) if count > 0 => ResponseClass::Answer,
        (Some(RCODE_SUCCESS), _) | (Some(RCODE_NAME_ERROR), _) => ResponseClass::Negative,
        _ => ResponseClass::Retry,
    }
}

fn rcode(resp: &[u8]) -> Option<u8> {
    (resp.len() >= 4).then(|| resp[3] & 0x0f)
}

fn answer_count(resp: &[u8]) -> Option<u16> {
    (resp.len() >= 8).then(|| u16::from_be_bytes([resp[6], resp[7]]))
}

fn answer_min_ttl(resp: &[u8]) -> Result<Option<u32>> {
    let ttl_offsets = section_ttl_offsets(resp, true)?;
    Ok(ttl_offsets
        .into_iter()
        .filter_map(|offset| {
            (offset + 4 <= resp.len()).then(|| {
                u32::from_be_bytes([
                    resp[offset],
                    resp[offset + 1],
                    resp[offset + 2],
                    resp[offset + 3],
                ])
            })
        })
        .min())
}

fn strip_aaaa_records(resp: &[u8]) -> Result<Vec<u8>> {
    let mut msg = DnsMessage::from_vec(resp).context("decode dns response")?;
    msg.answers
        .retain(|record| record.record_type() != DnsRecordType::AAAA);
    msg.authorities
        .retain(|record| record.record_type() != DnsRecordType::AAAA);
    msg.additionals
        .retain(|record| record.record_type() != DnsRecordType::AAAA);
    msg.to_vec().context("encode dns response")
}

fn skip_dns_name(msg: &[u8], mut offset: usize) -> Result<usize> {
    loop {
        let len = *msg
            .get(offset)
            .ok_or_else(|| anyhow!("dns name is truncated"))?;
        offset += 1;
        if len & 0b1100_0000 == 0b1100_0000 {
            if offset >= msg.len() {
                return Err(anyhow!("dns compression pointer is truncated"));
            }
            return Ok(offset + 1);
        }
        if len & 0b1100_0000 != 0 {
            return Err(anyhow!("unsupported dns name label type"));
        }
        if len == 0 {
            return Ok(offset);
        }
        offset += len as usize;
        if offset > msg.len() {
            return Err(anyhow!("dns name label is truncated"));
        }
    }
}

fn set_id(resp: &mut [u8], id: u16) {
    if resp.len() >= 2 {
        resp[0..2].copy_from_slice(&id.to_be_bytes());
    }
}

fn dns_message_id(msg: &[u8]) -> Option<u16> {
    (msg.len() >= 2).then(|| u16::from_be_bytes([msg[0], msg[1]]))
}

fn min_ttl_from_values(ttls: &[u32]) -> u32 {
    ttls.iter().copied().min().unwrap_or(0)
}

fn rr_ttls_with_offsets(resp: &[u8]) -> Result<(Vec<u32>, Vec<usize>)> {
    let ttl_offsets = rr_ttl_offsets(resp)?;
    let ttls = ttl_offsets
        .iter()
        .copied()
        .map(|offset| {
            if offset + 4 > resp.len() {
                return Err(anyhow!("dns rr ttl is truncated"));
            }
            Ok(u32::from_be_bytes([
                resp[offset],
                resp[offset + 1],
                resp[offset + 2],
                resp[offset + 3],
            ]))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((ttls, ttl_offsets))
}

#[cfg(test)]
fn rr_ttls(resp: &[u8]) -> Result<Vec<u32>> {
    rr_ttls_with_offsets(resp).map(|(ttls, _)| ttls)
}

fn rewrite_ttls(
    resp: &mut [u8],
    original_ttls: &[u32],
    ttl_offsets: &[usize],
    remaining: u32,
) -> Result<()> {
    if ttl_offsets.len() != original_ttls.len() {
        return Err(anyhow!("dns cached ttl count mismatch"));
    }
    for (offset, original) in ttl_offsets
        .iter()
        .copied()
        .zip(original_ttls.iter().copied())
    {
        if offset + 4 > resp.len() {
            return Err(anyhow!("dns rr ttl is truncated"));
        }
        let ttl = original.min(remaining);
        resp[offset..offset + 4].copy_from_slice(&ttl.to_be_bytes());
    }
    Ok(())
}

fn rr_ttl_offsets(resp: &[u8]) -> Result<Vec<usize>> {
    section_ttl_offsets(resp, false)
}

fn section_ttl_offsets(resp: &[u8], answers_only: bool) -> Result<Vec<usize>> {
    if resp.len() < 12 {
        return Err(anyhow!("dns response is too short"));
    }
    let qdcount = u16::from_be_bytes([resp[4], resp[5]]) as usize;
    let mut counts = [
        u16::from_be_bytes([resp[6], resp[7]]) as usize,
        u16::from_be_bytes([resp[8], resp[9]]) as usize,
        u16::from_be_bytes([resp[10], resp[11]]) as usize,
    ];
    if answers_only {
        counts[1] = 0;
        counts[2] = 0;
    }
    let mut offset = 12;
    for _ in 0..qdcount {
        offset = skip_dns_name(resp, offset)?;
        if offset + 4 > resp.len() {
            return Err(anyhow!("dns question is truncated"));
        }
        offset += 4;
    }

    let mut ttl_offsets = Vec::new();
    for count in counts {
        for _ in 0..count {
            offset = skip_dns_name(resp, offset)?;
            if offset + 10 > resp.len() {
                return Err(anyhow!("dns rr header is truncated"));
            }
            let ttl_offset = offset + 4;
            let rdlen = u16::from_be_bytes([resp[offset + 8], resp[offset + 9]]) as usize;
            offset += 10;
            if offset + rdlen > resp.len() {
                return Err(anyhow!("dns rr data is truncated"));
            }
            offset += rdlen;
            ttl_offsets.push(ttl_offset);
        }
    }
    if offset != resp.len() {
        return Err(anyhow!("dns response contains trailing bytes"));
    }
    Ok(ttl_offsets)
}

fn clamp_ttl(ttl: u32, min: u32, max: u32) -> u32 {
    let ttl = if min > 0 && ttl < min { min } else { ttl };
    if max > 0 && ttl > max {
        max
    } else {
        ttl
    }
}

fn normalize_domain(domain: &str) -> Result<String> {
    let trimmed = domain.trim().trim_end_matches('.');
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    let ascii = idna::domain_to_ascii(trimmed).map_err(|_| anyhow!("invalid IDNA domain"))?;
    Ok(ascii.trim_end_matches('.').to_ascii_lowercase())
}

fn normalize_domain_lossy(domain: &str) -> String {
    normalize_domain(domain)
        .unwrap_or_else(|_| domain.trim().trim_end_matches('.').to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

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
            0x00, 0x00, 0x00, 0x00, 0x03, b'w', b'w', b'w', 0xc0, 0x0b, 0x00, 0x07, b'e', b'x',
            b'a', b'm', b'p', b'l', b'e', 0x03, b'c', b'o', b'm', 0x00,
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
        let hosts = compile_hosts(&["例子.测试=192.0.2.20".to_string()]).expect("compile hosts");
        assert!(hosts.entries.contains_key("xn--fsqu00a.xn--0zwm56d"));

        let upstreams = ["cn"].into_iter().map(ToString::to_string).collect();
        let routes = compile_routes(&["suffix:例子.测试=cn".to_string()], &upstreams)
            .expect("compile routes");
        assert_route(&routes, "www.xn--fsqu00a.xn--0zwm56d", &[], 1, &["cn"]);
        assert_route(
            &routes,
            &normalize_domain_lossy("www.例子.测试"),
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
    fn health_recent_query_success_ignores_probe_success() {
        let health = HealthMonitor::new(true, 3, 2, vec!["upstream".to_string()]);

        health.record_probe_success("upstream", Duration::from_millis(12));
        assert!(!health.recent_query_success("upstream", Duration::from_secs(10)));

        health.record_query_success("upstream", Duration::from_millis(10));
        assert!(health.recent_query_success("upstream", Duration::from_secs(10)));
    }

    #[test]
    fn health_probe_delay_backs_off_after_unhealthy() {
        let health = HealthMonitor::new(true, 2, 1, vec!["upstream".to_string()]);
        let interval = Duration::from_secs(30);

        health.record_probe_failure("upstream", "first failure");
        assert_eq!(health.probe_delay("upstream", interval), interval);

        health.record_probe_failure("upstream", "second failure");
        assert_eq!(
            health.probe_delay("upstream", interval),
            Duration::from_secs(60)
        );

        health.record_probe_failure("upstream", "third failure");
        assert_eq!(
            health.probe_delay("upstream", interval),
            Duration::from_secs(120)
        );

        health.record_probe_success("upstream", Duration::from_millis(8));
        assert_eq!(health.probe_delay("upstream", interval), interval);
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
}

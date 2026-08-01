use super::*;
use crate::config::{parse_upstream_endpoint, CoreUpstreamConfig};
use anyhow::{anyhow, Context, Result};
use hickory_proto::op::Message as DnsMessage;
use parking_lot::Mutex;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use std::collections::HashMap;
use std::error::Error as StdError;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::io::{split, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::{oneshot, watch, Mutex as AsyncMutex};
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{ClientConfig as RustlsClientConfig, RootCertStore};
use tokio_rustls::TlsConnector;
use url::Url;

pub(crate) const BOOTSTRAP_LOOKUP_TIMEOUT: Duration = Duration::from_secs(2);
#[derive(Clone)]
pub(crate) struct UpstreamClient {
    pub(crate) name: String,
    pub(crate) endpoint: Endpoint,
    pub(crate) proxy: Option<ProxyEndpoint>,
    pub(crate) bootstrap: Option<Arc<BootstrapResolver>>,
    pub(crate) http: Option<reqwest::Client>,
    pub(crate) state: Arc<AtomicU8>,
    pub(crate) transport_failure_streak: Arc<AtomicU32>,
    pub(crate) connect_gate: Arc<AsyncMutex<()>>,
    pub(crate) resolve_gate: Arc<AsyncMutex<()>>,
    pub(crate) failure_threshold: u32,
    pub(crate) endpoint_addrs: Arc<Mutex<Option<Arc<[SocketAddr]>>>>,
    pub(crate) udp: Arc<Mutex<Option<Arc<UdpUpstreamClient>>>>,
    pub(crate) socks5_udp: Arc<Mutex<Option<Arc<Socks5UdpUpstreamClient>>>>,
    pub(crate) tcp: Arc<Mutex<Option<Arc<LenPrefixedUpstreamClient>>>>,
    pub(crate) dot: Arc<Mutex<Option<Arc<LenPrefixedUpstreamClient>>>>,
    pub(crate) doq: Arc<Mutex<Option<Arc<DoqUpstreamClient>>>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UpstreamPoolState {
    Ready = 0,
    Cold = 1,
    Connecting = 2,
    Degraded = 3,
    Recovering = 4,
}

impl UpstreamPoolState {
    fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Ready,
            2 => Self::Connecting,
            3 => Self::Degraded,
            4 => Self::Recovering,
            _ => Self::Cold,
        }
    }
}

#[derive(Debug)]
pub(crate) struct UpstreamConnecting;

impl std::fmt::Display for UpstreamConnecting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("upstream connection is already being established")
    }
}

impl StdError for UpstreamConnecting {}

pub(crate) fn is_upstream_connecting_error(err: &anyhow::Error) -> bool {
    err.downcast_ref::<UpstreamConnecting>().is_some()
}

#[derive(Clone)]
pub(crate) struct Endpoint {
    pub(crate) scheme: String,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) address: String,
    pub(crate) url: String,
    pub(crate) server_name: String,
}

#[derive(Clone)]
pub(crate) struct BootstrapResolver {
    servers: Arc<[SocketAddr]>,
}

#[derive(Clone)]
pub(crate) struct ProxyEndpoint {
    address: String,
    username: String,
    password: String,
}

pub(crate) type PendingUdpResponses = Arc<Mutex<HashMap<u16, oneshot::Sender<Vec<u8>>>>>;
pub(crate) type PendingStreamResponses = Arc<Mutex<HashMap<u16, oneshot::Sender<Vec<u8>>>>>;
pub(crate) type BoxedAsyncIo = Box<dyn AsyncIo>;

pub(crate) trait AsyncIo: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T> AsyncIo for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

pub(crate) struct UdpUpstreamClient {
    socket: Arc<UdpSocket>,
    pending: PendingUdpResponses,
    next_id: Mutex<u16>,
    stop_tx: watch::Sender<bool>,
}

pub(crate) struct Socks5UdpUpstreamClient {
    _control: TcpStream,
    socket: Arc<UdpSocket>,
    relay_address: String,
    pending: PendingUdpResponses,
    next_id: Mutex<u16>,
    stop_tx: watch::Sender<bool>,
}

pub(crate) struct LenPrefixedUpstreamClient {
    writer: AsyncMutex<WriteHalf<BoxedAsyncIo>>,
    pending: PendingStreamResponses,
    next_id: Mutex<u16>,
}

pub(crate) struct DoqUpstreamClient {
    _endpoint: quinn::Endpoint,
    connection: quinn::Connection,
}

pub(crate) fn endpoint_from_config(item: &CoreUpstreamConfig) -> Result<Endpoint> {
    let url = parse_upstream_endpoint(&item.endpoint)?;
    endpoint_from_url(&url, &item.server_name)
}

pub(crate) fn endpoint_from_url(url: &Url, server_name: &str) -> Result<Endpoint> {
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

pub(crate) fn format_host_port(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

pub(crate) fn proxy_endpoint_from_raw(raw: &str) -> Result<ProxyEndpoint> {
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

pub(crate) fn build_http_client(
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

pub(crate) fn client_tls_config() -> Arc<RustlsClientConfig> {
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
impl UpstreamClient {
    pub(crate) fn pool_state(&self) -> UpstreamPoolState {
        UpstreamPoolState::from_u8(self.state.load(Ordering::Acquire))
    }

    pub(crate) fn store_pool_state(&self, state: UpstreamPoolState) {
        self.state.store(state as u8, Ordering::Release);
    }

    pub(crate) fn should_skip_for_query(&self) -> bool {
        self.pool_state() == UpstreamPoolState::Connecting
    }

    pub(crate) fn mark_ready(&self) {
        self.transport_failure_streak.store(0, Ordering::Release);
        self.store_pool_state(UpstreamPoolState::Ready);
    }

    pub(crate) fn mark_transport_failure(&self) {
        *self.endpoint_addrs.lock() = None;
        *self.udp.lock() = None;
        *self.socks5_udp.lock() = None;
        *self.tcp.lock() = None;
        *self.dot.lock() = None;
        *self.doq.lock() = None;
        let failures = self.transport_failure_streak.fetch_add(1, Ordering::AcqRel) + 1;
        if failures >= self.failure_threshold {
            self.store_pool_state(UpstreamPoolState::Degraded);
        } else {
            self.store_pool_state(UpstreamPoolState::Cold);
        }
    }

    pub(crate) fn begin_connect(&self) -> Result<tokio::sync::MutexGuard<'_, ()>> {
        let state = if self.pool_state() == UpstreamPoolState::Degraded {
            UpstreamPoolState::Recovering
        } else {
            UpstreamPoolState::Connecting
        };
        let Ok(guard) = self.connect_gate.try_lock() else {
            self.store_pool_state(state);
            return Err(anyhow!(UpstreamConnecting));
        };
        self.store_pool_state(state);
        Ok(guard)
    }

    pub(crate) async fn exchange(&self, req: &[u8], timeout: Option<Duration>) -> Result<Vec<u8>> {
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
        let result = if let Some(timeout) = timeout {
            tokio::time::timeout(timeout, fut)
                .await
                .context("upstream timeout")
                .and_then(|result| result)
        } else {
            fut.await
        };
        result.inspect(|_| self.mark_ready()).inspect_err(|err| {
            if !is_upstream_connecting_error(err) {
                self.mark_transport_failure();
            }
        })
    }

    pub(crate) async fn exchange_udp(&self, req: &[u8]) -> Result<Vec<u8>> {
        let client = self.udp_client().await?;
        let target = self.first_endpoint_addr().await?;
        client.exchange(target, req).await
    }

    pub(crate) async fn udp_client(&self) -> Result<Arc<UdpUpstreamClient>> {
        if let Some(client) = self.udp.lock().clone() {
            return Ok(client);
        }
        let _connect_guard = self.begin_connect()?;
        if let Some(client) = self.udp.lock().clone() {
            self.mark_ready();
            return Ok(client);
        }
        let client = UdpUpstreamClient::new().await?;
        let mut udp = self.udp.lock();
        if let Some(existing) = udp.as_ref() {
            return Ok(existing.clone());
        }
        *udp = Some(client.clone());
        self.mark_ready();
        Ok(client)
    }

    pub(crate) async fn exchange_socks5_udp(&self, req: &[u8]) -> Result<Vec<u8>> {
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

    pub(crate) async fn socks5_udp_client(&self) -> Result<Arc<Socks5UdpUpstreamClient>> {
        if let Some(client) = self.socks5_udp.lock().clone() {
            return Ok(client);
        }
        let _connect_guard = self.begin_connect()?;
        if let Some(client) = self.socks5_udp.lock().clone() {
            self.mark_ready();
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
        self.mark_ready();
        Ok(client)
    }

    pub(crate) async fn exchange_tcp(&self, req: &[u8]) -> Result<Vec<u8>> {
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

    pub(crate) async fn tcp_client(&self) -> Result<Arc<LenPrefixedUpstreamClient>> {
        if let Some(client) = self.tcp.lock().clone() {
            return Ok(client);
        }
        let _connect_guard = self.begin_connect()?;
        if let Some(client) = self.tcp.lock().clone() {
            self.mark_ready();
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
        self.mark_ready();
        Ok(client)
    }

    pub(crate) async fn exchange_dot(&self, req: &[u8]) -> Result<Vec<u8>> {
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

    pub(crate) async fn dot_client(&self) -> Result<Arc<LenPrefixedUpstreamClient>> {
        if let Some(client) = self.dot.lock().clone() {
            return Ok(client);
        }
        let _connect_guard = self.begin_connect()?;
        if let Some(client) = self.dot.lock().clone() {
            self.mark_ready();
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
        self.mark_ready();
        Ok(client)
    }

    pub(crate) async fn exchange_doq(&self, req: &[u8]) -> Result<Vec<u8>> {
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

    pub(crate) async fn doq_client(&self) -> Result<Arc<DoqUpstreamClient>> {
        if let Some(client) = self.doq.lock().clone() {
            return Ok(client);
        }
        let _connect_guard = self.begin_connect()?;
        if let Some(client) = self.doq.lock().clone() {
            self.mark_ready();
            return Ok(client);
        }
        let client = DoqUpstreamClient::connect(&self.endpoint, self.bootstrap.as_deref()).await?;
        let mut doq = self.doq.lock();
        if let Some(existing) = doq.as_ref() {
            return Ok(existing.clone());
        }
        *doq = Some(client.clone());
        self.mark_ready();
        Ok(client)
    }

    pub(crate) async fn connect_direct(&self) -> Result<TcpStream> {
        let addrs = self.endpoint_addrs().await?;
        connect_socket_addrs(&addrs, &self.endpoint.address).await
    }

    pub(crate) async fn first_endpoint_addr(&self) -> Result<SocketAddr> {
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

    pub(crate) async fn endpoint_addrs(&self) -> Result<Vec<SocketAddr>> {
        if let Some(addrs) = self.endpoint_addrs.lock().clone() {
            return Ok(addrs.to_vec());
        }
        let Ok(_resolve_guard) = self.resolve_gate.try_lock() else {
            self.store_pool_state(if self.pool_state() == UpstreamPoolState::Degraded {
                UpstreamPoolState::Recovering
            } else {
                UpstreamPoolState::Connecting
            });
            return Err(anyhow!(UpstreamConnecting));
        };
        if let Some(addrs) = self.endpoint_addrs.lock().clone() {
            return Ok(addrs.to_vec());
        }
        let addrs = self.resolve_endpoint_addrs().await?;
        if !addrs.is_empty() {
            *self.endpoint_addrs.lock() = Some(addrs.clone().into());
        }
        Ok(addrs)
    }

    pub(crate) async fn resolve_endpoint_addrs(&self) -> Result<Vec<SocketAddr>> {
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

pub(crate) async fn connect_socket_addrs(addrs: &[SocketAddr], label: &str) -> Result<TcpStream> {
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
    pub(crate) fn new(servers: Vec<SocketAddr>) -> Self {
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

pub(crate) async fn bootstrap_lookup(
    server: &SocketAddr,
    domain: &str,
    qtype: u16,
) -> Result<Vec<IpAddr>> {
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

pub(crate) fn bootstrap_response_ips(resp: &[u8], qtype: u16) -> Result<Vec<IpAddr>> {
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
    pub(crate) fn new(stream: BoxedAsyncIo) -> Arc<Self> {
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

pub(crate) struct PendingUdpResponseGuard {
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

pub(crate) async fn run_udp_upstream_recv_loop(
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

pub(crate) async fn run_socks5_udp_recv_loop(
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

pub(crate) async fn run_len_prefixed_recv_loop(
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

pub(crate) async fn socks5_connect(proxy: &ProxyEndpoint, target: &str) -> Result<TcpStream> {
    let mut stream = TcpStream::connect(&proxy.address)
        .await
        .with_context(|| format!("connect SOCKS5 proxy {}", proxy.address))?;

    socks5_negotiate(&mut stream, proxy).await?;
    socks5_send_request(&mut stream, 0x01, target).await?;
    Ok(stream)
}

pub(crate) async fn socks5_udp_associate(proxy: &ProxyEndpoint) -> Result<(TcpStream, String)> {
    let mut stream = TcpStream::connect(&proxy.address)
        .await
        .with_context(|| format!("connect SOCKS5 proxy {}", proxy.address))?;

    socks5_negotiate(&mut stream, proxy).await?;
    let relay = socks5_send_request(&mut stream, 0x03, "0.0.0.0:0").await?;
    Ok((stream, relay))
}

pub(crate) async fn socks5_negotiate(stream: &mut TcpStream, proxy: &ProxyEndpoint) -> Result<()> {
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

pub(crate) async fn socks5_send_request(
    stream: &mut TcpStream,
    command: u8,
    target: &str,
) -> Result<String> {
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

pub(crate) async fn read_socks5_address(stream: &mut TcpStream, atyp: u8) -> Result<String> {
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

pub(crate) async fn write_socks5_auth(
    stream: &mut TcpStream,
    username: &str,
    password: &str,
) -> Result<()> {
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

pub(crate) fn split_host_port(address: &str) -> Result<(&str, u16)> {
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

pub(crate) fn socks5_udp_packet(target: &str, payload: &[u8]) -> Result<Vec<u8>> {
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

pub(crate) fn parse_socks5_udp_payload(packet: &[u8]) -> Result<Vec<u8>> {
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

pub(crate) fn doq_client_config() -> Result<quinn::ClientConfig> {
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

pub(crate) fn len_prefixed_dns_message(req: &[u8]) -> Result<Vec<u8>> {
    if req.len() > u16::MAX as usize {
        return Err(anyhow!("DNS message is too large"));
    }
    let mut out = Vec::with_capacity(req.len() + 2);
    out.extend_from_slice(&(req.len() as u16).to_be_bytes());
    out.extend_from_slice(req);
    Ok(out)
}

pub(crate) fn parse_len_prefixed_dns_message(raw: &[u8]) -> Result<Vec<u8>> {
    if raw.len() < 2 {
        return Err(anyhow!("length-prefixed DNS response is too short"));
    }
    let len = u16::from_be_bytes([raw[0], raw[1]]) as usize;
    if raw.len() < len + 2 {
        return Err(anyhow!("length-prefixed DNS response is truncated"));
    }
    Ok(raw[2..2 + len].to_vec())
}

pub(crate) async fn exchange_doh(
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

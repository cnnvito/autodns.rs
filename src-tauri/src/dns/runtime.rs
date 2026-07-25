use super::*;
use crate::config::{parse_go_duration, CoreConfig, CoreServerConfig};
use crate::history::DnsHistoryRecorder;
use crate::logging::LogBuffer;
use anyhow::{anyhow, Context, Result};
use arc_swap::ArcSwap;
use base64::Engine;
use parking_lot::Mutex;
use std::fs::File;
use std::io::ErrorKind;
use std::io::{BufReader, Cursor};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::{watch, OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinHandle;
use tokio_rustls::rustls::ServerConfig as RustlsServerConfig;
use tokio_rustls::TlsAcceptor;

pub(crate) const MAX_CONCURRENT_REQUESTS: usize = 256;
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

pub(crate) struct RuntimeState {
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
                .map_err(|err| listener_bind_error("UDP", &cfg.server.listen, err))?;
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
                .map_err(|err| listener_bind_error("TCP", &cfg.server.listen, err))?;
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
            let tls = load_server_tls(&cfg.server)?;
            let listener = TcpListener::bind(&cfg.server.listen)
                .await
                .map_err(|err| listener_bind_error("DoT", &cfg.server.listen, err))?;
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
            let tls = load_server_tls(&cfg.server)?;
            let listener = TcpListener::bind(&cfg.server.listen)
                .await
                .map_err(|err| listener_bind_error("DoH", &cfg.server.listen, err))?;
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

pub(crate) fn listener_bind_error(
    protocol: &str,
    listen: &str,
    err: std::io::Error,
) -> anyhow::Error {
    match err.kind() {
        ErrorKind::AddrInUse => anyhow!("{protocol} listen address {listen} is already in use"),
        ErrorKind::PermissionDenied => {
            anyhow!("{protocol} listen address {listen} permission denied")
        }
        _ => anyhow!("{protocol} listen address {listen} bind failed: {err}"),
    }
}

pub(crate) fn spawn_health_tasks(
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

pub(crate) fn same_server_identity(a: &CoreConfig, b: &CoreConfig) -> bool {
    a.server.mode == b.server.mode
        && a.server.listen == b.server.listen
        && a.server.tls_source == b.server.tls_source
        && a.server.cert_file == b.server.cert_file
        && a.server.key_file == b.server.key_file
        && a.server.cert_pem == b.server.cert_pem
        && a.server.key_pem == b.server.key_pem
        && (a.server.mode != "doh" || a.server.path == b.server.path)
}

pub fn validate_server_tls_config(server: &CoreServerConfig) -> Result<()> {
    load_server_tls(server).map(|_| ())
}

pub(crate) fn load_server_tls(server: &CoreServerConfig) -> Result<Arc<RustlsServerConfig>> {
    let (certs, key) = match server.tls_source.as_str() {
        "file" | "" => {
            let cert_file = &server.cert_file;
            let key_file = &server.key_file;
            let certs = {
                let file = File::open(cert_file)
                    .with_context(|| format!("open certificate file: {cert_file}"))?;
                rustls_pemfile::certs(&mut BufReader::new(file))
                    .collect::<std::result::Result<Vec<_>, _>>()?
            };
            let key = {
                let file = File::open(key_file)
                    .with_context(|| format!("open private key file: {key_file}"))?;
                rustls_pemfile::private_key(&mut BufReader::new(file))?
                    .ok_or_else(|| anyhow!("private key file does not contain a supported key"))?
            };
            (certs, key)
        }
        "inline" => {
            let certs = rustls_pemfile::certs(&mut Cursor::new(server.cert_pem.as_bytes()))
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let key = rustls_pemfile::private_key(&mut Cursor::new(server.key_pem.as_bytes()))?
                .ok_or_else(|| anyhow!("private key PEM does not contain a supported key"))?;
            (certs, key)
        }
        _ => return Err(anyhow!("server.tls_source must be one of file,inline")),
    };
    if certs.is_empty() {
        return Err(anyhow!("certificate PEM does not contain a certificate"));
    }
    Ok(Arc::new(
        RustlsServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .context("build server tls config")?,
    ))
}
pub(crate) async fn run_udp_listener(
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

pub(crate) async fn run_tcp_listener(
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

pub(crate) async fn handle_tcp_client(
    mut stream: TcpStream,
    resolver: Arc<Resolver>,
) -> Result<()> {
    handle_dns_stream(&mut stream, resolver).await
}

pub(crate) async fn run_dot_listener(
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

pub(crate) async fn run_doh_listener(
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

pub(crate) async fn wait_for_request_permit(
    request_limit: &Arc<Semaphore>,
    stop: &mut watch::Receiver<bool>,
) -> Option<OwnedSemaphorePermit> {
    tokio::select! {
        _ = stop.changed() => None,
        permit = request_limit.clone().acquire_owned() => permit.ok(),
    }
}

pub(crate) async fn handle_dns_stream<S>(stream: &mut S, resolver: Arc<Resolver>) -> Result<()>
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

pub(crate) async fn handle_doh_client<S>(
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

pub(crate) fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

pub(crate) fn doh_get_payload(target: &str, expected_path: &str) -> Option<Vec<u8>> {
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

pub(crate) async fn write_doh_response<S>(stream: &mut S, body: &[u8]) -> Result<()>
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

pub(crate) async fn write_http_response<S>(
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

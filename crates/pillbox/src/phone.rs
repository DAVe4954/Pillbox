//! M6: the phone server. A small HTTP server (axum on a one-thread tokio runtime) that lives
//! only while a QR code is on screen. It listens on one private LAN address and a random port,
//! and answers only requests that
//! - come from the same subnet,
//! - name exactly that address and port in the Host header (no DNS rebinding), and
//! - carry the 128-bit random token from the QR code as the first path part.
//!
//! Everything else gets the same plain 404. After 20 wrong tokens the app closes the code.
//! Dropping [`PhoneServer`] stops it: no new connections are accepted, transfers already
//! running may finish (cut off after 10 minutes), then the thread ends and the port closes.

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    task::{Context as TaskContext, Poll},
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use axum::{
    Router,
    body::{Body, Bytes},
    extract::{ConnectInfo, DefaultBodyLimit, Multipart, Request, State},
    http::{HeaderValue, StatusCode, Uri, header},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use pillbox_core::phone::{self as logic, Adapter};
use tokio::{io::AsyncWriteExt, sync::watch};
use tokio_util::io::ReaderStream;

use crate::platform::{net, window::Waker};

/// Wrong tokens tolerated before the app closes the code.
const MAX_BAD_TOKENS: u32 = 20;
/// After the code closes, transfers still running get this long to finish.
const DRAIN: Duration = Duration::from_secs(600);
/// Longest text accepted from the phone.
const MAX_TEXT: usize = 1 << 20;

/// What the server offers the phone.
#[derive(Debug, Clone)]
pub enum Offer {
    /// One file from the shelf, to download.
    File {
        path: PathBuf,
        name: String,
        size: u64,
    },
    /// Photos, files and text from the phone, saved in `dir`.
    Receive { dir: PathBuf, max_bytes: u64 },
}

/// What happened, for Pillbox to show.
#[derive(Debug, Clone, PartialEq)]
pub enum PhoneEvent {
    /// The page was loaded (the code was scanned). `remote`: from another device, not this PC
    /// (local connections never meet the firewall, so they prove nothing about it).
    Opened { remote: bool },
    /// The phone finished downloading the file.
    Downloaded,
    /// An upload started.
    Receiving,
    /// Files saved from the phone, in order.
    Received(Vec<PathBuf>),
    /// Text from the phone.
    Text(String),
    /// An upload failed part-way.
    Failed(String),
    /// Too many wrong tokens: someone is guessing.
    Guessing,
}

struct Shared {
    token: String,
    nonce: String,
    addr: Ipv4Addr,
    port: u16,
    prefix: u8,
    offer: Offer,
    events: Sender<PhoneEvent>,
    waker: Waker,
    bad: AtomicU32,
}

impl Shared {
    fn send(&self, e: PhoneEvent) {
        let _ = self.events.send(e);
        self.waker.wake();
    }
}

pub struct PhoneServer {
    /// The link in the QR code. Contains the token: never log it.
    pub url: String,
    pub addr: SocketAddr,
    events: Receiver<PhoneEvent>,
    /// Dropping this stops the server.
    _stop: watch::Sender<bool>,
}

impl PhoneServer {
    pub fn start(adapter: &Adapter, offer: Offer, waker: Waker) -> Result<Self> {
        let token = logic::hex(&net::random_bytes::<16>()?);
        let nonce = logic::hex(&net::random_bytes::<12>()?);
        let listener = bind_random_port(adapter.addr)?;
        let addr = listener
            .local_addr()
            .context("reading the server address")?;
        listener
            .set_nonblocking(true)
            .context("making the socket non-blocking")?;
        let (events_tx, events) = mpsc::channel();
        let (stop, stopped) = watch::channel(false);
        let url = format!("http://{}:{}/{token}/", adapter.addr, addr.port());
        let shared = Arc::new(Shared {
            token,
            nonce,
            addr: adapter.addr,
            port: addr.port(),
            prefix: adapter.prefix,
            offer,
            events: events_tx,
            waker,
            bad: AtomicU32::new(0),
        });
        std::thread::Builder::new()
            .name("phone-server".into())
            .spawn(move || {
                if let Err(e) = run(listener, shared, stopped) {
                    tracing::warn!("phone server: {e:#}");
                }
                tracing::info!("phone server stopped");
            })
            .context("starting the phone server thread")?;
        tracing::info!("phone server listening on {addr} ({})", adapter.name);
        Ok(Self {
            url,
            addr,
            events,
            _stop: stop,
        })
    }

    pub fn poll(&self) -> Vec<PhoneEvent> {
        self.events.try_iter().collect()
    }
}

/// A port picked at random from the dynamic range (not the OS's next free one), so the
/// address is not guessable either.
fn bind_random_port(ip: Ipv4Addr) -> Result<TcpListener> {
    for _ in 0..16 {
        let r = net::random_bytes::<2>()?;
        let port = 49152 + u16::from_le_bytes(r) % (65535 - 49152);
        if let Ok(l) = TcpListener::bind((ip, port)) {
            return Ok(l);
        }
    }
    TcpListener::bind((ip, 0)).with_context(|| format!("listening on {ip}"))
}

fn run(listener: TcpListener, shared: Arc<Shared>, stopped: watch::Receiver<bool>) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .context("starting the tokio runtime")?;
    rt.block_on(async move {
        let listener =
            tokio::net::TcpListener::from_std(listener).context("adopting the socket")?;
        let limit = match &shared.offer {
            Offer::Receive { max_bytes, .. } => {
                usize::try_from(*max_bytes).unwrap_or(usize::MAX).saturating_add(MAX_TEXT * 2)
            }
            Offer::File { .. } => 64 * 1024,
        };
        let app = Router::new()
            .route("/{token}", get(redirect_to_slash))
            .route("/{token}/", get(page))
            .route("/{token}/file", get(download))
            .route("/{token}/upload", post(upload))
            .layer(DefaultBodyLimit::max(limit))
            .layer(middleware::from_fn_with_state(shared.clone(), guard))
            .with_state(shared);
        let (mut graceful, mut hard) = (stopped.clone(), stopped);
        let serve = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            // A dropped sender counts as "stop" too.
            let _ = graceful.wait_for(|stop| *stop).await;
        });
        tokio::select! {
            r = serve => r.context("serving"),
            _ = async move {
                let _ = hard.wait_for(|stop| *stop).await;
                tokio::time::sleep(DRAIN).await;
            } => Err(anyhow!("transfers still running {} s after the code closed; cut off", DRAIN.as_secs())),
        }
    })
}

/// Runs before every request: subnet, Host and token checks, then safe headers.
async fn guard(
    State(s): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    let from_subnet = match peer.ip() {
        IpAddr::V4(ip) => logic::same_subnet(ip, s.addr, s.prefix),
        IpAddr::V6(_) => false,
    };
    let host_ok = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| logic::host_ok(h, s.addr, s.port));
    let first = req
        .uri()
        .path()
        .trim_start_matches('/')
        .split('/')
        .next()
        .unwrap_or("");
    let token_ok = logic::token_ok(&s.token, first);
    let guessing = s.bad.load(Ordering::Relaxed) >= MAX_BAD_TOKENS;
    if !(from_subnet && host_ok && token_ok) || guessing {
        // Only things shaped like a token count as guesses (not /favicon.ico and the like).
        if from_subnet && !token_ok && first.len() == s.token.len() {
            let n = s.bad.fetch_add(1, Ordering::Relaxed) + 1;
            if n == MAX_BAD_TOKENS {
                s.send(PhoneEvent::Guessing);
            }
        }
        tracing::debug!(
            "phone server: refused a request from {peer} (subnet {from_subnet}, host {host_ok}, token {token_ok})"
        );
        return with_headers(StatusCode::NOT_FOUND.into_response(), &s.nonce);
    }
    with_headers(next.run(req).await, &s.nonce)
}

fn with_headers(mut r: Response, nonce: &str) -> Response {
    let h = r.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    if let Ok(v) = HeaderValue::from_str(&logic::csp(nonce)) {
        h.insert(header::CONTENT_SECURITY_POLICY, v);
    }
    r
}

async fn redirect_to_slash(uri: Uri) -> Redirect {
    // The token was checked by the guard; the path is ours.
    Redirect::to(&format!("{}/", uri.path()))
}

async fn page(
    State(s): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> Response {
    s.send(PhoneEvent::Opened {
        remote: peer.ip() != IpAddr::V4(s.addr),
    });
    let html = match &s.offer {
        Offer::File { name, size, .. } => logic::send_page(name, *size, &s.nonce),
        Offer::Receive { max_bytes, .. } => logic::receive_page(&s.nonce, *max_bytes),
    };
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response()
}

async fn download(State(s): State<Arc<Shared>>, uri: Uri) -> Response {
    let Offer::File { path, name, .. } = &s.offer else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let inline = uri.query() == Some("inline=1");
    let file = match tokio::fs::File::open(path).await {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("phone download: {e}");
            return (StatusCode::GONE, "The file is no longer on the PC.").into_response();
        }
    };
    let len = file.metadata().await.map(|m| m.len()).ok();
    let done: Option<Box<dyn FnOnce() + Send>> = if inline {
        None
    } else {
        let s = s.clone();
        Some(Box::new(move || s.send(PhoneEvent::Downloaded)))
    };
    let body = Body::from_stream(Tracked {
        inner: ReaderStream::with_capacity(file, 64 * 1024),
        left: len.unwrap_or(u64::MAX),
        done,
    });
    let mut r = Response::new(body);
    let h = r.headers_mut();
    if let Ok(v) = HeaderValue::from_str(logic::content_type(name)) {
        h.insert(header::CONTENT_TYPE, v);
    }
    if let Ok(v) = HeaderValue::from_str(&logic::content_disposition(name, inline)) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    if let Some(len) = len {
        h.insert(header::CONTENT_LENGTH, HeaderValue::from(len));
    }
    r
}

/// A file stream that calls `done` once the last byte has been handed to the connection.
/// With a Content-Length the connection stops polling as soon as that many bytes went out
/// and never asks for the end of the stream, so the bytes are counted.
struct Tracked {
    inner: ReaderStream<tokio::fs::File>,
    left: u64,
    done: Option<Box<dyn FnOnce() + Send>>,
}

impl futures_core::Stream for Tracked {
    type Item = std::io::Result<Bytes>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        let r = Pin::new(&mut this.inner).poll_next(cx);
        let finished = match &r {
            Poll::Ready(Some(Ok(chunk))) => {
                this.left = this.left.saturating_sub(chunk.len() as u64);
                this.left == 0
            }
            Poll::Ready(None) => true,
            _ => false,
        };
        if finished && let Some(done) = this.done.take() {
            done();
        }
        r
    }
}

async fn upload(
    State(s): State<Arc<Shared>>,
    headers: axum::http::HeaderMap,
    mut form: Multipart,
) -> Response {
    let Offer::Receive { dir, max_bytes } = &s.offer else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // Refuse a declared size that cannot fit before reading anything (the page checks too).
    let declared = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    if declared.is_some_and(|n| n > max_bytes.saturating_add(MAX_TEXT as u64 * 2)) {
        let msg = format!("Too big: the limit is {}.", logic::human_size(*max_bytes));
        s.send(PhoneEvent::Failed(msg.clone()));
        return (StatusCode::PAYLOAD_TOO_LARGE, msg).into_response();
    }
    let mut saved: Vec<PathBuf> = Vec::new();
    let mut text: Option<String> = None;
    let mut total: u64 = 0;
    let outcome: Result<(), (StatusCode, String)> = async {
        while let Some(mut field) = form.next_field().await.map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                format!("The upload broke off ({e})."),
            )
        })? {
            match field.name() {
                Some("text") => {
                    let t = field.text().await.map_err(|_| {
                        (StatusCode::BAD_REQUEST, "The text did not arrive.".into())
                    })?;
                    if t.len() > MAX_TEXT {
                        return Err((
                            StatusCode::PAYLOAD_TOO_LARGE,
                            "The text is too long.".into(),
                        ));
                    }
                    if !t.trim().is_empty() {
                        text = Some(t);
                    }
                }
                Some("files") => {
                    // An empty file input still sends a part, with no file name.
                    let raw = field.file_name().unwrap_or("").to_string();
                    if raw.is_empty() {
                        continue;
                    }
                    if saved.is_empty() {
                        s.send(PhoneEvent::Receiving);
                    }
                    let path = save_field(&mut field, dir, &raw, *max_bytes, &mut total).await?;
                    saved.push(path);
                }
                _ => {}
            }
        }
        Ok(())
    }
    .await;
    let (files, got_text) = (saved.len(), text.is_some());
    if !saved.is_empty() {
        s.send(PhoneEvent::Received(saved));
    }
    if let Some(t) = text {
        s.send(PhoneEvent::Text(t));
    }
    match outcome {
        Ok(()) if files == 0 && !got_text => {
            (StatusCode::BAD_REQUEST, "Nothing to send.").into_response()
        }
        Ok(()) => logic::upload_summary(files, got_text).into_response(),
        Err((code, msg)) => {
            s.send(PhoneEvent::Failed(msg.clone()));
            (code, msg).into_response()
        }
    }
}

/// Streams one uploaded file to `<dir>\<name>.part`, then renames it to a free name.
async fn save_field(
    field: &mut axum::extract::multipart::Field<'_>,
    dir: &Path,
    raw_name: &str,
    max_bytes: u64,
    total: &mut u64,
) -> Result<PathBuf, (StatusCode, String)> {
    let io = |e: std::io::Error| {
        tracing::warn!("phone upload: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "The PC could not save the file.".to_string(),
        )
    };
    tokio::fs::create_dir_all(dir).await.map_err(io)?;
    let name = logic::safe_file_name(raw_name);
    let part_name = logic::unique_name(&format!("{name}.part"), |n| dir.join(n).exists());
    let part = dir.join(&part_name);
    let mut file = tokio::fs::File::create(&part).await.map_err(io)?;
    let result: Result<(), (StatusCode, String)> = async {
        loop {
            let chunk = field.chunk().await.map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("The upload broke off ({e})."),
                )
            })?;
            let Some(chunk) = chunk else { break };
            *total += chunk.len() as u64;
            if *total > max_bytes {
                return Err((
                    StatusCode::PAYLOAD_TOO_LARGE,
                    format!("Too big: the limit is {}.", logic::human_size(max_bytes)),
                ));
            }
            file.write_all(&chunk).await.map_err(io)?;
        }
        file.flush().await.map_err(io)?;
        Ok(())
    }
    .await;
    drop(file);
    if let Err(e) = result {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(e);
    }
    let final_name = logic::unique_name(&name, |n| dir.join(n).exists());
    let path = dir.join(final_name);
    if let Err(e) = tokio::fs::rename(&part, &path).await {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(io(e));
    }
    tracing::info!("phone upload saved: {}", path.display());
    Ok(path)
}

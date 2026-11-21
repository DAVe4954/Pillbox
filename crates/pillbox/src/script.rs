//! M7: the local script API. A small HTTP server (axum on a one-thread tokio runtime, the same
//! stack as the phone server) that listens on `127.0.0.1` only, on a random port, and answers
//! only requests that
//! - name exactly `127.0.0.1:<port>` in the Host header (no DNS rebinding),
//! - carry no `Origin` header (a web page in a browser always sends one on a cross-site POST;
//!   `pillboxctl` and hooks never do), and
//! - carry the 128-bit random token as `Authorization: Bearer <token>`.
//!
//! Everything else gets the same plain 404. Port and token are written to `data\api.json`
//! (a sub-folder, so the settings watcher is not woken) while the server runs, and the file is
//! removed when it stops. The folder is under the user's profile, so other Windows accounts
//! cannot read the token. Anything running as the same user can, as with any local secret.

use std::{
    net::{Ipv4Addr, TcpListener},
    path::{Path, PathBuf},
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use pillbox_core::{
    phone as logic,
    script::{self, AgentUpdate, Command, MAX_BODY},
};
use serde_json::Value;
use tokio::sync::{oneshot, watch};

use crate::platform::{net, window::Waker};

/// How much of the end of a transcript is read to find the model and effort.
const TRANSCRIPT_TAIL: u64 = 192 * 1024;
/// Longest a permission question may wait for a click, whatever the client asks for.
const MAX_WAIT_SECS: u64 = 600;
/// After a stop, requests still waiting get this long.
const DRAIN: Duration = Duration::from_secs(2);

/// What the server hands to the app.
pub enum Inbound {
    Cmd(Command),
    /// A permission question: answer `true` (allow) or `false` (deny) through the sender, or
    /// drop it to let the agent ask in its own window.
    Permission(Command, oneshot::Sender<bool>),
}

struct Shared {
    token: String,
    host: String,
    events: Sender<Inbound>,
    waker: Waker,
}

impl Shared {
    fn send(&self, i: Inbound) {
        let _ = self.events.send(i);
        self.waker.wake();
    }
}

pub struct ScriptServer {
    events: Receiver<Inbound>,
    info_path: PathBuf,
    /// Dropping this stops the server.
    _stop: watch::Sender<bool>,
}

impl ScriptServer {
    pub fn start(data_dir: &Path, waker: Waker) -> Result<Self> {
        let token = logic::hex(&net::random_bytes::<16>()?);
        let listener = bind_random_port()?;
        let addr = listener
            .local_addr()
            .context("reading the server address")?;
        listener
            .set_nonblocking(true)
            .context("making the socket non-blocking")?;
        let (events_tx, events) = mpsc::channel();
        let (stop, stopped) = watch::channel(false);
        let shared = Arc::new(Shared {
            token: token.clone(),
            host: format!("127.0.0.1:{}", addr.port()),
            events: events_tx,
            waker,
        });
        let info_path = data_dir.join("api.json");
        write_info(&info_path, addr.port(), &token)?;
        let spawned = std::thread::Builder::new()
            .name("script-server".into())
            .spawn(move || {
                if let Err(e) = run(listener, shared, stopped) {
                    tracing::warn!("script server: {e:#}");
                }
                tracing::info!("script server stopped");
            });
        if let Err(e) = spawned {
            let _ = std::fs::remove_file(&info_path);
            return Err(e).context("starting the script server thread");
        }
        tracing::info!("script server listening on {addr}");
        Ok(Self {
            events,
            info_path,
            _stop: stop,
        })
    }

    pub fn poll(&self) -> Vec<Inbound> {
        self.events.try_iter().collect()
    }
}

impl Drop for ScriptServer {
    fn drop(&mut self) {
        // The token dies with the server; a stale file would send clients to a dead port.
        let _ = std::fs::remove_file(&self.info_path);
    }
}

/// `{"port":..,"token":..,"pid":..,"version":1}`, replaced atomically.
fn write_info(path: &Path, port: u16, token: &str) -> Result<()> {
    let json = serde_json::json!({
        "version": 1,
        "port": port,
        "token": token,
        "pid": std::process::id(),
    });
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, json.to_string()).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

fn bind_random_port() -> Result<TcpListener> {
    let ip = Ipv4Addr::LOCALHOST;
    for _ in 0..16 {
        let r = net::random_bytes::<2>()?;
        let port = 49152 + u16::from_le_bytes(r) % (65535 - 49152);
        if let Ok(l) = TcpListener::bind((ip, port)) {
            return Ok(l);
        }
    }
    TcpListener::bind((ip, 0)).context("listening on 127.0.0.1")
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
        let app = Router::new().fallback(handle).with_state(shared);
        let (mut graceful, mut hard) = (stopped.clone(), stopped);
        let serve = axum::serve(listener, app).with_graceful_shutdown(async move {
            let _ = graceful.wait_for(|stop| *stop).await;
        });
        tokio::select! {
            r = serve => r.context("serving"),
            _ = async move {
                let _ = hard.wait_for(|stop| *stop).await;
                tokio::time::sleep(DRAIN).await;
            } => Ok(()),
        }
    })
}

fn plain(status: StatusCode, text: &'static str) -> Response {
    let mut r = (status, text).into_response();
    r.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0))
}

/// Host, Origin and token checks (see the module comment). Nothing is said about which failed.
fn authorised(s: &Shared, req: &Request) -> bool {
    let h = req.headers();
    let host_ok = h
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case(&s.host));
    let no_origin = !h.contains_key(header::ORIGIN);
    let given = h
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    // Evaluate all three so the time taken does not say which one failed.
    let token_ok = logic::token_ok(&s.token, given);
    host_ok & no_origin & token_ok
}

/// The `wait` seconds from `?wait=N`, if present.
fn wait_secs(query: Option<&str>) -> Option<u64> {
    query?
        .split('&')
        .find_map(|kv| kv.strip_prefix("wait="))
        .and_then(|n| n.parse::<u64>().ok())
        .map(|n| n.min(MAX_WAIT_SECS))
}

async fn handle(State(s): State<Arc<Shared>>, req: Request) -> Response {
    if !authorised(&s, &req) {
        tracing::debug!("script server: refused a request");
        return plain(StatusCode::NOT_FOUND, "");
    }
    let (parts, body) = req.into_parts();
    let Some(path) = parts.uri.path().strip_prefix("/v1/") else {
        return plain(StatusCode::NOT_FOUND, "");
    };
    if parts.method == Method::GET && path == "ping" {
        return plain(StatusCode::OK, "{\"ok\":true,\"version\":1}");
    }
    if parts.method != Method::POST {
        return plain(StatusCode::METHOD_NOT_ALLOWED, "");
    }
    let bytes = match to_bytes(body, MAX_BODY).await {
        Ok(b) => b,
        Err(_) => return plain(StatusCode::PAYLOAD_TOO_LARGE, "too large"),
    };
    let mut cmd = match script::parse_request(path, &bytes, now_ms()) {
        Ok(c) => c,
        Err(e) => {
            // A hook event Pillbox has nothing to show for is not an error to the agent.
            tracing::debug!("script server: {path}: {e}");
            return if path.starts_with("hook/") {
                plain(StatusCode::NO_CONTENT, "")
            } else {
                let mut r = plain(StatusCode::BAD_REQUEST, "");
                *r.body_mut() = Body::from(serde_json::json!({ "error": e }).to_string());
                r
            };
        }
    };
    if let Command::Agent(u) = &mut cmd {
        if let Some(pid) = parts
            .headers
            .get("x-pillbox-pid")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u32>().ok())
        {
            u.pid = Some(pid);
        }
        if path.starts_with("hook/")
            && let Ok(v) = serde_json::from_slice::<Value>(&bytes)
        {
            enrich(u, &v);
        }
    }
    let wants_answer =
        path.starts_with("hook/") && matches!(&cmd, Command::Agent(u) if u.ask.is_some());
    if let (true, Some(secs)) = (wants_answer, wait_secs(parts.uri.query())) {
        let (tx, rx) = oneshot::channel();
        s.send(Inbound::Permission(cmd, tx));
        return match tokio::time::timeout(Duration::from_secs(secs), rx).await {
            Ok(Ok(allow)) => {
                let mut r = plain(StatusCode::OK, "");
                *r.body_mut() = Body::from(script::permission_reply(allow));
                r.headers_mut().insert(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("application/json"),
                );
                r
            }
            // Not answered, or Pillbox does not answer for this agent: no decision.
            _ => plain(StatusCode::NO_CONTENT, ""),
        };
    }
    s.send(Inbound::Cmd(cmd));
    plain(StatusCode::OK, "{\"ok\":true}")
}

/// Fill in the model and thinking effort from the session's transcript (Claude Code writes
/// both on every assistant message). Read only at the moments a turn starts, ends or asks, and
/// only from inside `~\.claude\projects`, so a client cannot make Pillbox read other files.
fn enrich(u: &mut AgentUpdate, hook: &Value) {
    if u.agent != "claude" {
        return;
    }
    let event = hook
        .get("hook_event_name")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !matches!(
        event,
        "SessionStart" | "UserPromptSubmit" | "Stop" | "PermissionRequest"
    ) {
        return;
    }
    let Some(path) = hook.get("transcript_path").and_then(Value::as_str) else {
        return;
    };
    let Some(tail) = read_transcript_tail(Path::new(path)) else {
        return;
    };
    if let Some((model, effort)) = script::transcript_info(&tail) {
        u.model = Some(model);
        if !effort.is_empty() {
            u.effort = Some(effort);
        }
    }
}

fn read_transcript_tail(path: &Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let root = PathBuf::from(std::env::var_os("USERPROFILE")?)
        .join(".claude")
        .join("projects");
    let root = std::fs::canonicalize(root).ok()?;
    let real = std::fs::canonicalize(path).ok()?;
    if !real.starts_with(&root) || real.extension().is_none_or(|e| e != "jsonl") {
        tracing::debug!("script server: transcript outside the Claude folder ignored");
        return None;
    }
    let mut f = std::fs::File::open(real).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(TRANSCRIPT_TAIL)))
        .ok()?;
    let mut buf = Vec::new();
    f.take(TRANSCRIPT_TAIL).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

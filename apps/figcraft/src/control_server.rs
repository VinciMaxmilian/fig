//! Localhost JSON-lines control server: each line `{"id", "method", "params"}` is queued for the UI
//! thread; the reply line `{"id", "ok", "result" | "error"}` is written when the frame handled it.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use figcraft_ui_egui::ControlRequest;
use serde_json::{Value, json};

type Queue = Arc<Mutex<Vec<ControlRequest>>>;
type CtxSlot = Arc<Mutex<Option<egui::Context>>>;

const MAX_LINE: usize = 32 * 1024 * 1024;
const MAX_CONNECTIONS: usize = 8;
const MAX_QUEUED_REQUESTS: usize = 8;
const TIMEOUT: Duration = Duration::from_secs(60);

/// Deferred requests for a hidden window must not make the UI's queue grow without a bound.
pub fn drain_queue(queue: &Queue, target: &mut Vec<ControlRequest>) {
    target.retain(|request| !request.is_cancelled());
    let mut queue = queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    queue.retain(|request| !request.is_cancelled());
    let count = queue.len().min(MAX_QUEUED_REQUESTS.saturating_sub(target.len()));
    target.extend(queue.drain(..count));
}

/// Listen on `127.0.0.1:port` (0: any free port) and return the port. With a `token`, a connection
/// must send `{"method":"auth","params":{"token":…}}` first; without one (`--control`) it is open
/// to every program of this user, as before.
pub fn start(port: u16, token: Option<String>, queue: Queue, ctx: CtxSlot) -> Option<u16> {
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            log::error!("control server: can't bind 127.0.0.1:{port}: {e} (another app may own it; pick another port)");
            eprintln!("FigCraft control server failed to bind port {port}: {e}");
            return None;
        }
    };
    let bound = listener.local_addr().ok()?.port();
    eprintln!("FigCraft control server listening on 127.0.0.1:{bound}");
    let token: Option<Arc<str>> = token.map(Into::into);
    let spawned = std::thread::Builder::new().name("figcraft-control".into()).spawn(move || {
        let mut workers = Vec::new();
        for stream in listener.incoming().flatten() {
            spawn_connection_with(stream, queue.clone(), ctx.clone(), &mut workers, TIMEOUT, token.clone());
        }
    });
    if let Err(e) = spawned {
        log::error!("control server thread: {e}");
        return None;
    }
    Some(bound)
}

#[cfg(test)]
fn spawn_connection(stream: TcpStream, queue: Queue, ctx: CtxSlot, workers: &mut Vec<std::thread::JoinHandle<()>>, timeout: Duration) {
    spawn_connection_with(stream, queue, ctx, workers, timeout, None);
}

fn spawn_connection_with(
    stream: TcpStream,
    queue: Queue,
    ctx: CtxSlot,
    workers: &mut Vec<std::thread::JoinHandle<()>>,
    timeout: Duration,
    token: Option<Arc<str>>,
) {
    workers.retain(|worker| !worker.is_finished());
    if workers.len() >= MAX_CONNECTIONS {
        return;
    }
    match std::thread::Builder::new().name("figcraft-control-conn".into()).spawn(move || {
        let _ = serve_with(stream, queue, ctx, timeout, token.as_deref());
    }) {
        Ok(worker) => workers.push(worker),
        Err(e) => log::error!("control connection thread: {e}"),
    }
}

#[cfg(test)]
fn serve(out: TcpStream, queue: Queue, ctx: CtxSlot, timeout: Duration) -> std::io::Result<()> {
    serve_with(out, queue, ctx, timeout, None)
}

/// Whether `a` equals `b`, taking the same time wherever they differ.
fn same_secret(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

fn serve_with(mut out: TcpStream, queue: Queue, ctx: CtxSlot, timeout: Duration, token: Option<&str>) -> std::io::Result<()> {
    // Protocol lines are JSON objects. A web page can make a browser send text to this port, but
    // only as HTTP (`POST / HTTP/1.1`, `GET …` for a WebSocket), so a connection that doesn't open
    // with `{` is dropped before anything is dispatched.
    let mut first = true;
    let mut authed = token.is_none();
    out.set_read_timeout(Some(timeout))?;
    out.set_write_timeout(Some(timeout))?;
    let mut reader = BufReader::new(out.try_clone()?);
    let mut line = Vec::new();
    loop {
        line.clear();
        if (&mut reader).take((MAX_LINE + 1) as u64).read_until(b'\n', &mut line)? == 0 {
            break;
        }
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        if line.len() > MAX_LINE {
            writeln!(out, "{}", json!({"id": null, "ok": false, "error": "message too large"}))?;
            break;
        }
        let Ok(line) = std::str::from_utf8(&line) else { break };
        if line.trim().is_empty() {
            continue;
        }
        if std::mem::take(&mut first) && !line.trim_start().starts_with('{') {
            break;
        }
        if !authed {
            let sent = serde_json::from_str::<Value>(line).ok();
            let id = sent.as_ref().and_then(|v| v.get("id").cloned()).unwrap_or(Value::Null);
            let is_auth = sent.as_ref().and_then(|v| v.get("method")).and_then(Value::as_str) == Some("auth");
            let given = sent.as_ref().filter(|_| is_auth).and_then(|v| v.pointer("/params/token")).and_then(Value::as_str).unwrap_or("");
            if token.is_some_and(|t| same_secret(t, given)) {
                authed = true;
                writeln!(out, "{}", json!({"id": id, "ok": true, "result": {}}))?;
                continue;
            }
            writeln!(
                out,
                "{}",
                json!({"id": id, "ok": false, "error": "authentication required: send {\"method\":\"auth\",\"params\":{\"token\":…}} first"})
            )?;
            break;
        }
        let reply = match serde_json::from_str::<Value>(line) {
            Ok(v) => {
                let id = v.get("id").cloned().unwrap_or(Value::Null);
                let method = v.get("method").and_then(Value::as_str).unwrap_or("").to_string();
                let params = v.get("params").cloned().unwrap_or(json!({}));
                let (tx, rx) = std::sync::mpsc::channel();
                let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let mut pending = queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                pending.retain(|request| !request.is_cancelled());
                if pending.len() >= MAX_QUEUED_REQUESTS {
                    drop(pending);
                    writeln!(out, "{}", json!({"id": id, "ok": false, "error": "control request queue is full"}))?;
                    continue;
                }
                pending.push(ControlRequest { id: id.clone(), method, params, reply: tx, cancelled: cancelled.clone() });
                drop(pending);
                if let Some(c) = ctx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref() {
                    c.request_repaint();
                }
                match rx.recv_timeout(timeout) {
                    Ok(r) => match r.result {
                        Ok(res) => json!({"id": r.id, "ok": true, "result": res}),
                        Err(e) => json!({"id": r.id, "ok": false, "error": e}),
                    },
                    Err(_) => {
                        cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
                        json!({"id": id, "ok": false, "error": "timed out waiting for the UI (is the window visible?)"})
                    }
                }
            }
            Err(e) => json!({"id": null, "ok": false, "error": format!("bad JSON: {e}")}),
        };
        writeln!(out, "{reply}")?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "control_server_tests.rs"]
mod tests;

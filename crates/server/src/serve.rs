//! The HTTP accept loop.
//!
//! `axum::serve` (0.7) builds its hyper connections without a timer, so a
//! client could hold a connection open forever by sending a request head
//! one byte at a time, or nothing at all (slowloris), and it accepts any
//! number of connections from one address. This loop is the same, plus:
//!
//! - a header read timeout ([`ServeLimits::header_timeout`]): a request head
//!   must arrive whole within it, and an idle keep-alive connection is
//!   closed after it (so is a new connection that sends nothing);
//! - at most [`ServeLimits::conns_per_ip`] open connections per client
//!   address (its /64 for IPv6, as for the rate limits); more are closed as
//!   soon as they are accepted. A trusted reverse proxy (`--trusted-proxy`)
//!   carries everyone's connections and is not capped. A connection upgraded to a WebSocket leaves
//!   the count (players are capped by `http::MAX_CONNS_PER_PLAYER`).
//!
//! The peer address is handed to the handlers as axum's `ConnectInfo`.

use std::collections::HashMap;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::ConnectInfo;
use axum::Router;
use hyper::body::Incoming;
use hyper::Request;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto::Builder;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tower_service::Service;

use crate::limits::{client_key, TrustedProxies};

/// What one client may hold.
#[derive(Clone, Copy, Debug)]
pub struct ServeLimits {
    pub header_timeout: Duration,
    pub conns_per_ip: usize,
}

type Open = Arc<Mutex<HashMap<IpAddr, usize>>>;

/// One open connection of a client, counted while it lives.
struct Slot {
    open: Open,
    key: IpAddr,
}

impl Slot {
    fn take(open: &Open, key: IpAddr, max: usize) -> Option<Slot> {
        let mut g = open.lock().unwrap();
        let n = g.entry(key).or_default();
        if *n >= max {
            return None;
        }
        *n += 1;
        Some(Slot { open: open.clone(), key })
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut g = self.open.lock().unwrap();
        if let Some(n) = g.get_mut(&self.key) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                g.remove(&self.key);
            }
        }
    }
}

/// Wait (without consuming anything) until the client has sent enough for
/// hyper to tell HTTP/1 from HTTP/2: its first bytes differ from the HTTP/2
/// preface, or the whole preface is there. hyper-util's version detection
/// waits for that with no timer, before HTTP/1's header timeout applies.
async fn protocol_known(stream: &tokio::net::TcpStream) -> std::io::Result<()> {
    const PREFACE: &[u8; 24] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
    let mut buf = [0u8; 24];
    loop {
        let n = stream.peek(&mut buf).await?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        if n == buf.len() || buf[..n] != PREFACE[..n] {
            return Ok(());
        }
        // A partial preface: `peek` would return at once, so wait a little.
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Serve `app` on `listener` until `stop` resolves, then let open requests
/// finish (graceful shutdown) and return.
pub async fn serve(
    listener: TcpListener,
    app: Router,
    limits: ServeLimits,
    trusted: TrustedProxies,
    stop: impl Future<Output = ()>,
) {
    let open: Open = Default::default();
    let (stop_tx, stop_rx) = watch::channel(false);
    // Every connection task holds a receiver; `closed` resolves when all are gone.
    let (done_tx, done_rx) = watch::channel(());
    tokio::pin!(stop);
    loop {
        let (stream, peer) = tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok(c) => c,
                Err(e) => {
                    // Out of file descriptors and the like: don't spin.
                    tracing::debug!("accept failed: {e}");
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
            },
            _ = &mut stop => break,
        };
        // A trusted reverse proxy carries every player's connections, so
        // it is not capped here; the per-client limits behind it use the
        // address it forwards.
        let slot = if trusted.contains(peer.ip()) {
            None
        } else {
            let Some(slot) = Slot::take(&open, client_key(peer.ip()), limits.conns_per_ip) else {
                drop(stream);
                continue;
            };
            Some(slot)
        };
        let app = app.clone();
        let mut stop_rx = stop_rx.clone();
        let done_rx = done_rx.clone();
        tokio::spawn(async move {
            let _slot = slot;
            let _done = done_rx;
            // hyper's header timer starts only once the protocol is known.
            if !matches!(tokio::time::timeout(limits.header_timeout, protocol_known(&stream)).await, Ok(Ok(()))) {
                return;
            }
            let service = hyper::service::service_fn(move |mut req: Request<Incoming>| {
                req.extensions_mut().insert(ConnectInfo::<SocketAddr>(peer));
                app.clone().call(req)
            });
            let mut builder = Builder::new(TokioExecutor::new());
            builder.http1().timer(TokioTimer::new()).header_read_timeout(limits.header_timeout);
            builder.http2().timer(TokioTimer::new()).keep_alive_interval(limits.header_timeout);
            let conn = builder.serve_connection_with_upgrades(TokioIo::new(stream), service);
            tokio::pin!(conn);
            let mut stopping = false;
            loop {
                tokio::select! {
                    result = conn.as_mut() => {
                        if let Err(e) = result {
                            tracing::trace!("connection from {peer}: {e}");
                        }
                        break;
                    }
                    changed = stop_rx.changed(), if !stopping => {
                        stopping = true;
                        if changed.is_ok() {
                            conn.as_mut().graceful_shutdown();
                        }
                    }
                }
            }
        });
    }
    drop(listener);
    let _ = stop_tx.send(true);
    drop(done_rx);
    done_tx.closed().await;
}

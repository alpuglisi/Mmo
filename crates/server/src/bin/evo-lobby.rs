//! `evo-lobby`: the front door in front of several `evo-server` processes
//! (a prototype; see "Beyond one box" in `docs/load-test.md`).
//!
//! ```text
//! evo-lobby --bind 0.0.0.0:8000 --backend http://10.0.0.11:8080 --backend http://10.0.0.12:8080
//! ```
//!
//! Each backend is a server process hosting one or more worlds. The lobby
//! keeps no state of its own. `GET /lobby/assign?name=ann` asks every
//! backend's `GET /api/assign?name=ann` at once (one second each) and
//! answers with the server and world the player should use:
//!
//! - a backend where the player has a lineage, if any (players stay with
//!   their lineage);
//! - else the same rule over every world of every backend
//!   (`worlds::assign`): the busiest world still under its fill target,
//!   then the one with the most room;
//! - 503 `full` when every world is full, or no backend answered.
//!
//! The answer is `{"server", "world", "name", "reason", "players_online",
//! "capacity", "has_account"}`. The client then logs in (or registers) on
//! that server and opens `/ws?world=` there, as today. The backends must
//! share one PostgreSQL database for accounts to work everywhere. Login
//! sessions are per process, so a login is made on the server the player
//! plays on.
//!
//! `GET /lobby/servers` shows each backend's worlds and load as last asked.
//!
//! The web client asks the lobby when given one (`?lobby=URL` or the
//! build-time `VITE_EVO_LOBBY`, `client/src/assign.ts`), before logging in.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use evo_server::worlds::{assign, AssignReason, AssignView, WorldLoad};
use serde::{Deserialize, Serialize};

const USAGE: &str = "usage: evo-lobby [--bind 127.0.0.1:8000] --backend URL [--backend URL ...]";

#[derive(Clone)]
struct Lobby {
    backends: Arc<Vec<String>>,
    http: reqwest::Client,
}

#[derive(Deserialize)]
struct AssignQuery {
    name: Option<String>,
}

#[derive(Serialize)]
struct LobbyAssign {
    /// The backend to log in to and play on.
    server: String,
    #[serde(flatten)]
    view: AssignView,
}

/// A successful JSON answer, else `None`.
async fn json<T: serde::de::DeserializeOwned>(req: reqwest::RequestBuilder) -> Option<T> {
    let r = req.send().await.ok().filter(|r| r.status().is_success())?;
    serde_json::from_slice(&r.bytes().await.ok()?).ok()
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(serde_json::json!({ "code": code, "message": message }))).into_response()
}

impl Lobby {
    /// Every backend's `/api/assign` for `name`, as (backend, answer, or
    /// `None` for full / unreachable).
    async fn ask(&self, name: Option<&str>) -> Vec<(String, Option<AssignView>)> {
        // Account names are letters, digits, `_` and `-`: nothing to escape.
        let name = name.filter(|n| n.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'));
        let calls = self.backends.iter().map(|b| {
            let url = match name {
                Some(n) => format!("{b}/api/assign?name={n}"),
                None => format!("{b}/api/assign"),
            };
            let req = self.http.get(url);
            async move { (b.clone(), json::<AssignView>(req).await) }
        });
        futures_util::future::join_all(calls).await
    }
}

/// Pick among the backends' answers: a lineage first, else
/// `worlds::assign` over each backend's choice.
fn choose(answers: Vec<(String, Option<AssignView>)>) -> Option<LobbyAssign> {
    let ok: Vec<(String, AssignView)> = answers.into_iter().filter_map(|(b, v)| Some((b, v?))).collect();
    let loads: Vec<WorldLoad> = ok
        .iter()
        .map(|(_, v)| WorldLoad {
            players_online: v.players_online,
            capacity: v.capacity,
            has_lineage: v.reason == AssignReason::Lineage,
        })
        .collect();
    let (i, reason) = assign(&loads)?;
    let (server, mut view) = ok.into_iter().nth(i)?;
    view.reason = reason;
    Some(LobbyAssign { server, view })
}

async fn lobby_assign(State(lobby): State<Lobby>, Query(q): Query<AssignQuery>) -> Response {
    match choose(lobby.ask(q.name.as_deref()).await) {
        Some(a) => Json(a).into_response(),
        None => error(StatusCode::SERVICE_UNAVAILABLE, "full", "every world is full or no server answered"),
    }
}

async fn lobby_servers(State(lobby): State<Lobby>) -> Response {
    let calls = lobby.backends.iter().map(|b| {
        let req = lobby.http.get(format!("{b}/api/worlds"));
        async move {
            let worlds = json::<serde_json::Value>(req).await;
            serde_json::json!({ "server": b, "up": worlds.is_some(), "worlds": worlds })
        }
    });
    Json(futures_util::future::join_all(calls).await).into_response()
}

fn parse_args() -> Result<(SocketAddr, Vec<String>), String> {
    let mut bind: SocketAddr = "127.0.0.1:8000".parse().unwrap();
    let mut backends = Vec::new();
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut v = || it.next().ok_or_else(|| format!("{k} needs a value"));
        match k.as_str() {
            "--bind" => bind = v()?.parse().map_err(|e| format!("--bind: {e}"))?,
            "--backend" => backends.push(v()?.trim_end_matches('/').to_string()),
            "-h" | "--help" => return Err(USAGE.into()),
            other => return Err(format!("unknown argument {other}\n{USAGE}")),
        }
    }
    if backends.is_empty() {
        return Err(format!("no --backend\n{USAGE}"));
    }
    Ok((bind, backends))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).init();
    let (bind, backends) = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let http = reqwest::Client::builder().timeout(Duration::from_secs(1)).build().expect("http client");
    let lobby = Lobby { backends: Arc::new(backends), http };
    let app = Router::new()
        .route("/lobby/assign", get(lobby_assign))
        .route("/lobby/servers", get(lobby_servers))
        // Pages from anywhere (the game servers' own pages, the apps) ask
        // it; it holds no credentials.
        .layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods([axum::http::Method::GET]),
        )
        .with_state(lobby);
    let listener = tokio::net::TcpListener::bind(bind).await.expect("bind");
    tracing::info!("lobby on http://{}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.expect("serve");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(players_online: usize, capacity: usize, reason: AssignReason) -> AssignView {
        AssignView { world: "w".into(), name: "W".into(), reason, players_online, capacity, has_account: true }
    }

    #[test]
    fn the_lobby_prefers_lineages_then_fills() {
        let a = |b: &str, v| (b.to_string(), v);
        // A lineage on the second server wins over room on the first.
        let got = choose(vec![
            a("http://a", Some(view(10, 100, AssignReason::Fill))),
            a("http://b", Some(view(99, 100, AssignReason::Lineage))),
        ])
        .unwrap();
        assert_eq!((got.server.as_str(), got.view.reason), ("http://b", AssignReason::Lineage));
        // Otherwise the busiest under the fill target; a dead server is skipped.
        let got = choose(vec![
            a("http://a", Some(view(10, 100, AssignReason::Fill))),
            a("http://dead", None),
            a("http://c", Some(view(60, 100, AssignReason::Fill))),
        ])
        .unwrap();
        assert_eq!(got.server, "http://c");
        assert!(choose(vec![a("http://dead", None)]).is_none());
    }
}

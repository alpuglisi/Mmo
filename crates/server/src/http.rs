//! HTTP routes and the WebSocket session.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot, watch};
use tower_http::services::{ServeDir, ServeFile};

use crate::accounts::{Accounts, AuthError, Session};
use crate::design::{self, DesignSpec, SpecError};
use crate::game::FoundError;
use crate::protocol::{ClientEnvelope, ErrorInfo, ServerMsg};
use crate::runner::{Published, Request};
use crate::store::{DesignRecord, Store};

/// Largest WebSocket message a client may send.
const MAX_CLIENT_MESSAGE: usize = 16 * 1024;
/// Per-connection queue of outgoing messages.
const CONN_QUEUE: usize = 256;

#[derive(Clone)]
pub struct AppState {
    pub world: SyncSender<Request>,
    pub accounts: Arc<Accounts>,
    pub store: Arc<dyn Store>,
    pub published: watch::Receiver<Arc<Published>>,
    pub next_conn: Arc<AtomicU64>,
}

/// The API routes, plus the files in `static_dir` (the built web client)
/// on every other path. A path with no matching file gets `index.html`.
pub fn router(state: AppState, static_dir: Option<&Path>) -> Router {
    let api = api_router(state);
    match static_dir {
        Some(dir) => api.fallback_service(ServeDir::new(dir).fallback(ServeFile::new(dir.join("index.html")))),
        None => api,
    }
}

fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/api/register", post(register))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/lineage", post(found_lineage))
        .route("/api/genes", get(genes))
        .route("/api/world", get(world_summary))
        .route("/api/leaderboard", get(leaderboard))
        .route("/api/leaderboard/history", get(leaderboard_history))
        .route("/api/map.png", get(map_png))
        .route("/ws", get(ws_upgrade))
        .with_state(state)
}

/// JSON error body: `{"code": "...", "message": "...", ...detail}`.
pub struct ApiError {
    status: StatusCode,
    code: String,
    message: String,
    detail: Option<serde_json::Value>,
}

impl ApiError {
    fn new(status: StatusCode, code: &str, message: impl Into<String>) -> Self {
        Self { status, code: code.into(), message: message.into(), detail: None }
    }

    fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized", "missing or unknown session token")
    }

    fn busy() -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, "busy", "the world is busy; try again")
    }

    fn internal(e: impl std::fmt::Display) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", e.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut body = serde_json::json!({ "code": self.code, "message": self.message });
        if let Some(serde_json::Value::Object(extra)) = self.detail {
            body.as_object_mut().unwrap().extend(extra);
        }
        (self.status, Json(body)).into_response()
    }
}

impl From<AuthError> for ApiError {
    fn from(e: AuthError) -> Self {
        let status = match e {
            AuthError::BadCredentials => StatusCode::UNAUTHORIZED,
            AuthError::NameTaken => StatusCode::CONFLICT,
            AuthError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::BAD_REQUEST,
        };
        ApiError::new(status, e.code(), e.message())
    }
}

impl From<SpecError> for ApiError {
    fn from(e: SpecError) -> Self {
        let mut err = ApiError::new(StatusCode::BAD_REQUEST, e.code(), e.message());
        err.detail = e.detail();
        err
    }
}

fn send_request(world: &SyncSender<Request>, req: Request) -> Result<(), ApiError> {
    world.try_send(req).map_err(|e| match e {
        TrySendError::Full(_) => ApiError::busy(),
        TrySendError::Disconnected(_) => ApiError::internal("world stopped"),
    })
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers.get(header::AUTHORIZATION)?.to_str().ok()?.strip_prefix("Bearer ").map(str::trim)
}

fn authed(app: &AppState, headers: &HeaderMap) -> Result<Session, ApiError> {
    bearer(headers).and_then(|t| app.accounts.session(t)).ok_or_else(ApiError::unauthorized)
}

#[derive(Deserialize)]
pub struct Credentials {
    pub name: String,
    pub password: String,
}

#[derive(Serialize, Deserialize)]
pub struct AuthResponse {
    pub token: String,
    pub player: u32,
    pub name: String,
}

async fn register(State(app): State<AppState>, Json(c): Json<Credentials>) -> Result<(StatusCode, Json<AuthResponse>), ApiError> {
    let (token, s) = app.accounts.register(c.name.trim(), &c.password).await?;
    Ok((StatusCode::CREATED, Json(AuthResponse { token, player: s.player, name: s.name })))
}

async fn login(State(app): State<AppState>, Json(c): Json<Credentials>) -> Result<Json<AuthResponse>, ApiError> {
    let (token, s) = app.accounts.login(c.name.trim(), &c.password).await?;
    Ok(Json(AuthResponse { token, player: s.player, name: s.name }))
}

async fn logout(State(app): State<AppState>, headers: HeaderMap) -> StatusCode {
    if let Some(t) = bearer(&headers) {
        app.accounts.logout(t);
    }
    StatusCode::NO_CONTENT
}

#[derive(Serialize, Deserialize)]
pub struct FoundResponse {
    pub lineage: u32,
    pub founders: Vec<u64>,
    pub cost: f32,
}

async fn found_lineage(State(app): State<AppState>, headers: HeaderMap, Json(spec): Json<DesignSpec>) -> Result<(StatusCode, Json<FoundResponse>), ApiError> {
    let session = authed(&app, &headers)?;
    let design = spec.to_design()?;
    let (reply, rx) = oneshot::channel();
    send_request(&app.world, Request::Found { player: session.player, name: session.name.clone(), design, reply })?;
    let ok = match rx.await.map_err(ApiError::internal)? {
        Ok(ok) => ok,
        Err(FoundError::AlreadyFounded { lineage }) => {
            let mut e = ApiError::new(StatusCode::CONFLICT, "already_founded", "you already founded a lineage in this world");
            e.detail = Some(serde_json::json!({ "lineage": lineage }));
            return Err(e);
        }
        Err(FoundError::Design(d)) => return Err(SpecError::Design(d).into()),
    };
    let day = app.published.borrow().summary.clock.day;
    let unix = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let record = DesignRecord { player: session.player, design: spec, founded_day: day, founded_unix: unix };
    if let Err(e) = app.store.put_design(&record).await {
        tracing::error!("could not persist design of player {}: {e}", session.player);
    }
    Ok((StatusCode::CREATED, Json(FoundResponse { lineage: ok.lineage, founders: ok.founders, cost: ok.cost })))
}

async fn genes() -> Json<design::GeneCatalog> {
    Json(design::catalog())
}

async fn world_summary(State(app): State<AppState>) -> Json<crate::protocol::WorldSummary> {
    Json(app.published.borrow().summary.clone())
}

async fn leaderboard(State(app): State<AppState>) -> Json<crate::protocol::Leaderboard> {
    Json(app.published.borrow().leaderboard.clone())
}

#[derive(Deserialize)]
struct HistoryQuery {
    limit: Option<usize>,
}

async fn leaderboard_history(State(app): State<AppState>, Query(q): Query<HistoryQuery>) -> Result<Json<Vec<crate::store::DailyRecord>>, ApiError> {
    let limit = q.limit.unwrap_or(30).clamp(1, 1000);
    Ok(Json(app.store.recent_daily(limit).await.map_err(ApiError::internal)?))
}

#[derive(Deserialize)]
struct MapQuery {
    scale: Option<usize>,
}

async fn map_png(State(app): State<AppState>, Query(q): Query<MapQuery>) -> Result<Response, ApiError> {
    let scale = q.scale.unwrap_or(4).clamp(1, 8);
    let (reply, rx) = oneshot::channel();
    send_request(&app.world, Request::Snapshot { scale, reply })?;
    let (w, h, rgb) = rx.await.map_err(ApiError::internal)?;
    let png = tokio::task::spawn_blocking(move || encode_png(w, h, &rgb)).await.map_err(ApiError::internal)?.map_err(ApiError::internal)?;
    Ok(([(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "no-store")], png).into_response())
}

fn encode_png(w: usize, h: usize, rgb: &[u8]) -> Result<Vec<u8>, png::EncodingError> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header()?;
        writer.write_image_data(rgb)?;
    }
    Ok(out)
}

#[derive(Deserialize)]
struct WsQuery {
    token: Option<String>,
}

/// `GET /ws?token=…` (browsers can't set headers on a WebSocket) or with an
/// `Authorization: Bearer …` header.
async fn ws_upgrade(
    State(app): State<AppState>,
    Query(q): Query<WsQuery>,
    headers: HeaderMap,
    ws: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Result<Response, ApiError> {
    // Authenticate before looking at the upgrade, so a bad token is a 401.
    let token = q.token.as_deref().or_else(|| bearer(&headers)).ok_or_else(ApiError::unauthorized)?;
    let session = app.accounts.session(token).ok_or_else(ApiError::unauthorized)?;
    let ws = match ws {
        Ok(ws) => ws,
        Err(rejection) => return Ok(rejection.into_response()),
    };
    Ok(ws.max_message_size(MAX_CLIENT_MESSAGE).on_upgrade(move |socket| client_session(socket, app, session)))
}

async fn client_session(socket: WebSocket, app: AppState, session: Session) {
    let conn = app.next_conn.fetch_add(1, Ordering::Relaxed);
    let (tx, mut rx) = mpsc::channel::<ServerMsg>(CONN_QUEUE);
    // The reader only holds a weak handle, so when the world thread drops a
    // connection (too slow), the writer ends and the socket closes.
    let weak = tx.downgrade();
    let connect = Request::Connect { conn, player: session.player, name: session.name.clone(), tx };
    if app.world.try_send(connect).is_err() {
        return;
    }
    let (mut sink, mut stream) = socket.split();
    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let Ok(text) = serde_json::to_string(&msg) else { continue };
            if sink.send(Message::Text(text)).await.is_err() {
                return;
            }
        }
        let _ = sink.close().await;
    });
    let local_error = |e: ErrorInfo| {
        if let Some(tx) = weak.upgrade() {
            let _ = tx.try_send(ServerMsg::err(None, "message", e));
        }
    };
    while let Some(Ok(frame)) = stream.next().await {
        match frame {
            Message::Text(text) => match serde_json::from_str::<ClientEnvelope>(&text) {
                Ok(env) => {
                    let req = Request::Command { conn, player: session.player, id: env.id, msg: env.msg };
                    match app.world.try_send(req) {
                        Ok(()) => {}
                        Err(TrySendError::Full(_)) => local_error(ErrorInfo::new("busy", "the world is busy; try again")),
                        Err(TrySendError::Disconnected(_)) => break,
                    }
                }
                Err(e) => local_error(ErrorInfo::new("bad_message", e.to_string())),
            },
            Message::Close(_) => break,
            _ => {}
        }
    }
    let _ = app.world.try_send(Request::Disconnect { conn });
    writer.abort();
}

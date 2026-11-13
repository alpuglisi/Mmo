//! HTTP routes and the WebSocket session.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::{ConnectInfo, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot, watch};
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};

use crate::accounts::{Accounts, AuthError, Session};
use crate::design::{self, DesignSpec, SpecError};
use crate::game::FoundError;
use crate::limits::{Bucket, Limiters};
use crate::protocol::{
    AchievementList, CatastropheList, ChronicleView, ClientEnvelope, EraList, ErrorInfo, LineageStats, SeasonBoard, ServerMsg, SpeciesList,
    TerrainDynamic, TerrainView,
};
use crate::runner::{Published, Request, TerrainReply};
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
    pub limiters: Arc<Limiters>,
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
        .route("/api/leaderboard/seasons", get(leaderboard_seasons))
        .route("/api/lineage/stats", get(lineage_stats))
        .route("/api/species", get(species))
        .route("/api/eras", get(eras))
        .route("/api/catastrophes", get(catastrophes))
        .route("/api/achievements", get(achievements))
        .route("/api/terrain", get(terrain))
        .route("/api/terrain/dynamic", get(terrain_dynamic))
        .route("/api/map.png", get(map_png))
        .route("/ws", get(ws_upgrade))
        .with_state(state)
        .layer(cors())
}

/// Cross-origin access to the API, so a client bundle served from elsewhere
/// (a Capacitor app's `capacitor://localhost` / `https://localhost`, or a
/// dev server on another port) can call it. Safe for any origin: auth is a
/// bearer token the page holds, never a cookie the browser would attach.
fn cors() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
        .max_age(Duration::from_secs(3600))
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

    fn rate_limited(retry: Duration) -> Self {
        let ms = retry.as_millis().max(1) as u64;
        let mut e = Self::new(StatusCode::TOO_MANY_REQUESTS, "rate_limited", "too many attempts; try again later");
        e.detail = Some(serde_json::json!({ "retry_after_ms": ms }));
        e
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut body = serde_json::json!({ "code": self.code, "message": self.message });
        let retry = self.detail.as_ref().and_then(|d| d.get("retry_after_ms")).and_then(|v| v.as_u64());
        if let Some(serde_json::Value::Object(extra)) = self.detail {
            body.as_object_mut().unwrap().extend(extra);
        }
        let mut res = (self.status, Json(body)).into_response();
        if let Some(ms) = retry {
            res.headers_mut().insert(header::RETRY_AFTER, ms.div_ceil(1000).max(1).into());
        }
        res
    }
}

/// The client's IP (unspecified when the server runs without connect info).
fn client_ip(info: &Option<ConnectInfo<SocketAddr>>) -> IpAddr {
    info.as_ref().map_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED), |c| c.0.ip())
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

async fn register(
    State(app): State<AppState>,
    info: Option<ConnectInfo<SocketAddr>>,
    Json(c): Json<Credentials>,
) -> Result<(StatusCode, Json<AuthResponse>), ApiError> {
    app.limiters.logins.check(&client_ip(&info)).map_err(ApiError::rate_limited)?;
    let (token, s) = app.accounts.register(c.name.trim(), &c.password).await?;
    Ok((StatusCode::CREATED, Json(AuthResponse { token, player: s.player, name: s.name })))
}

async fn login(
    State(app): State<AppState>,
    info: Option<ConnectInfo<SocketAddr>>,
    Json(c): Json<Credentials>,
) -> Result<Json<AuthResponse>, ApiError> {
    app.limiters.logins.check(&client_ip(&info)).map_err(ApiError::rate_limited)?;
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
struct SeasonsQuery {
    limit: Option<usize>,
}

/// Seasonal leaderboards from the whole stored history, newest first.
async fn leaderboard_seasons(State(app): State<AppState>, Query(q): Query<SeasonsQuery>) -> Result<Json<Vec<SeasonBoard>>, ApiError> {
    let limit = q.limit.unwrap_or(8).clamp(1, 100);
    let records = app.store.recent_daily(100_000).await.map_err(ApiError::internal)?;
    let clock = app.published.borrow().summary.clock.clone();
    let accounts = app.accounts.clone();
    let name_of = move |p: evo_sim::PlayerId| {
        if p == evo_sim::WILD {
            "wild".to_string()
        } else {
            accounts.name_of(p).unwrap_or_else(|| format!("player {p}"))
        }
    };
    let boards = tokio::task::spawn_blocking(move || crate::seasons::season_boards(&records, Some((clock.year, &clock.season)), name_of, limit))
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(boards))
}

async fn lineage_stats(State(app): State<AppState>, headers: HeaderMap) -> Result<Json<LineageStats>, ApiError> {
    let session = authed(&app, &headers)?;
    let (reply, rx) = oneshot::channel();
    send_request(&app.world, Request::Stats { player: session.player, reply })?;
    Ok(Json(rx.await.map_err(ApiError::internal)?))
}

async fn terrain_request(app: &AppState, full: bool) -> Result<TerrainReply, ApiError> {
    let (reply, rx) = oneshot::channel();
    send_request(&app.world, Request::Terrain { full, reply })?;
    rx.await.map_err(ApiError::internal)
}

const NO_STORE: [(header::HeaderName, &str); 1] = [(header::CACHE_CONTROL, "no-store")];

async fn terrain(State(app): State<AppState>) -> Result<Response, ApiError> {
    let TerrainReply::Full(raw) = terrain_request(&app, true).await? else { return Err(ApiError::internal("wrong terrain reply")) };
    let view: TerrainView = tokio::task::spawn_blocking(move || raw.encode()).await.map_err(ApiError::internal)?;
    Ok((NO_STORE, Json(view)).into_response())
}

async fn terrain_dynamic(State(app): State<AppState>) -> Result<Response, ApiError> {
    let TerrainReply::Dynamic(raw) = terrain_request(&app, false).await? else { return Err(ApiError::internal("wrong terrain reply")) };
    let view: TerrainDynamic = tokio::task::spawn_blocking(move || raw.encode()).await.map_err(ApiError::internal)?;
    Ok((NO_STORE, Json(view)).into_response())
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
    info: Option<ConnectInfo<SocketAddr>>,
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
    let token = token.to_string();
    let ip = client_ip(&info);
    Ok(ws.max_message_size(MAX_CLIENT_MESSAGE).on_upgrade(move |socket| client_session(socket, app, session, token, ip)))
}

/// How often an open WebSocket checks that its session is still live.
fn session_check_interval(app: &AppState) -> Duration {
    let p = app.accounts.policy();
    (p.idle.min(p.max_age) / 4).clamp(Duration::from_millis(50), Duration::from_secs(30))
}

async fn client_session(socket: WebSocket, app: AppState, session: Session, token: String, ip: IpAddr) {
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
    let local = |msg: ServerMsg| {
        if let Some(tx) = weak.upgrade() {
            let _ = tx.try_send(msg);
        }
    };
    let rate = app.limiters.limits.commands;
    let mut bucket = Bucket::new(rate, Instant::now());
    let mut check = tokio::time::interval(session_check_interval(&app));
    check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        let frame = tokio::select! {
            f = stream.next() => f,
            _ = check.tick() => {
                if app.accounts.is_live(&token) {
                    continue;
                }
                local(ServerMsg::SessionEnded { reason: "expired".into() });
                break;
            }
        };
        let Some(Ok(frame)) = frame else { break };
        match frame {
            Message::Text(text) => match serde_json::from_str::<ClientEnvelope>(&text) {
                Ok(env) => {
                    let name = env.msg.name();
                    // A command counts as use of the session.
                    if app.accounts.session(&token).is_none() {
                        local(ServerMsg::SessionEnded { reason: "expired".into() });
                        break;
                    }
                    let now = Instant::now();
                    if let Err(wait) = bucket.take(rate, now).and_then(|_| app.limiters.ip_commands.check_at(&ip, now)) {
                        let msg = format!("too many commands; retry in {} ms", wait.as_millis().max(1));
                        local(ServerMsg::err(env.id, name, ErrorInfo::new("rate_limited", msg)));
                        continue;
                    }
                    let req = Request::Command { conn, player: session.player, id: env.id, msg: env.msg };
                    match app.world.try_send(req) {
                        Ok(()) => {}
                        Err(TrySendError::Full(_)) => local(ServerMsg::err(env.id, name, ErrorInfo::new("busy", "the world is busy; try again"))),
                        Err(TrySendError::Disconnected(_)) => break,
                    }
                }
                Err(e) => local(ServerMsg::err(None, "message", ErrorInfo::new("bad_message", e.to_string()))),
            },
            Message::Close(_) => break,
            _ => {}
        }
    }
    // The world thread drops its sender on disconnect, which ends the writer
    // after it has flushed what is queued (e.g. `session_ended`).
    let _ = app.world.try_send(Request::Disconnect { conn });
    let mut writer = writer;
    if tokio::time::timeout(Duration::from_secs(2), &mut writer).await.is_err() {
        writer.abort();
    }
}

/// The world thread's chronicle (species, eras, catastrophes, achievements).
async fn chronicle(st: &AppState) -> Result<ChronicleView, ApiError> {
    let (reply, rx) = oneshot::channel();
    send_request(&st.world, Request::Chronicle { reply })?;
    Ok(*rx.await.map_err(ApiError::internal)?)
}

/// `GET /api/species`: every detected species (living first, by size), with
/// parents (the species tree), founders and dominant allele owners.
async fn species(State(st): State<AppState>) -> Result<Json<SpeciesList>, ApiError> {
    Ok(Json(chronicle(&st).await?.species))
}

/// `GET /api/eras`: the era clock and every finished era's final ranking.
async fn eras(State(st): State<AppState>) -> Result<Json<EraList>, ApiError> {
    Ok(Json(chronicle(&st).await?.eras))
}

/// `GET /api/catastrophes`: the active catastrophe and those before it.
async fn catastrophes(State(st): State<AppState>) -> Result<Json<CatastropheList>, ApiError> {
    Ok(Json(chronicle(&st).await?.catastrophes))
}

/// `GET /api/achievements`: per-player tallies and the latest achievements.
async fn achievements(State(st): State<AppState>) -> Result<Json<AchievementList>, ApiError> {
    Ok(Json(chronicle(&st).await?.achievements))
}

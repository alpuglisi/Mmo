//! HTTP routes and the WebSocket session.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path as UrlPath, Query, Request as HttpRequest, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::{oneshot, watch};
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};

use evo_sim::PlayerId;

use crate::accounts::{Accounts, AuthError, Session};
use crate::design::{self, DesignSpec, SpecError};
use crate::game::FoundError;
use crate::limits::{client_key, Bucket, Limiters};
use crate::metrics::{Metrics, MetricsView, NetStats};
use crate::protocol::{
    AchievementList, AlertPrefs, CatastropheList, ChronicleView, ClientEnvelope, ClientMsg, EraList, ErrorInfo,
    LineageStats, SeasonBoard, ServerMsg, SpeciesList, TerrainDynamic, TerrainView,
};
use crate::runner::{Published, Request, TerrainReply};
use crate::store::{AlertPrefsRecord, DesignRecord, Store};
use crate::wire::{DeltaEncoder, KEYFRAME_EVERY, PROTO_DELTA, PROTO_JSON};

/// Largest WebSocket message (and frame) a client may send.
pub const MAX_CLIENT_MESSAGE: usize = 16 * 1024;
/// Per-connection queue of outgoing messages.
const CONN_QUEUE: usize = 256;
/// Largest HTTP request body (credentials, a founder design).
pub const MAX_BODY: usize = 64 * 1024;
/// Most WebSocket connections one player may hold open at once (tabs,
/// devices); each costs the world thread a view and a state per interval.
pub const MAX_CONNS_PER_PLAYER: usize = 8;
/// Longest side of `GET /api/map.png`, in pixels: the scale is lowered to
/// fit (8 pixels per tile on a 1024-tile map would be a 200 MB image).
pub const MAX_MAP_SIDE: usize = 2048;

#[derive(Clone)]
pub struct AppState {
    pub world: SyncSender<Request>,
    pub accounts: Arc<Accounts>,
    pub store: Arc<dyn Store>,
    pub published: watch::Receiver<Arc<Published>>,
    pub next_conn: Arc<AtomicU64>,
    pub limiters: Arc<Limiters>,
    pub metrics: Arc<Mutex<Metrics>>,
    pub net: Arc<NetStats>,
    /// Open WebSocket connections per player.
    pub ws_conns: Arc<Mutex<HashMap<PlayerId, usize>>>,
    /// The running world's id: `/api/seasons/:n` means its seasons unless
    /// the request names another world.
    pub world_id: Arc<str>,
}

/// One of a player's WebSocket connections, counted while it lives.
struct ConnSlot {
    conns: Arc<Mutex<HashMap<PlayerId, usize>>>,
    player: PlayerId,
}

impl ConnSlot {
    /// Take a slot, unless the player already has [`MAX_CONNS_PER_PLAYER`].
    fn take(conns: &Arc<Mutex<HashMap<PlayerId, usize>>>, player: PlayerId) -> Option<ConnSlot> {
        let mut g = conns.lock().unwrap();
        let n = g.entry(player).or_default();
        if *n >= MAX_CONNS_PER_PLAYER {
            return None;
        }
        *n += 1;
        Some(ConnSlot { conns: conns.clone(), player })
    }
}

impl Drop for ConnSlot {
    fn drop(&mut self) {
        let mut g = self.conns.lock().unwrap();
        if let Some(n) = g.get_mut(&self.player) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                g.remove(&self.player);
            }
        }
    }
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
    // Reads that cost the world thread or the store: rate-limited per client.
    let reads = Router::new()
        .route("/api/leaderboard/history", get(leaderboard_history))
        .route("/api/leaderboard/seasons", get(leaderboard_seasons))
        .route("/api/lineage/stats", get(lineage_stats))
        .route("/api/alerts/prefs", get(alert_prefs).post(set_alert_prefs))
        .route("/api/species", get(species))
        .route("/api/eras", get(eras))
        .route("/api/catastrophes", get(catastrophes))
        .route("/api/achievements", get(achievements))
        .route("/api/seasons", get(seasons_list))
        .route("/api/seasons/current", get(season_current))
        .route("/api/seasons/player/:player", get(season_player))
        .route("/api/seasons/:season", get(season_one))
        .route("/api/terrain", get(terrain))
        .route("/api/terrain/dynamic", get(terrain_dynamic))
        .route("/api/map.png", get(map_png))
        .route_layer(middleware::from_fn_with_state(state.clone(), limit_reads));
    Router::new()
        .route("/api/register", post(register))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/lineage", post(found_lineage))
        .route("/api/genes", get(genes))
        .route("/api/world", get(world_summary))
        .route("/api/leaderboard", get(leaderboard))
        .route("/api/metrics", get(metrics))
        .merge(reads)
        .route("/ws", get(ws_upgrade))
        .with_state(state)
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(cors())
}

/// Refuse a read past the client's `Limits::reads` rate.
async fn limit_reads(
    State(app): State<AppState>,
    info: Option<ConnectInfo<SocketAddr>>,
    req: HttpRequest,
    next: Next,
) -> Response {
    match app.limiters.reads.check(&client_ip(&info)) {
        Ok(()) => next.run(req).await,
        Err(wait) => ApiError::rate_limited(wait).into_response(),
    }
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

/// The client's rate-limit key: its IP, or its IPv6 /64 (`limits::client_key`;
/// unspecified when the server runs without connect info).
fn client_ip(info: &Option<ConnectInfo<SocketAddr>>) -> IpAddr {
    client_key(info.as_ref().map_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED), |c| c.0.ip()))
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

async fn found_lineage(
    State(app): State<AppState>,
    headers: HeaderMap,
    Json(spec): Json<DesignSpec>,
) -> Result<(StatusCode, Json<FoundResponse>), ApiError> {
    let session = authed(&app, &headers)?;
    let design = spec.to_design()?;
    let (reply, rx) = oneshot::channel();
    send_request(&app.world, Request::Found { player: session.player, name: session.name.clone(), design, reply })?;
    let ok = match rx.await.map_err(ApiError::internal)? {
        Ok(ok) => ok,
        Err(FoundError::AlreadyFounded { lineage }) => {
            let mut e =
                ApiError::new(StatusCode::CONFLICT, "already_founded", "you already founded a lineage in this world");
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

/// `GET /api/metrics`: world-thread timings, queues, saves, traffic and
/// process usage ([`MetricsView`]).
async fn metrics(State(app): State<AppState>) -> Json<MetricsView> {
    let tick = app.published.borrow().summary.clock.tick;
    Json(MetricsView::gather(&app.metrics, &app.net, tick))
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

async fn leaderboard_history(
    State(app): State<AppState>,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<Vec<crate::store::DailyRecord>>, ApiError> {
    let limit = q.limit.unwrap_or(30).clamp(1, 1000);
    Ok(Json(app.store.recent_daily(limit).await.map_err(ApiError::internal)?))
}

#[derive(Deserialize)]
struct SeasonsQuery {
    limit: Option<usize>,
}

/// Seasonal leaderboards from the whole stored history, newest first.
async fn leaderboard_seasons(
    State(app): State<AppState>,
    Query(q): Query<SeasonsQuery>,
) -> Result<Json<Vec<SeasonBoard>>, ApiError> {
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
    let boards = tokio::task::spawn_blocking(move || {
        crate::seasons::season_boards(&records, Some((clock.year, &clock.season)), name_of, limit)
    })
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

/// The caller's alert preferences (the defaults until they save some).
async fn alert_prefs(State(app): State<AppState>, headers: HeaderMap) -> Result<Json<AlertPrefs>, ApiError> {
    let session = authed(&app, &headers)?;
    let stored = app.store.load_alert_prefs(session.player).await.map_err(ApiError::internal)?;
    Ok(Json(stored.map(|r| r.prefs.normalized()).unwrap_or_default()))
}

/// Replace the caller's alert preferences; replies with them normalized.
/// Unchanged preferences are not written again.
async fn set_alert_prefs(
    State(app): State<AppState>,
    headers: HeaderMap,
    Json(prefs): Json<AlertPrefs>,
) -> Result<Json<AlertPrefs>, ApiError> {
    let session = authed(&app, &headers)?;
    let prefs = prefs.normalized();
    let stored = app.store.load_alert_prefs(session.player).await.map_err(ApiError::internal)?;
    if stored.map(|r| r.prefs.normalized()) != Some(prefs.clone()) {
        let record = AlertPrefsRecord { player: session.player, prefs: prefs.clone() };
        app.store.put_alert_prefs(&record).await.map_err(ApiError::internal)?;
    }
    Ok(Json(prefs))
}

async fn terrain_request(app: &AppState, full: bool) -> Result<TerrainReply, ApiError> {
    let (reply, rx) = oneshot::channel();
    send_request(&app.world, Request::Terrain { full, reply })?;
    rx.await.map_err(ApiError::internal)
}

const NO_STORE: [(header::HeaderName, &str); 1] = [(header::CACHE_CONTROL, "no-store")];

async fn terrain(State(app): State<AppState>) -> Result<Response, ApiError> {
    let TerrainReply::Full(raw) = terrain_request(&app, true).await? else {
        return Err(ApiError::internal("wrong terrain reply"));
    };
    let view: TerrainView = tokio::task::spawn_blocking(move || raw.encode()).await.map_err(ApiError::internal)?;
    Ok((NO_STORE, Json(view)).into_response())
}

async fn terrain_dynamic(State(app): State<AppState>) -> Result<Response, ApiError> {
    let TerrainReply::Dynamic(raw) = terrain_request(&app, false).await? else {
        return Err(ApiError::internal("wrong terrain reply"));
    };
    let view: TerrainDynamic = tokio::task::spawn_blocking(move || raw.encode()).await.map_err(ApiError::internal)?;
    Ok((NO_STORE, Json(view)).into_response())
}

#[derive(Deserialize)]
struct MapQuery {
    scale: Option<usize>,
}

async fn map_png(State(app): State<AppState>, Query(q): Query<MapQuery>) -> Result<Response, ApiError> {
    let side = {
        let s = &app.published.borrow().summary;
        s.width.max(s.height)
    };
    let scale = map_scale(q.scale, side);
    let (reply, rx) = oneshot::channel();
    send_request(&app.world, Request::Snapshot { scale, reply })?;
    let (w, h, rgb) = rx.await.map_err(ApiError::internal)?;
    let png = tokio::task::spawn_blocking(move || encode_png(w, h, &rgb))
        .await
        .map_err(ApiError::internal)?
        .map_err(ApiError::internal)?;
    Ok(([(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "no-store")], png).into_response())
}

/// Pixels per tile for the map image: as asked (default 4, 1 to 8), lowered
/// so the longest side stays within [`MAX_MAP_SIDE`].
pub fn map_scale(asked: Option<usize>, tiles: usize) -> usize {
    asked.unwrap_or(4).clamp(1, 8).min(MAX_MAP_SIDE / tiles.max(1)).max(1)
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
    /// The highest protocol the client speaks (1 when absent): 2 gets
    /// binary delta states (`wire.rs`).
    proto: Option<u32>,
}

/// `GET /ws?token=…` (browsers can't set headers on a WebSocket) or with an
/// `Authorization: Bearer …` header; `&proto=2` asks for delta states.
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
    let slot = ConnSlot::take(&app.ws_conns, session.player).ok_or_else(|| {
        ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_connections",
            format!("at most {MAX_CONNS_PER_PLAYER} connections per player"),
        )
    })?;
    let token = token.to_string();
    let ip = client_ip(&info);
    let proto = if q.proto.unwrap_or(PROTO_JSON) >= PROTO_DELTA { PROTO_DELTA } else { PROTO_JSON };
    // A frame is read whole before the message limit applies: bound both.
    Ok(ws.max_message_size(MAX_CLIENT_MESSAGE).max_frame_size(MAX_CLIENT_MESSAGE).on_upgrade(
        move |socket| async move {
            client_session(socket, app, session, token, ip, proto).await;
            drop(slot);
        },
    ))
}

/// A protocol 2 connection, counted in `delta_conns` while its writer lives
/// (also when the writer is aborted).
struct DeltaConn(Arc<NetStats>);

impl DeltaConn {
    fn new(net: &Arc<NetStats>) -> Self {
        net.delta_conns.fetch_add(1, Ordering::Relaxed);
        DeltaConn(net.clone())
    }
}

impl Drop for DeltaConn {
    fn drop(&mut self) {
        self.0.delta_conns.fetch_sub(1, Ordering::Relaxed);
    }
}

/// How often an open WebSocket checks that its session is still live.
fn session_check_interval(app: &AppState) -> Duration {
    let p = app.accounts.policy();
    (p.idle.min(p.max_age) / 4).clamp(Duration::from_millis(50), Duration::from_secs(30))
}

async fn client_session(socket: WebSocket, app: AppState, session: Session, token: String, ip: IpAddr, proto: u32) {
    let conn = app.next_conn.fetch_add(1, Ordering::Relaxed);
    let (tx, mut rx) = crate::push::channel(CONN_QUEUE);
    // The reader only holds a weak handle, so when the world thread drops a
    // connection (too slow), the writer ends and the socket closes.
    let weak = tx.downgrade();
    let connect = Request::Connect { conn, player: session.player, name: session.name.clone(), tx };
    if app.world.try_send(connect).is_err() {
        return;
    }
    let (mut sink, mut stream) = socket.split();
    let net = app.net.clone();
    net.ws_opened.fetch_add(1, Ordering::Relaxed);
    // Protocol 2: the reader asks for a keyframe when the client resyncs.
    let resync = Arc::new(AtomicBool::new(false));
    let wants_key = resync.clone();
    let writer = tokio::spawn(async move {
        let _counted = (proto == PROTO_DELTA).then(|| DeltaConn::new(&net));
        // States are encoded here, when sent: a delta is against the state
        // this client actually got, whatever the queue replaced meanwhile.
        let mut delta = (proto == PROTO_DELTA).then(DeltaEncoder::new);
        let mut last_key = Instant::now();
        while let Some(mut msg) = rx.recv().await {
            if let ServerMsg::Welcome { proto: p, .. } = &mut msg {
                *p = delta.is_some().then_some(PROTO_DELTA);
            }
            let started = Instant::now();
            let frame = match (&mut delta, &msg) {
                (Some(enc), ServerMsg::State(s)) => {
                    let key = wants_key.swap(false, Ordering::Relaxed) || last_key.elapsed() >= KEYFRAME_EVERY;
                    let bytes = enc.encode(s, key);
                    if bytes.first() == Some(&crate::wire::KEYFRAME) {
                        last_key = Instant::now();
                        net.keyframes_out.fetch_add(1, Ordering::Relaxed);
                    }
                    Message::Binary(bytes)
                }
                _ => match serde_json::to_string(&msg) {
                    Ok(text) => Message::Text(text),
                    Err(_) => continue,
                },
            };
            let len = match &frame {
                Message::Binary(b) => b.len(),
                Message::Text(t) => t.len(),
                _ => 0,
            };
            let state = matches!(msg, ServerMsg::State(_));
            if state {
                net.state_encode_ns.fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
            }
            net.sent(len, state);
            if sink.send(frame).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
        net.ws_closed.fetch_add(1, Ordering::Relaxed);
    });
    let local = |msg: ServerMsg| weak.try_send(msg);
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
                    if let Err(wait) = bucket.take(rate, now).and_then(|_| app.limiters.ip_commands.check_at(&ip, now))
                    {
                        let msg = format!("too many commands; retry in {} ms", wait.as_millis().max(1));
                        local(ServerMsg::err(env.id, name, ErrorInfo::new("rate_limited", msg)));
                        continue;
                    }
                    if env.msg == ClientMsg::Resync {
                        if proto == PROTO_DELTA {
                            resync.store(true, Ordering::Relaxed);
                            app.net.resyncs.fetch_add(1, Ordering::Relaxed);
                        }
                        continue;
                    }
                    let req = Request::Command { conn, player: session.player, id: env.id, msg: env.msg };
                    match app.world.try_send(req) {
                        Ok(()) => {}
                        Err(TrySendError::Full(_)) => {
                            local(ServerMsg::err(env.id, name, ErrorInfo::new("busy", "the world is busy; try again")))
                        }
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

#[derive(Deserialize)]
struct SeasonPaging {
    offset: Option<usize>,
    limit: Option<usize>,
    /// Always include this player's row in each category.
    player: Option<PlayerId>,
    /// The world whose season is meant (default: the running one).
    world: Option<String>,
}

/// `GET /api/seasons?offset=0&limit=20`: finished ranked seasons of every
/// world, newest first (heads: winners and summary, no standings).
async fn seasons_list(
    State(app): State<AppState>,
    Query(q): Query<SeasonPaging>,
) -> Result<Json<crate::league::SeasonList>, ApiError> {
    let offset = q.offset.unwrap_or(0);
    let limit = q.limit.unwrap_or(20).clamp(1, 100);
    let (seasons, total) = app.store.list_seasons(offset, limit).await.map_err(ApiError::internal)?;
    Ok(Json(crate::league::SeasonList { total, offset, seasons }))
}

/// `GET /api/seasons/current?offset=0&limit=10&player=N`: the ranked season
/// under way, standings so far.
async fn season_current(
    State(app): State<AppState>,
    Query(q): Query<SeasonPaging>,
) -> Result<Json<crate::league::CurrentSeason>, ApiError> {
    let (offset, limit) = crate::league::paging(q.offset, q.limit);
    let (reply, rx) = oneshot::channel();
    send_request(&app.world, Request::Season { offset, limit, player: q.player, reply })?;
    Ok(Json(*rx.await.map_err(ApiError::internal)?))
}

/// `GET /api/seasons/:n?world=ID&offset=0&limit=10&player=N`: one finished
/// season with every category's standings, a page at a time.
async fn season_one(
    State(app): State<AppState>,
    UrlPath(season): UrlPath<String>,
    Query(q): Query<SeasonPaging>,
) -> Result<Json<crate::league::SeasonPage>, ApiError> {
    let season: u32 = season
        .parse()
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "bad_season", "a season is a positive number"))?;
    let world = q.world.unwrap_or_else(|| app.world_id.to_string());
    let (offset, limit) = crate::league::paging(q.offset, q.limit);
    let record = app.store.load_season(&world, season).await.map_err(ApiError::internal)?.ok_or_else(|| {
        ApiError::new(StatusCode::NOT_FOUND, "no_season", format!("season {season} of world {world} has not ended"))
    })?;
    let standings = crate::league::page(&record.standings, offset, limit, q.player);
    Ok(Json(crate::league::SeasonPage { head: record.head, offset, limit, standings }))
}

#[derive(Deserialize)]
struct PlayerSeasonsQuery {
    limit: Option<usize>,
}

/// `GET /api/seasons/player/:player?limit=50`: the finished seasons a
/// player was ranked in, newest first, with every placement.
async fn season_player(
    State(app): State<AppState>,
    UrlPath(player): UrlPath<String>,
    Query(q): Query<PlayerSeasonsQuery>,
) -> Result<Json<crate::league::PlayerHistory>, ApiError> {
    let player: PlayerId =
        player.parse().map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "bad_player", "a player is a number"))?;
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    let seasons = app.store.player_seasons(player, limit).await.map_err(ApiError::internal)?;
    Ok(Json(crate::league::PlayerHistory { player, seasons }))
}

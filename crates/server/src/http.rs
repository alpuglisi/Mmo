//! HTTP routes and the WebSocket session.
//!
//! Routes about one world (founding, the world summary, leaderboards,
//! ranked seasons, terrain, the chronicle, metrics, the WebSocket) take `?world=<id>` and
//! default to the server's first world; an unknown id is a 404
//! (`unknown_world`). Accounts, sessions, alert and push preferences, push
//! subscriptions and the gene catalog are global. `GET /api/worlds` lists the worlds.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::extract::{
    ConnectInfo, DefaultBodyLimit, FromRequestParts, Path as UrlPath, Query, Request as HttpRequest, State,
};
use axum::http::request::Parts;
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
use yawc::frame::OpCode;
use yawc::{Frame, WebSocket};

use evo_sim::PlayerId;

use crate::accounts::{Accounts, AuthError, Session};
use crate::design::{self, DesignSpec, SpecError};
use crate::game::FoundError;
use crate::limits::{client_key, Bucket, Limiters};
use crate::metrics::{Metrics, MetricsView, NetStats};
use crate::protocol::{
    AchievementList, AlertPrefs, CatastropheList, ChronicleView, ClientEnvelope, ClientMsg, DesignCompatView, EraList,
    ErrorInfo, LineageStats, NativeService, PushKind, PushPrefs, SeasonBoard, ServerMsg, SpeciesList, TerrainDynamic,
    TerrainView, WorldList, WorldListing,
};
use crate::runner::{Published, Request, TerrainReply};
use crate::store::{AlertPrefsRecord, DesignRecord, PushPrefsRecord, Store, WorldStore};
use crate::webpush::service::SubscribeError;
use crate::webpush::PushService;
use crate::wire::{DeltaEncoder, KEYFRAME_EVERY, PROTO_DELTA, PROTO_JSON, PROTO_MAX};

/// Largest WebSocket message a client may send as a command: a longer one
/// is answered `too_large` (with its id when it can be read) and dropped,
/// and the connection stays open.
pub const MAX_CLIENT_MESSAGE: usize = 16 * 1024;
/// Largest message (and frame) the socket reads at all: past it the
/// connection closes. Well above [`MAX_CLIENT_MESSAGE`], so an oversized
/// command gets a reply rather than a silent close (playtest 5: 49 KB
/// closed the socket with code 1000 and dropped a queued reply).
pub const MAX_WS_READ: usize = 256 * 1024;
/// Per-connection queue of outgoing messages.
const CONN_QUEUE: usize = 256;
/// Largest HTTP request body (credentials, a founder design).
pub const MAX_BODY: usize = 64 * 1024;
/// Most WebSocket connections one player may hold open at once (tabs,
/// devices); each costs the world thread a view and a state per interval.
pub const MAX_CONNS_PER_PLAYER: usize = 8;
/// Longest `?map=` id of `GET /api/seasons/:n` (they are 16 hex digits).
pub const MAX_MAP_ID: usize = 64;
/// Longest side of `GET /api/map.png`, in pixels: the scale is lowered to
/// fit (8 pixels per tile on a 1024-tile map would be a 200 MB image).
pub const MAX_MAP_SIDE: usize = 2048;

#[derive(Clone)]
pub struct AppState {
    /// The hosted worlds, the default first (never empty).
    pub worlds: Arc<Vec<WorldHandle>>,
    pub accounts: Arc<Accounts>,
    /// The whole store (global records: alert preferences).
    pub store: Arc<dyn Store>,
    /// Connection ids, unique across worlds.
    pub next_conn: Arc<AtomicU64>,
    pub limiters: Arc<Limiters>,
    /// Open WebSocket connections per player, over every world.
    pub ws_conns: Arc<Mutex<HashMap<PlayerId, usize>>>,
    /// The push notification service (`None` with `--no-push`).
    pub push: Option<Arc<PushService>>,
    /// The game WebSocket's settings: message limits and permessage-deflate
    /// ([`ws_options`]).
    pub ws: yawc::Options,
}

/// How the game WebSocket compresses (`--ws-compression`): with
/// permessage-deflate (RFC 7692), which every browser and the Capacitor
/// apps' web views offer. A client that doesn't offer it gets plain frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WsCompression {
    Off,
    /// Each message compressed on its own (no context takeover) with
    /// deflate state shared by all connections: little memory per
    /// connection, but small messages (most deltas) barely shrink.
    Message,
    /// Each connection keeps its deflate stream (context takeover): later
    /// messages refer back to earlier ones, so repetitive deltas and replies
    /// shrink the most, at about 300 KB of deflate state per connection.
    #[default]
    Context,
}

impl std::str::FromStr for WsCompression {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "off" => Ok(Self::Off),
            "message" => Ok(Self::Message),
            "context" => Ok(Self::Context),
            other => Err(format!("unknown WebSocket compression {other:?} (off, message, context)")),
        }
    }
}

/// The game WebSocket's options: client messages bounded by
/// [`MAX_WS_READ`] (frames and whole messages, compressed or not; commands
/// by [`MAX_CLIENT_MESSAGE`]),
/// and compression as configured. Messages under 64 bytes go out plain
/// (deflate's framing would make them longer).
pub fn ws_options(compression: WsCompression) -> yawc::Options {
    let base = yawc::Options::default().with_limits(MAX_WS_READ, MAX_WS_READ).with_utf8();
    match compression {
        WsCompression::Off => base.without_compression(),
        WsCompression::Message => base
            .with_low_latency_compression()
            .server_no_context_takeover()
            .client_no_context_takeover()
            .with_compression_threshold(64)
            // About the number of runtime threads compressing at once.
            .with_deflate_pool(yawc::DeflatePool::new(64)),
        WsCompression::Context => base.with_low_latency_compression().with_compression_threshold(64),
    }
}

/// One hosted world: its world thread, what it publishes, its metrics and
/// traffic, and its view of the store.
#[derive(Clone)]
pub struct WorldHandle {
    pub id: Arc<str>,
    pub name: Arc<str>,
    pub tx: SyncSender<Request>,
    pub published: watch::Receiver<Arc<Published>>,
    pub metrics: Arc<Mutex<Metrics>>,
    pub net: Arc<NetStats>,
    pub store: WorldStore,
    /// The id of the world's generated map at start-up (`Game::world_id`):
    /// `/api/seasons/:n` means its seasons unless the request names another
    /// map of the world (`?map=`, a world started over with `--fresh`).
    pub map_id: Arc<str>,
    /// Players online before `GET /api/assign` sends new players elsewhere.
    pub capacity: usize,
}

impl AppState {
    /// The world `id`, or the default one when none is named.
    pub fn world(&self, id: Option<&str>) -> Result<&WorldHandle, ApiError> {
        match id.filter(|s| !s.is_empty()) {
            None => Ok(&self.worlds[0]),
            Some(id) => self.worlds.iter().find(|w| &*w.id == id).ok_or_else(|| {
                let mut e = ApiError::new(StatusCode::NOT_FOUND, "unknown_world", format!("no world {id:?} here"));
                e.detail =
                    Some(serde_json::json!({ "worlds": self.worlds.iter().map(|w| &*w.id).collect::<Vec<_>>() }));
                e
            }),
        }
    }
}

#[derive(Deserialize)]
struct WorldQuery {
    world: Option<String>,
}

/// The world a request is about: `?world=<id>`, the default without one.
pub struct World(pub WorldHandle);

#[axum::async_trait]
impl FromRequestParts<AppState> for World {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, app: &AppState) -> Result<Self, ApiError> {
        let q: Query<WorldQuery> = Query::try_from_uri(&parts.uri)
            .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, "bad_query", e.body_text()))?;
        Ok(World(app.world(q.world.as_deref())?.clone()))
    }
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
    let app = match static_dir {
        Some(dir) => api.fallback_service(ServeDir::new(dir).fallback(ServeFile::new(dir.join("index.html")))),
        None => api,
    };
    app.layer(middleware::map_response(security_headers))
}

/// `Content-Security-Policy` of every response: the page may not be framed
/// (clickjacking), load plugins or change its base URL. Scripts, styles and
/// connections are not restricted here (PixiJS compiles shaders with `new
/// Function` unless built with its `unsafe-eval` shim).
pub const CSP: &str = "frame-ancestors 'none'; object-src 'none'; base-uri 'self'";

/// Headers on every response, pages and API alike (unless a handler set
/// its own): no MIME sniffing, no framing, no referrer to other sites.
async fn security_headers(mut res: Response) -> Response {
    let h = res.headers_mut();
    for (name, value) in [
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::X_FRAME_OPTIONS, "DENY"),
        (header::REFERRER_POLICY, "no-referrer"),
        (header::CONTENT_SECURITY_POLICY, CSP),
    ] {
        h.entry(name).or_insert(header::HeaderValue::from_static(value));
    }
    res
}

fn api_router(state: AppState) -> Router {
    // Requests that cost the world thread or the store: rate-limited per
    // client. Founding and opening a WebSocket (the world thread builds a
    // full state for it) too.
    let reads = Router::new()
        .route("/api/lineage", post(found_lineage))
        .route("/api/design/compatibility", post(design_compatibility))
        .route("/api/design/last", get(last_design))
        .route("/ws", get(ws_upgrade))
        .route("/api/leaderboard/history", get(leaderboard_history))
        .route("/api/leaderboard/seasons", get(leaderboard_seasons))
        .route("/api/lineage/stats", get(lineage_stats))
        .route("/api/matings/history", get(mating_history))
        .route("/api/lineage/tree", get(family_tree))
        .merge(e2e_hooks())
        .route("/api/alerts/prefs", get(alert_prefs).post(set_alert_prefs))
        .route("/api/push/subscriptions", get(push_subscriptions))
        .route("/api/push/subscribe", post(push_subscribe))
        .route("/api/push/unsubscribe", post(push_unsubscribe))
        .route("/api/push/native/subscribe", post(push_native_subscribe))
        .route("/api/push/native/unsubscribe", post(push_native_unsubscribe))
        .route("/api/push/prefs", get(push_prefs).post(set_push_prefs))
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
        .route("/api/spectate", get(spectate))
        .route_layer(middleware::from_fn_with_state(state.clone(), limit_reads));
    Router::new()
        .route("/api/register", post(register))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/genes", get(genes))
        .route("/api/push/key", get(push_key))
        .route("/api/worlds", get(world_list))
        .route("/api/assign", get(assign))
        .route("/api/world", get(world_summary))
        .route("/api/leaderboard", get(leaderboard))
        .route("/api/metrics", get(metrics))
        .merge(reads)
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
    match app.limiters.reads.check(&client_ip(&app, &info, req.headers())) {
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

    /// A request without a known session token. Sessions end on logout,
    /// when idle or too old, and when the server restarts without a clean
    /// stop (it saves them only then): the message says to log in again,
    /// for API clients that don't know (the web client already does).
    fn unauthorized() -> Self {
        let mut e = Self::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "missing or unknown session token: please log in again (POST /api/login). Sessions end on logout, \
             after being idle, and when the server restarts.",
        );
        e.detail = Some(serde_json::json!({ "relogin": true }));
        e
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
/// unspecified when the server runs without connect info). Behind a trusted
/// reverse proxy (`--trusted-proxy`) the IP is the one the proxy puts in
/// `X-Forwarded-For`.
fn client_ip(app: &AppState, info: &Option<ConnectInfo<SocketAddr>>, headers: &HeaderMap) -> IpAddr {
    let peer = info.as_ref().map_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED), |c| c.0.ip());
    let forwarded = headers.get_all("x-forwarded-for").iter().filter_map(|v| v.to_str().ok());
    client_key(app.limiters.proxies.client_addr(peer, forwarded))
}

impl From<AuthError> for ApiError {
    fn from(e: AuthError) -> Self {
        if let AuthError::Busy(retry) = e {
            // Like `rate_limited`: `retry_after_ms` and a Retry-After header.
            let mut err = ApiError::new(StatusCode::SERVICE_UNAVAILABLE, e.code(), e.message());
            err.detail = Some(serde_json::json!({ "retry_after_ms": retry.as_millis().max(1) as u64 }));
            return err;
        }
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
    headers: HeaderMap,
    Json(c): Json<Credentials>,
) -> Result<(StatusCode, Json<AuthResponse>), ApiError> {
    app.limiters.logins.check(&client_ip(&app, &info, &headers)).map_err(ApiError::rate_limited)?;
    let (token, s) = app.accounts.register(c.name.trim(), &c.password).await?;
    Ok((StatusCode::CREATED, Json(AuthResponse { token, player: s.player, name: s.name })))
}

async fn login(
    State(app): State<AppState>,
    info: Option<ConnectInfo<SocketAddr>>,
    headers: HeaderMap,
    Json(c): Json<Credentials>,
) -> Result<Json<AuthResponse>, ApiError> {
    app.limiters.logins.check(&client_ip(&app, &info, &headers)).map_err(ApiError::rate_limited)?;
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
    /// Points the founding cost (with the founders it got).
    pub cost: f32,
    /// Founders the design asked for: more than `founders` when it was a
    /// re-founding soon after the player's last lineage ended.
    #[serde(default)]
    pub requested: u32,
    /// Why fewer were founded, if they were.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reduced: Option<String>,
    /// Renown the re-founding cost (0 outside the window).
    #[serde(default)]
    pub refound_fee: u64,
    /// Whether the lineage has a newcomer grace, and why (or why not).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grace: Option<String>,
}

async fn found_lineage(
    State(app): State<AppState>,
    World(w): World,
    headers: HeaderMap,
    spec: Result<Json<DesignSpec>, axum::extract::rejection::JsonRejection>,
) -> Result<(StatusCode, Json<FoundResponse>), ApiError> {
    let session = authed(&app, &headers)?;
    let spec = design_body(spec)?;
    let design = spec.to_design()?;
    let (reply, rx) = oneshot::channel();
    send_request(&w.tx, Request::Found { player: session.player, name: session.name.clone(), design, reply })?;
    let ok = match rx.await.map_err(ApiError::internal)? {
        Ok(ok) => ok,
        Err(FoundError::AlreadyFounded { lineage }) => {
            const STILL_ALIVE: &str =
                "your lineage in this world is still alive (organisms or eggs): a new one can be founded once it has died out";
            let mut e = ApiError::new(StatusCode::CONFLICT, "already_founded", STILL_ALIVE);
            e.detail = Some(serde_json::json!({ "lineage": lineage }));
            return Err(e);
        }
        Err(FoundError::Design(d)) => return Err(SpecError::Design(d).into()),
        Err(FoundError::BadName(why)) => return Err(SpecError::BadName(why).into()),
    };
    let day = w.published.borrow().summary.clock.day;
    let unix = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let record = DesignRecord { player: session.player, design: spec, founded_day: day, founded_unix: unix };
    if let Err(e) = w.store.put_design(&record).await {
        tracing::error!("could not persist design of player {} in world {}: {e}", session.player, w.id);
    }
    Ok((
        StatusCode::CREATED,
        Json(FoundResponse {
            lineage: ok.lineage,
            founders: ok.founders,
            cost: ok.cost,
            requested: ok.requested,
            reduced: ok.reduced,
            refound_fee: ok.refound_fee,
            grace: ok.grace,
        }),
    ))
}

/// A design body, or a plain error when it isn't one (playtest 4: a `NaN`
/// gave the JSON parser's own text): which field is wrong, not how serde
/// says it.
fn design_body(
    body: Result<Json<DesignSpec>, axum::extract::rejection::JsonRejection>,
) -> Result<DesignSpec, ApiError> {
    body.map(|Json(spec)| spec).map_err(|e| {
        // Over the body limit: say that, not that the design is malformed.
        if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
            return ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "too_large",
                format!("that design is too large: a request body goes up to {} KB", MAX_BODY / 1024),
            );
        }
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "bad_design",
            "that design couldn't be read: it needs a name, thermo (warm or cold), birth, litter_size and founders, \
             and every trait value a number from 0 to 1 (no NaN or text)",
        )
    })
}

/// `POST /api/design/compatibility` (logged in, body: a design): who the
/// design's founders could breed with now, from samples of individuals on
/// both sides, with each lineage's fertility range and drift risk
/// ([`DesignCompatView`]). Read only; nothing is founded.
async fn design_compatibility(
    State(app): State<AppState>,
    World(w): World,
    headers: HeaderMap,
    spec: Result<Json<DesignSpec>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<DesignCompatView>, ApiError> {
    let session = authed(&app, &headers)?;
    let spec = design_body(spec)?;
    let design = spec.to_design()?;
    let (reply, rx) = oneshot::channel();
    send_request(&w.tx, Request::DesignCompat { player: session.player, design, reply })?;
    Ok(Json(rx.await.map_err(ApiError::internal)?))
}

async fn genes() -> Json<design::GeneCatalog> {
    Json(design::catalog())
}

/// `GET /api/metrics`: one world's world-thread timings, queues, saves and
/// traffic, and the process's usage ([`MetricsView`]).
async fn metrics(State(app): State<AppState>, World(w): World) -> Json<MetricsView> {
    let tick = w.published.borrow().summary.clock.tick;
    Json(MetricsView::gather(&w.id, &w.metrics, &w.net, tick, app.accounts.hash_stats()))
}

/// `GET /api/worlds`: every hosted world with its clock, population and
/// players online (no login needed: the start screen picks from it).
async fn world_list(State(app): State<AppState>) -> Json<WorldList> {
    let worlds = app
        .worlds
        .iter()
        .map(|w| {
            let s = w.published.borrow().summary.clone();
            WorldListing {
                id: w.id.to_string(),
                name: w.name.to_string(),
                width: s.width,
                height: s.height,
                day: s.clock.day,
                year: s.clock.year,
                season: s.clock.season,
                population: s.organisms,
                lineages: s.lineages,
                players_online: w.metrics.lock().unwrap().players,
            }
        })
        .collect();
    Json(WorldList { default: app.worlds[0].id.to_string(), worlds })
}

#[derive(Deserialize)]
struct AssignQuery {
    /// An account name (the lobby asks for players who haven't logged in
    /// here); a bearer token is used instead when there is one.
    name: Option<String>,
}

/// `GET /api/assign`: the world this server would put a player in
/// (`worlds::assign`): the one holding their lineage, else the busiest one
/// under its fill target, else the emptiest one with room; 503 `full` when
/// every world is full. `evo-lobby` asks every server and picks among them.
async fn assign(
    State(app): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<AssignQuery>,
) -> Result<Json<crate::worlds::AssignView>, ApiError> {
    let player = match bearer(&headers) {
        Some(t) => Some(app.accounts.session(t).ok_or_else(ApiError::unauthorized)?.player),
        None => match q.name.as_deref() {
            Some(n) => app.accounts.player_of(n).await,
            None => None,
        },
    };
    let loads: Vec<crate::worlds::WorldLoad> = app
        .worlds
        .iter()
        .map(|w| crate::worlds::WorldLoad {
            players_online: w.metrics.lock().unwrap().players,
            capacity: w.capacity,
            has_lineage: player.is_some_and(|p| w.published.borrow().leaderboard.rows.iter().any(|r| r.player == p)),
        })
        .collect();
    let Some((i, reason)) = crate::worlds::assign(&loads) else {
        let mut e = ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "full", "every world here is full");
        e.detail = Some(serde_json::json!({ "retry_after_ms": 30_000 }));
        return Err(e);
    };
    let w = &app.worlds[i];
    Ok(Json(crate::worlds::AssignView {
        world: w.id.to_string(),
        name: w.name.to_string(),
        reason,
        players_online: loads[i].players_online,
        capacity: loads[i].capacity,
        has_account: player.is_some(),
    }))
}

async fn world_summary(World(w): World) -> Json<crate::protocol::WorldSummary> {
    Json(w.published.borrow().summary.clone())
}

async fn leaderboard(World(w): World) -> Json<crate::protocol::Leaderboard> {
    Json(w.published.borrow().leaderboard.clone())
}

#[derive(Deserialize)]
struct HistoryQuery {
    limit: Option<usize>,
    /// First and last day (inclusive) of the newest world's records.
    from: Option<u64>,
    to: Option<u64>,
    /// `false` leaves out the lineages' gene means.
    genes: Option<bool>,
}

/// The stored daily records (thinned with age: [`crate::history`]), oldest
/// first: the newest `limit` (30 by default, at most
/// [`crate::history::MAX_LIMIT`]) in the day range.
async fn leaderboard_history(
    State(app): State<AppState>,
    World(w): World,
    headers: HeaderMap,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<Vec<crate::store::DailyRecord>>, ApiError> {
    // Gene means are a design recipe: they need a login, and give the full
    // set only for the caller's own lineages (`public_genes` for others).
    let session = bearer(&headers).and_then(|t| app.accounts.session(t));
    let genes = q.genes.unwrap_or(session.is_some());
    let Some(player) = session.map(|s| s.player).or(if genes { None } else { Some(evo_sim::WILD) }) else {
        return Err(ApiError::unauthorized());
    };
    let query = crate::history::DailyQuery {
        from_day: q.from,
        to_day: q.to,
        // Logged out: at most `PUBLIC_HISTORY_LIMIT` days (a whole history
        // per anonymous request was an easy bandwidth amplifier).
        limit: history_limit(q.limit, player != evo_sim::WILD),
        genes,
    };
    let mut records = w.store.daily_range(&query).await.map_err(ApiError::internal)?;
    for l in records.iter_mut().flat_map(|r| r.lineages.iter_mut()) {
        if l.owner != player {
            public_genes(&mut l.genes);
        }
        // Stored before the name filter: cleaned as they go out.
        if l.owner != evo_sim::WILD {
            let fallback = format!("Lineage {}", l.lineage);
            l.name =
                crate::names::sanitize(&l.name, crate::design::MAX_NAME_LEN, crate::names::Kind::Lineage, &fallback);
        }
    }
    Ok(Json(records))
}

/// Most days `/api/leaderboard/history` gives without a login.
const PUBLIC_HISTORY_LIMIT: usize = 60;

/// The days `/api/leaderboard/history` returns for `?limit=`: 30 by
/// default, at most [`PUBLIC_HISTORY_LIMIT`] logged out and
/// `history::MAX_LIMIT` logged in.
fn history_limit(asked: Option<usize>, logged_in: bool) -> usize {
    let most = if logged_in { crate::history::MAX_LIMIT } else { PUBLIC_HISTORY_LIMIT };
    asked.unwrap_or(30).clamp(1, most)
}

/// Genes anyone can judge by looking at the animals, the only ones
/// `/api/leaderboard/history` gives of other players' lineages.
const PUBLIC_GENES: &[&str] = &["body_size", "leg_length", "fur"];

/// Another player's gene means as `/api/leaderboard/history` gives them:
/// only [`PUBLIC_GENES`], rounded to quarters.
fn public_genes(genes: &mut std::collections::BTreeMap<String, f32>) {
    genes.retain(|name, _| PUBLIC_GENES.contains(&name.as_str()));
    for v in genes.values_mut() {
        *v = (*v * 4.0).round() / 4.0;
    }
}

#[derive(Deserialize)]
struct SeasonsQuery {
    limit: Option<usize>,
}

/// Seasonal leaderboards from the whole stored history, newest first.
async fn leaderboard_seasons(
    State(app): State<AppState>,
    World(w): World,
    Query(q): Query<SeasonsQuery>,
) -> Result<Json<Vec<SeasonBoard>>, ApiError> {
    const MAX_BOARDS: usize = 100;
    let limit = q.limit.unwrap_or(8).clamp(1, MAX_BOARDS);
    let clock = w.published.borrow().summary.clock.clone();
    // Computed once per new daily record or weather season, for the most
    // boards anyone may ask for; a request takes the newest `limit`.
    let boards = w
        .store
        .cache()
        .weather(clock.year, &clock.season.clone(), || async {
            // The whole stored history (bounded by its retention), without
            // the gene means the boards don't use.
            let query = crate::history::DailyQuery { genes: false, ..crate::history::DailyQuery::recent(usize::MAX) };
            let records = w.store.daily_range(&query).await.map_err(ApiError::internal)?;
            let accounts = app.accounts.clone();
            let name_of = move |p: evo_sim::PlayerId| {
                if p == evo_sim::WILD {
                    "wild".to_string()
                } else {
                    accounts.name_of(p).unwrap_or_else(|| format!("player {p}"))
                }
            };
            tokio::task::spawn_blocking(move || {
                crate::seasons::season_boards(&records, Some((clock.year, &clock.season)), name_of, MAX_BOARDS)
            })
            .await
            .map_err(ApiError::internal)
        })
        .await?;
    Ok(Json(boards.iter().take(limit).cloned().collect()))
}

async fn lineage_stats(
    State(app): State<AppState>,
    World(w): World,
    headers: HeaderMap,
) -> Result<Json<LineageStats>, ApiError> {
    let session = authed(&app, &headers)?;
    let (reply, rx) = oneshot::channel();
    send_request(&w.tx, Request::Stats { player: session.player, reply })?;
    Ok(Json(rx.await.map_err(ApiError::internal)?))
}

/// The family tree of the caller's lineage (`FamilyTree`).
async fn family_tree(
    State(app): State<AppState>,
    World(w): World,
    headers: HeaderMap,
) -> Result<Json<crate::protocol::FamilyTree>, ApiError> {
    let session = authed(&app, &headers)?;
    let (reply, rx) = oneshot::channel();
    send_request(&w.tx, Request::FamilyTree { player: session.player, reply })?;
    Ok(Json(rx.await.map_err(ApiError::internal)?))
}

/// The caller's recent finished matings, newest first (closed offers with
/// their outcome or error, and matings within their own lineage). Kept in
/// memory: a reload of the page keeps it, a server restart does not.
/// `GET /api/design/last`: the design of the caller's last founding in this
/// world, as founded (the designer starts from it when the browser has none
/// saved); 404 `no_design` before a first founding.
async fn last_design(
    State(app): State<AppState>,
    World(w): World,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let session = authed(&app, &headers)?;
    let designs = w.store.load_designs().await.map_err(ApiError::internal)?;
    let d = designs.into_iter().find(|d| d.player == session.player).ok_or_else(|| {
        ApiError::new(StatusCode::NOT_FOUND, "no_design", "you have not founded a lineage in this world")
    })?;
    Ok(Json(serde_json::json!({ "design": d.design, "founded_day": d.founded_day })))
}

async fn mating_history(
    State(app): State<AppState>,
    World(w): World,
    headers: HeaderMap,
) -> Result<Json<Vec<crate::protocol::MatingRecord>>, ApiError> {
    let session = authed(&app, &headers)?;
    let (reply, rx) = oneshot::channel();
    send_request(&w.tx, Request::MatingHistory { player: session.player, reply })?;
    Ok(Json(rx.await.map_err(ApiError::internal)?))
}

/// Test-only routes, mounted only when the server runs with
/// `EVO_E2E_HOOKS=1` (the client's e2e scripts set it): `POST
/// /api/e2e/wipe-out` kills the caller's lineage at once, so re-founding can
/// be tested without waiting for a real extinction, and `POST
/// /api/e2e/catastrophe {"kind": "severe_winter", "days": 8}` starts a
/// catastrophe today (the marketing capture script, tools/marketing).
/// Absent (404) otherwise.
fn e2e_hooks() -> Router<AppState> {
    if std::env::var("EVO_E2E_HOOKS").as_deref() == Ok("1") {
        tracing::warn!("EVO_E2E_HOOKS=1: test-only routes are mounted (never set this in production)");
        Router::new()
            .route("/api/e2e/wipe-out", post(e2e_wipe_out))
            .route("/api/e2e/catastrophe", post(e2e_catastrophe))
    } else {
        Router::new()
    }
}

async fn e2e_wipe_out(
    State(app): State<AppState>,
    World(w): World,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let session = authed(&app, &headers)?;
    let (reply, rx) = oneshot::channel();
    send_request(&w.tx, Request::E2eWipeOut { player: session.player, reply })?;
    let killed = rx.await.map_err(ApiError::internal)?;
    Ok(Json(serde_json::json!({ "killed": killed })))
}

#[derive(Deserialize)]
struct CatastropheAsk {
    /// `severe_winter`, `megadrought`, `epidemic` or `flood_year`.
    kind: String,
    /// Its length in days (default: its usual length).
    days: Option<u64>,
}

async fn e2e_catastrophe(
    World(w): World,
    Json(ask): Json<CatastropheAsk>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let kind = evo_sim::chronicle::CatastropheKind::from_label(&ask.kind).ok_or_else(|| {
        ApiError::new(StatusCode::BAD_REQUEST, "bad_kind", "kind: severe_winter, megadrought, epidemic or flood_year")
    })?;
    let (reply, rx) = oneshot::channel();
    send_request(&w.tx, Request::E2eCatastrophe { kind, days: ask.days, reply })?;
    let id = rx.await.map_err(ApiError::internal)?;
    Ok(Json(serde_json::json!({ "id": id })))
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
    // Hidden alert kinds are not pushed either.
    if let Some(p) = &app.push {
        p.invalidate(session.player);
    }
    Ok(Json(prefs))
}

/// `GET /api/push/key`: whether push notifications are on, the VAPID public
/// key a browser subscribes with, the push kinds, and the native push
/// services the apps can register with (left out when none is configured).
#[derive(Serialize)]
struct PushKeyView {
    enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    public_key: Option<String>,
    kinds: Vec<PushKind>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    native: Vec<NativeService>,
}

async fn push_key(State(app): State<AppState>) -> Json<PushKeyView> {
    Json(PushKeyView {
        enabled: app.push.is_some(),
        public_key: app.push.as_ref().map(|p| p.public_key().to_string()),
        kinds: PushKind::ALL.to_vec(),
        native: app.push.as_ref().map(|p| p.native_services()).unwrap_or_default(),
    })
}

fn push_service(app: &AppState) -> Result<&Arc<PushService>, ApiError> {
    app.push.as_ref().ok_or_else(|| {
        ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "push_disabled", "push notifications are off on this server")
    })
}

/// One of the caller's devices, as listed (the keys stay on the server).
#[derive(Serialize)]
struct PushSubscriptionView {
    endpoint: String,
    created_unix: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
}

/// One of the caller's app devices, as listed (the token stays on the
/// server).
#[derive(Serialize)]
struct PushDeviceView {
    service: NativeService,
    created_unix: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
}

#[derive(Serialize)]
struct PushSubscriptionList {
    subscriptions: Vec<PushSubscriptionView>,
    /// App devices (native push), left out when there are none.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    devices: Vec<PushDeviceView>,
}

/// `GET /api/push/subscriptions`: the caller's subscribed devices, oldest first.
async fn push_subscriptions(
    State(app): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<PushSubscriptionList>, ApiError> {
    let session = authed(&app, &headers)?;
    let subs = app.store.load_push_subscriptions(session.player).await.map_err(ApiError::internal)?;
    let subscriptions = subs
        .into_iter()
        .map(|s| PushSubscriptionView { endpoint: s.endpoint, created_unix: s.created_unix, label: s.label })
        .collect();
    let devices = app.store.load_push_devices(session.player).await.map_err(ApiError::internal)?;
    let devices = devices
        .into_iter()
        .map(|d| PushDeviceView { service: d.service, created_unix: d.created_unix, label: d.label })
        .collect();
    Ok(Json(PushSubscriptionList { subscriptions, devices }))
}

#[derive(Deserialize)]
struct PushKeys {
    p256dh: String,
    auth: String,
}

/// `POST /api/push/subscribe`: a browser's `PushSubscription.toJSON()`
/// (`expirationTime` ignored), plus an optional device label.
#[derive(Deserialize)]
struct SubscribeBody {
    endpoint: String,
    keys: PushKeys,
    #[serde(default)]
    label: Option<String>,
}

async fn push_subscribe(
    State(app): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SubscribeBody>,
) -> Result<Json<PushSubscriptionView>, ApiError> {
    let session = authed(&app, &headers)?;
    let push = push_service(&app)?;
    let label = body.label.or_else(|| headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).map(String::from));
    match push.subscribe(session.player, &body.endpoint, &body.keys.p256dh, &body.keys.auth, label).await {
        Ok(s) => Ok(Json(PushSubscriptionView { endpoint: s.endpoint, created_unix: s.created_unix, label: s.label })),
        Err(e) => Err(subscribe_error(e)),
    }
}

fn subscribe_error(e: SubscribeError) -> ApiError {
    match e {
        SubscribeError::Invalid(why) => ApiError::new(StatusCode::BAD_REQUEST, "bad_subscription", why),
        SubscribeError::Unavailable(why) => ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "push_disabled", why),
        SubscribeError::Store(e) => ApiError::internal(e),
    }
}

/// `POST /api/push/native/subscribe`: a native app's device token (from
/// `@capacitor/push-notifications`): `fcm` for Android, `apns` for iOS.
#[derive(Deserialize)]
struct NativeSubscribeBody {
    service: NativeService,
    token: String,
    #[serde(default)]
    label: Option<String>,
}

async fn push_native_subscribe(
    State(app): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<NativeSubscribeBody>,
) -> Result<Json<PushDeviceView>, ApiError> {
    let session = authed(&app, &headers)?;
    let push = push_service(&app)?;
    match push.register_device(session.player, body.service, &body.token, body.label).await {
        Ok(d) => Ok(Json(PushDeviceView { service: d.service, created_unix: d.created_unix, label: d.label })),
        Err(e) => Err(subscribe_error(e)),
    }
}

#[derive(Deserialize)]
struct NativeUnsubscribeBody {
    service: NativeService,
    token: String,
}

/// `POST /api/push/native/unsubscribe`: forget one of the caller's app
/// devices (also while push is off).
async fn push_native_unsubscribe(
    State(app): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<NativeUnsubscribeBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let session = authed(&app, &headers)?;
    let removed = match &app.push {
        Some(p) => p.unregister_device(session.player, body.service, &body.token).await,
        None => {
            crate::webpush::service::unregister_device(&*app.store, session.player, body.service, &body.token).await
        }
    }
    .map_err(ApiError::internal)?;
    Ok(Json(serde_json::json!({ "removed": removed })))
}

#[derive(Deserialize)]
struct UnsubscribeBody {
    endpoint: String,
}

/// `POST /api/push/unsubscribe`: forget one of the caller's devices.
async fn push_unsubscribe(
    State(app): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<UnsubscribeBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let session = authed(&app, &headers)?;
    let removed = match &app.push {
        Some(p) => p.unsubscribe(session.player, &body.endpoint).await,
        // Push off: still let a player clean up.
        None => match app.store.load_push_subscriptions(session.player).await {
            Ok(subs) if subs.iter().any(|s| s.endpoint == body.endpoint) => {
                app.store.delete_push_subscription(&body.endpoint).await
            }
            Ok(_) => Ok(false),
            Err(e) => Err(e),
        },
    }
    .map_err(ApiError::internal)?;
    Ok(Json(serde_json::json!({ "removed": removed })))
}

/// The caller's push preferences (every kind on until they save some).
async fn push_prefs(State(app): State<AppState>, headers: HeaderMap) -> Result<Json<PushPrefs>, ApiError> {
    let session = authed(&app, &headers)?;
    let stored = app.store.load_push_prefs(session.player).await.map_err(ApiError::internal)?;
    Ok(Json(stored.map(|r| r.prefs.normalized()).unwrap_or_default()))
}

/// Replace the caller's push preferences; replies with them normalized.
async fn set_push_prefs(
    State(app): State<AppState>,
    headers: HeaderMap,
    Json(prefs): Json<PushPrefs>,
) -> Result<Json<PushPrefs>, ApiError> {
    let session = authed(&app, &headers)?;
    let prefs = prefs.normalized();
    let stored = app.store.load_push_prefs(session.player).await.map_err(ApiError::internal)?;
    if stored.map(|r| r.prefs.normalized()) != Some(prefs.clone()) {
        let record = PushPrefsRecord { player: session.player, prefs: prefs.clone() };
        app.store.put_push_prefs(&record).await.map_err(ApiError::internal)?;
    }
    if let Some(p) = &app.push {
        p.invalidate(session.player);
    }
    Ok(Json(prefs))
}

async fn terrain_request(w: &WorldHandle, full: bool) -> Result<TerrainReply, ApiError> {
    let (reply, rx) = oneshot::channel();
    send_request(&w.tx, Request::Terrain { full, reply })?;
    rx.await.map_err(ApiError::internal)
}

const NO_STORE: [(header::HeaderName, &str); 1] = [(header::CACHE_CONTROL, "no-store")];

async fn terrain(World(w): World) -> Result<Response, ApiError> {
    let TerrainReply::Full(raw) = terrain_request(&w, true).await? else {
        return Err(ApiError::internal("wrong terrain reply"));
    };
    let view: TerrainView = tokio::task::spawn_blocking(move || raw.encode()).await.map_err(ApiError::internal)?;
    Ok((NO_STORE, Json(view)).into_response())
}

async fn terrain_dynamic(World(w): World) -> Result<Response, ApiError> {
    let TerrainReply::Dynamic(raw) = terrain_request(&w, false).await? else {
        return Err(ApiError::internal("wrong terrain reply"));
    };
    let view: TerrainDynamic = tokio::task::spawn_blocking(move || raw.encode()).await.map_err(ApiError::internal)?;
    Ok((NO_STORE, Json(view)).into_response())
}

#[derive(Deserialize)]
struct MapQuery {
    scale: Option<usize>,
}

async fn map_png(
    State(app): State<AppState>,
    World(w): World,
    headers: HeaderMap,
    Query(q): Query<MapQuery>,
) -> Result<Response, ApiError> {
    // The caller's own organisms and those within their sight: anyone could
    // otherwise see every player's animals.
    let session = authed(&app, &headers)?;
    let side = {
        let s = &w.published.borrow().summary;
        s.width.max(s.height)
    };
    let scale = map_scale(q.scale, side);
    let (reply, rx) = oneshot::channel();
    send_request(&w.tx, Request::Snapshot { scale, player: session.player, reply })?;
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
    /// binary delta states (`wire.rs`), 3 those plus its own organisms'
    /// heading and eyes (vision cones).
    proto: Option<u32>,
}

/// `GET /ws?token=…` (browsers can't set headers on a WebSocket) or with an
/// `Authorization: Bearer …` header; `&proto=2` asks for delta states
/// (`&proto=3` with headings and vision, the highest this server speaks) and
/// `&world=<id>` picks the world (the default one without it).
async fn ws_upgrade(
    State(app): State<AppState>,
    World(world): World,
    info: Option<ConnectInfo<SocketAddr>>,
    Query(q): Query<WsQuery>,
    headers: HeaderMap,
    mut request: axum::extract::Request,
) -> Result<Response, ApiError> {
    // Authenticate before looking at the upgrade, so a bad token is a 401.
    let token = q.token.as_deref().or_else(|| bearer(&headers)).ok_or_else(ApiError::unauthorized)?;
    let session = app.accounts.session(token).ok_or_else(ApiError::unauthorized)?;
    if request.method() != axum::http::Method::GET {
        return Err(ApiError::new(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed", "a WebSocket opens with GET"));
    }
    let slot = ConnSlot::take(&app.ws_conns, session.player).ok_or_else(|| {
        ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_connections",
            format!("at most {MAX_CONNS_PER_PLAYER} connections per player"),
        )
    })?;
    let token = token.to_string();
    let ip = client_ip(&app, &info, &headers);
    let asked = q.proto.unwrap_or(PROTO_JSON);
    let proto = if asked >= PROTO_DELTA { asked.min(PROTO_MAX) } else { PROTO_JSON };
    // Negotiates permessage-deflate when the client offers it.
    let (response, upgrade) = WebSocket::upgrade_with_options(&mut request, app.ws.clone())
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, "bad_upgrade", format!("not a WebSocket upgrade: {e}")))?;
    tokio::spawn(async move {
        if let Ok(socket) = upgrade.await {
            client_session(socket, app, world, session, token, ip, proto).await;
        }
        drop(slot);
    });
    Ok(response.map(|_| axum::body::Body::empty()))
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

async fn client_session(
    socket: WebSocket<yawc::HttpStream>,
    app: AppState,
    world: WorldHandle,
    session: Session,
    token: String,
    ip: IpAddr,
    proto: u32,
) {
    let conn = app.next_conn.fetch_add(1, Ordering::Relaxed);
    let (tx, mut rx) = crate::push::channel(CONN_QUEUE);
    // The reader only holds a weak handle, so when the world thread drops a
    // connection (too slow), the writer ends and the socket closes.
    let weak = tx.downgrade();
    let connect = Request::Connect { conn, player: session.player, name: session.name.clone(), tx };
    if world.tx.try_send(connect).is_err() {
        return;
    }
    let (mut sink, mut stream) = socket.split();
    let net = world.net.clone();
    net.ws_opened.fetch_add(1, Ordering::Relaxed);
    // Protocol 2: the reader asks for a keyframe when the client resyncs.
    let resync = Arc::new(AtomicBool::new(false));
    let wants_key = resync.clone();
    let writer = tokio::spawn(async move {
        let _counted = (proto >= PROTO_DELTA).then(|| DeltaConn::new(&net));
        // States are encoded here, when sent: a delta is against the state
        // this client actually got, whatever the queue replaced meanwhile.
        let mut delta = (proto >= PROTO_DELTA).then(|| DeltaEncoder::for_proto(proto));
        let mut last_key = Instant::now();
        while let Some(mut msg) = rx.recv().await {
            if let ServerMsg::Welcome { proto: p, .. } = &mut msg {
                *p = delta.as_ref().map(DeltaEncoder::proto);
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
                    Frame::binary(bytes)
                }
                _ => match serde_json::to_string(&msg) {
                    Ok(text) => Frame::text(text),
                    Err(_) => continue,
                },
            };
            let len = frame.payload().len();
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
        let Some(frame) = frame else { break };
        // Too long for a command: say so (rate-limited like any message).
        if frame.opcode() == OpCode::Text && frame.payload().len() > MAX_CLIENT_MESSAGE {
            let (id, command) = malformed_id(&frame.payload()[..frame.payload().len().min(512)]);
            let err = match bucket.take(rate, Instant::now()) {
                Ok(()) => ErrorInfo::new(
                    "too_large",
                    format!(
                        "that message is {} KB; commands go up to {} KB",
                        frame.payload().len() / 1024,
                        MAX_CLIENT_MESSAGE / 1024
                    ),
                ),
                Err(wait) => ErrorInfo::new(
                    "rate_limited",
                    format!("too many commands; retry in {} ms", wait.as_millis().max(1)),
                ),
            };
            local(ServerMsg::err(id, command, err));
            continue;
        }
        match frame.opcode() {
            OpCode::Text => match serde_json::from_slice::<ClientEnvelope>(frame.payload()) {
                Ok(env) => {
                    let name = env.msg.name();
                    // A command counts as use of the session.
                    if app.accounts.session(&token).is_none() {
                        local(ServerMsg::SessionEnded { reason: "expired".into() });
                        break;
                    }
                    let now = Instant::now();
                    let group_change = env.msg.is_group_change();
                    if let Err(wait) = bucket
                        .take(rate, now)
                        .and_then(|_| app.limiters.ip_commands.check_at(&ip, now))
                        .and_then(|_| {
                            if group_change {
                                app.limiters.group_commands.check_at(&session.player, now)
                            } else {
                                Ok(())
                            }
                        })
                    {
                        let msg = format!("too many commands; retry in {} ms", wait.as_millis().max(1));
                        local(ServerMsg::err(env.id, name, ErrorInfo::new("rate_limited", msg)));
                        continue;
                    }
                    if env.msg == ClientMsg::Resync {
                        if proto >= PROTO_DELTA {
                            resync.store(true, Ordering::Relaxed);
                            world.net.resyncs.fetch_add(1, Ordering::Relaxed);
                        }
                        continue;
                    }
                    let req = Request::Command { conn, player: session.player, id: env.id, msg: env.msg };
                    match world.tx.try_send(req) {
                        Ok(()) => {}
                        Err(TrySendError::Full(_)) => {
                            local(ServerMsg::err(env.id, name, ErrorInfo::new("busy", "the world is busy; try again")))
                        }
                        Err(TrySendError::Disconnected(_)) => break,
                    }
                }
                // Malformed messages count against the command rate too.
                Err(_) => {
                    // The reply carries the request's id (and names its
                    // command) when they can be read, so the client matches
                    // it instead of timing out; the parser's own text (which
                    // listed every command name) stays on the server.
                    let (id, command) = malformed_id(frame.payload());
                    let err = match bucket.take(rate, Instant::now()) {
                        // JSON has no NaN or Infinity: a client sent a bad number.
                        Ok(()) if non_finite_number(frame.payload()) => {
                            ErrorInfo::new("bad_coordinates", "numbers must be finite (no NaN or Infinity)")
                        }
                        Ok(()) => ErrorInfo::new(
                            "bad_message",
                            "the server didn't understand that command (unknown, or a field is missing or wrong)",
                        ),
                        Err(wait) => ErrorInfo::new(
                            "rate_limited",
                            format!("too many commands; retry in {} ms", wait.as_millis().max(1)),
                        ),
                    };
                    local(ServerMsg::err(id, command, err));
                }
            },
            OpCode::Close => break,
            _ => {}
        }
    }
    // The world thread drops its sender on disconnect, which ends the writer
    // after it has flushed what is queued (e.g. `session_ended`).
    let _ = world.tx.try_send(Request::Disconnect { conn });
    let mut writer = writer;
    if tokio::time::timeout(Duration::from_secs(2), &mut writer).await.is_err() {
        writer.abort();
    }
}

/// The request id and command name of a message that didn't parse as a
/// command, if it is JSON with them: `id` a number, `type` one of the known
/// command names ("message" otherwise, never the client's text).
///
/// Best effort: an id sent as a numeric string (`"id": "12"`) counts, and
/// text that isn't valid JSON is scanned for an `"id": <number>` and a
/// `"type": "<name>"`, so a truncated or mangled request still gets its
/// reply matched.
pub fn malformed_id(text: &[u8]) -> (Option<u64>, &'static str) {
    let command = |t: Option<&str>| t.and_then(crate::protocol::command_name).unwrap_or("message");
    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(text) {
        let id = v.get("id").and_then(|i| i.as_u64().or_else(|| i.as_str().and_then(|s| s.trim().parse().ok())));
        return (id, command(v.get("type").and_then(|t| t.as_str())));
    }
    let text = String::from_utf8_lossy(text);
    // The value after `"key"` and a colon, as far as `stop` allows.
    let after = |key: &str| -> Option<&str> {
        let at = text.find(&format!("\"{key}\""))? + key.len() + 2;
        let rest = text[at..].trim_start().strip_prefix(':')?.trim_start();
        Some(rest)
    };
    let id = after("id").and_then(|r| {
        let r = r.strip_prefix('"').unwrap_or(r);
        let digits: String = r.chars().take_while(char::is_ascii_digit).take(20).collect();
        digits.parse().ok()
    });
    let kind = after("type").and_then(|r| r.strip_prefix('"')).and_then(|r| r.split('"').next());
    (id, command(kind))
}

/// Whether a message that didn't parse has a bare `NaN` or `Infinity` where
/// a number goes (outside strings), as JavaScript's `JSON.stringify` never
/// writes but hand-made messages do.
fn non_finite_number(text: &[u8]) -> bool {
    let mut in_string = false;
    let mut escaped = false;
    for (i, &b) in text.iter().enumerate() {
        if in_string {
            match b {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'N' if text[i..].starts_with(b"NaN") => return true,
            b'I' if text[i..].starts_with(b"Infinity") => return true,
            _ => {}
        }
    }
    false
}

/// The world thread's chronicle (species, eras, catastrophes, achievements).
async fn chronicle(w: &WorldHandle) -> Result<ChronicleView, ApiError> {
    let (reply, rx) = oneshot::channel();
    send_request(&w.tx, Request::Chronicle { reply })?;
    Ok(*rx.await.map_err(ApiError::internal)?)
}

/// `GET /api/spectate`: the read-only spectator view of the world (no login;
/// nothing private: see `game::spectate`), for watch links and streams.
async fn spectate(World(w): World) -> Result<Json<crate::game::Spectate>, ApiError> {
    let (reply, rx) = oneshot::channel();
    send_request(&w.tx, Request::Spectate { reply })?;
    let s = rx.await.map_err(ApiError::internal)?;
    Ok(Json((*s).clone()))
}

/// `GET /api/species`: every detected species (living first, by size), with
/// parents (the species tree), founders and dominant allele owners.
async fn species(World(w): World) -> Result<Json<SpeciesList>, ApiError> {
    Ok(Json(chronicle(&w).await?.species))
}

/// `GET /api/eras`: the era clock and every finished era's final ranking.
async fn eras(World(w): World) -> Result<Json<EraList>, ApiError> {
    Ok(Json(chronicle(&w).await?.eras))
}

/// `GET /api/catastrophes`: the active catastrophe and those before it.
async fn catastrophes(World(w): World) -> Result<Json<CatastropheList>, ApiError> {
    Ok(Json(chronicle(&w).await?.catastrophes))
}

/// `GET /api/achievements`: per-player tallies and the latest achievements.
async fn achievements(World(w): World) -> Result<Json<AchievementList>, ApiError> {
    Ok(Json(chronicle(&w).await?.achievements))
}

#[derive(Deserialize)]
struct SeasonPaging {
    offset: Option<usize>,
    limit: Option<usize>,
    /// Always include this player's row in each category.
    player: Option<PlayerId>,
    /// The generated map of the world whose season is meant (default: the
    /// running one). The world itself is `?world=`, as everywhere.
    map: Option<String>,
}

/// `GET /api/seasons?world=ID&offset=0&limit=20`: the world's finished
/// ranked seasons (of every map it had), newest first (heads: winners and
/// summary, no standings).
async fn seasons_list(
    World(w): World,
    Query(q): Query<SeasonPaging>,
) -> Result<Json<crate::league::SeasonList>, ApiError> {
    let offset = q.offset.unwrap_or(0);
    let limit = q.limit.unwrap_or(20).clamp(1, 100);
    let (seasons, total) = w.store.list_seasons(offset, limit).await.map_err(ApiError::internal)?;
    // The hall of fame counts every finished season, page by page.
    let mut heads = Vec::with_capacity(total);
    let mut at = 0;
    while at < total {
        let (page, _) = w.store.list_seasons(at, 100).await.map_err(ApiError::internal)?;
        if page.is_empty() {
            break;
        }
        at += page.len();
        heads.extend(page);
    }
    let hall_of_fame = crate::league::hall_of_fame(&heads);
    Ok(Json(crate::league::SeasonList { total, offset, seasons, hall_of_fame }))
}

/// `GET /api/seasons/current?offset=0&limit=10&player=N`: the ranked season
/// under way, standings so far: a page of the standings the world thread
/// published when they last changed (`crate::standings`), so a request
/// neither waits for the world thread nor recomputes them.
async fn season_current(
    World(w): World,
    Query(q): Query<SeasonPaging>,
) -> Result<Json<crate::league::CurrentSeason>, ApiError> {
    let (offset, limit) = crate::league::paging(q.offset, q.limit);
    let season = w.published.borrow().season.clone();
    Ok(Json(season.page(offset, limit, q.player)))
}

/// `GET /api/seasons/:n?world=ID&map=ID&offset=0&limit=10&player=N`: one
/// finished season of the world (of its current map unless `map` names an
/// earlier one) with every category's standings, a page at a time.
async fn season_one(
    World(w): World,
    UrlPath(season): UrlPath<String>,
    Query(q): Query<SeasonPaging>,
) -> Result<Json<crate::league::SeasonPage>, ApiError> {
    let season: u32 = season
        .parse()
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "bad_season", "a season is a positive number"))?;
    let map = q.map.unwrap_or_else(|| w.map_id.to_string());
    // Map ids are 16 hex digits; the id keys a cache entry, so a long one
    // would let a client fill the cache with huge keys.
    if map.len() > MAX_MAP_ID {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "bad_map", "not a map id"));
    }
    let (offset, limit) = crate::league::paging(q.offset, q.limit);
    let record = w.store.season_record(&map, season).await.map_err(ApiError::internal)?.ok_or_else(|| {
        let msg = format!("season {season} of world {} (map {map}) has not ended", w.id);
        ApiError::new(StatusCode::NOT_FOUND, "no_season", msg)
    })?;
    let standings = crate::league::page(&record.standings, offset, limit, q.player);
    Ok(Json(crate::league::SeasonPage { head: record.head.clone(), offset, limit, standings }))
}

#[derive(Deserialize)]
struct PlayerSeasonsQuery {
    limit: Option<usize>,
}

/// `GET /api/seasons/player/:player?world=ID&limit=50`: the world's
/// finished seasons a player was ranked in, newest first, with every
/// placement.
async fn season_player(
    World(w): World,
    UrlPath(player): UrlPath<String>,
    Query(q): Query<PlayerSeasonsQuery>,
) -> Result<Json<crate::league::PlayerHistory>, ApiError> {
    let player: PlayerId =
        player.parse().map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "bad_player", "a player is a number"))?;
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    let seasons = w.store.player_seasons(player, limit).await.map_err(ApiError::internal)?;
    Ok(Json(crate::league::PlayerHistory { player, seasons }))
}

#[cfg(test)]
mod tests {
    use super::{history_limit, malformed_id, non_finite_number, public_genes};

    #[test]
    fn bare_nan_and_infinity_are_spotted_outside_strings() {
        assert!(non_finite_number(br#"{"type":"set_view","x":NaN,"y":1,"radius":5}"#));
        assert!(non_finite_number(br#"{"type":"order_move","x":-Infinity}"#));
        assert!(!non_finite_number(br#"{"type":"set_nameplate","title":"NaN \" Infinity"}"#));
        assert!(!non_finite_number(br#"{"type":"set_view","x":1}"#));
    }

    #[test]
    fn the_history_limit_is_capped_tighter_without_a_login() {
        assert_eq!(history_limit(None, false), 30);
        assert_eq!(history_limit(Some(999_999_999), false), 60);
        assert_eq!(history_limit(Some(999_999_999), true), crate::history::MAX_LIMIT);
        assert_eq!(history_limit(Some(0), true), 1);
    }

    #[test]
    fn rivals_gene_means_are_coarse_looks_only() {
        let mut g: std::collections::BTreeMap<String, f32> =
            [("body_size", 0.61), ("birth_mode", 1.0), ("toxins", 0.21), ("fur", 0.13)]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect();
        public_genes(&mut g);
        let want: std::collections::BTreeMap<String, f32> =
            [("body_size".to_string(), 0.5), ("fur".to_string(), 0.25)].into_iter().collect();
        assert_eq!(g, want);
    }

    #[test]
    fn a_malformed_message_gets_its_id_when_it_can_be_read() {
        assert_eq!(malformed_id(br#"{"type":"possess","id":7}"#), (Some(7), "possess"));
        // A numeric string id counts; any other string doesn't.
        assert_eq!(malformed_id(br#"{"type":"possess","id":"12"}"#), (Some(12), "possess"));
        assert_eq!(malformed_id(br#"{"type":"possess","id":"abc"}"#), (None, "possess"));
        // Not JSON at all: scanned.
        assert_eq!(malformed_id(br#"{"id": 80, "type": "migrate", "x": "#), (Some(80), "migrate"));
        assert_eq!(malformed_id(br#"{"type":"bid","id":"9"#), (Some(9), "bid"));
        assert_eq!(malformed_id(b"\xff\xfe garbage"), (None, "message"));
        assert_eq!(malformed_id(br#"{"type":"fly_to_the_moon","id":3,"#), (Some(3), "message"));
        // Ids too long for a u64 are dropped, not wrapped.
        assert_eq!(malformed_id(br#"{"id":99999999999999999999999,"#).0, None);
    }
}

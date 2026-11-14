//! Load and soak test: drive N simulated players against a game server over
//! the real protocol and measure what the server does under them.
//!
//! ```text
//! cargo build --release -p evo-server --bins --examples
//! target/release/examples/loadgen --players 200 --duration 300 --size 512 --warm-until 20000
//! target/release/examples/loadgen --players 500 --duration 1800 --fast --warm-until 20000   # soak
//! target/release/examples/loadgen --url http://host:8080 --players 50 --duration 120         # a running server
//! target/release/examples/loadgen --url http://host:8080 --world isle --players 50           # one of its worlds
//! target/release/examples/loadgen --worlds 2 --players 400 --duration 300 --size 512        # two worlds, one process
//! ```
//!
//! `--world ID` sends every player to that world (`?world=` on the API and
//! the WebSocket) and samples its metrics. `--worlds N` starts the server
//! with N worlds (`main`, `w2`, ... `wN`, seeds `--seed` + i) and spreads
//! the players over them round-robin; every report line then shows each
//! world, and the summary has a `worlds` entry per world.
//!
//! Without `--url` it starts `evo-server` (next to this example in
//! `target/release`, or `--server PATH`) on a free port with a file store in
//! `--data DIR` (default: a new directory under the system temp dir) and
//! `--no-rate-limits` (every player comes from 127.0.0.1). With
//! `--warm-until N` it first runs the server in fast mode until every world
//! under test holds N organisms, stops it (saving) and restarts it from the
//! saves with the test's clock, so the test starts on grown worlds.
//!
//! Each player registers, logs in, founds a lineage over HTTP and opens the
//! game WebSocket, with `--proto 2` (the default, as the web client) for
//! binary delta states, decoded with `wire::DeltaDecoder` (a frame that
//! doesn't decode sends `resync`), or `--proto 1` for whole JSON states. It
//! reads every message (states parsed for its own
//! organisms and the foreign ones in view) and, about `--actions-per-min`
//! times a minute, does what a player does: moves the view, possesses an
//! organism and steers it, requests and previews matings (in its lineage
//! and with others' organisms, some priced), answers mating requests,
//! browses the market (a page of 20, sometimes the next page or another
//! order), lists an adult (at a fixed fee or at auction), buys a listing or
//! bids on an auction, reads its ledger, sets migrations. Every command carries an id; its latency is the time
//! to the matching `result`.
//!
//! Every `--report` seconds it samples the server's `GET /api/metrics`
//! (tick, state-push, sub-step and apply times; queue depth; dropped
//! connections and skipped state frames; saves; bytes out; process CPU and
//! memory) and `GET /api/world`, and prints a line. `--deflate` also runs
//! every state through a per-connection deflate stream to show what
//! permessage-deflate (with context takeover) would save. At the end it prints a
//! summary (and writes it as JSON with `--json FILE`), stops the server
//! gracefully and scans its log for panics.

#![recursion_limit = "256"]

use std::collections::HashMap;
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use evo_server::metrics::{proc_usage, MetricsView};
use evo_server::protocol::StateView;
use evo_server::wire::DeltaDecoder;
use flate2::{Compress, Compression, FlushCompress};
use futures_util::{SinkExt, StreamExt};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::{Method, Request};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::Message;

const USAGE: &str = "usage: loadgen [--players 50] [--duration 120] [--ramp 20] [--actions-per-min 30] \
[--url http://HOST:PORT | --server PATH] [--data DIR] [--size 512] [--seed 1] [--tick-ms 1000] [--state-ms 1000] \
[--fast] [--lod] [--warm-until 0] [--report 10] [--json FILE] [--dump-state FILE] [--founders 12] [--proto 2] [--deflate] \
[--world ID | --worlds N]";

#[derive(Clone, Debug)]
struct Args {
    players: usize,
    duration: f64,
    ramp: f64,
    actions_per_min: f64,
    url: Option<String>,
    server: Option<PathBuf>,
    data: Option<PathBuf>,
    size: usize,
    seed: u64,
    tick_ms: u64,
    state_ms: u64,
    fast: bool,
    lod: bool,
    warm_until: usize,
    report: f64,
    json: Option<PathBuf>,
    dump_state: Option<PathBuf>,
    founders: u32,
    /// The state protocol the players ask for (1 JSON, 2 binary deltas).
    proto: u32,
    /// Also measure what permessage-deflate would make of each state (a
    /// deflate stream per connection, as with context takeover).
    deflate: bool,
    /// The world every player plays in (`?world=`).
    world: Option<String>,
    /// Worlds the started server hosts (players spread over them).
    worlds: usize,
}

impl Args {
    /// The worlds under test: `--world`, the started server's `--worlds`,
    /// or the default world (`None`: no `?world=`).
    fn world_ids(&self) -> Vec<Option<String>> {
        if let Some(w) = &self.world {
            return vec![Some(w.clone())];
        }
        if self.worlds <= 1 {
            return vec![None];
        }
        (0..self.worlds).map(|i| Some(if i == 0 { "main".to_string() } else { format!("w{}", i + 1) })).collect()
    }
}

/// One report of a world other than the first.
#[derive(Clone, Copy)]
struct WorldRow {
    t: f64,
    tick_p50: f32,
    tick_p99: f32,
    states_p99: f32,
    substep_p99: f32,
    ticks_per_s: f64,
    out_mb_s: f64,
    conns: usize,
    organisms: usize,
}

/// `path` with `world=<id>` added.
fn in_world(path: &str, world: &Option<String>) -> String {
    match world {
        None => path.to_string(),
        Some(w) => format!("{path}{}world={w}", if path.contains('?') { '&' } else { '?' }),
    }
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        players: 50,
        duration: 120.0,
        ramp: 20.0,
        actions_per_min: 30.0,
        url: None,
        server: None,
        data: None,
        size: 512,
        seed: 1,
        tick_ms: 1000,
        state_ms: 1000,
        fast: false,
        lod: false,
        warm_until: 0,
        report: 10.0,
        json: None,
        dump_state: None,
        founders: 12,
        proto: 2,
        deflate: false,
        world: None,
        worlds: 1,
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut v = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        fn num<T: std::str::FromStr>(name: &str, s: String) -> Result<T, String> {
            s.parse().map_err(|_| format!("{name}: not a number: {s}"))
        }
        match k.as_str() {
            "--players" => a.players = num("--players", v("--players")?)?,
            "--duration" => a.duration = num("--duration", v("--duration")?)?,
            "--ramp" => a.ramp = num("--ramp", v("--ramp")?)?,
            "--actions-per-min" => a.actions_per_min = num("--actions-per-min", v("--actions-per-min")?)?,
            "--url" => a.url = Some(v("--url")?.trim_end_matches('/').to_string()),
            "--server" => a.server = Some(v("--server")?.into()),
            "--data" => a.data = Some(v("--data")?.into()),
            "--size" => a.size = num("--size", v("--size")?)?,
            "--seed" => a.seed = num("--seed", v("--seed")?)?,
            "--tick-ms" => a.tick_ms = num("--tick-ms", v("--tick-ms")?)?,
            "--state-ms" => a.state_ms = num("--state-ms", v("--state-ms")?)?,
            "--fast" => a.fast = true,
            "--lod" => a.lod = true,
            "--warm-until" => a.warm_until = num("--warm-until", v("--warm-until")?)?,
            "--report" => a.report = num("--report", v("--report")?)?,
            "--json" => a.json = Some(v("--json")?.into()),
            "--dump-state" => a.dump_state = Some(v("--dump-state")?.into()),
            "--founders" => a.founders = num("--founders", v("--founders")?)?,
            "--proto" => a.proto = num("--proto", v("--proto")?)?,
            "--deflate" => a.deflate = true,
            "--world" => a.world = Some(v("--world")?),
            "--worlds" => a.worlds = num("--worlds", v("--worlds")?)?,
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument {other:?}\n{USAGE}")),
        }
    }
    if a.fast {
        a.tick_ms = 20;
        a.state_ms = 100;
    }
    if a.world.is_some() && a.worlds > 1 {
        return Err("--world and --worlds don't mix".into());
    }
    Ok(a)
}

// --- HTTP ------------------------------------------------------------------

#[derive(Clone)]
struct Http {
    base: String,
    client: Client<HttpConnector, Full<Bytes>>,
}

impl Http {
    fn new(base: &str) -> Self {
        let mut conn = HttpConnector::new();
        conn.set_nodelay(true);
        Self { base: base.to_string(), client: Client::builder(TokioExecutor::new()).build(conn) }
    }

    async fn call(
        &self,
        method: Method,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> Result<(u16, Value), String> {
        let mut req = Request::builder().method(method).uri(format!("{}{path}", self.base));
        if let Some(t) = token {
            req = req.header("authorization", format!("Bearer {t}"));
        }
        let body = match body {
            Some(v) => {
                req = req.header("content-type", "application/json");
                Full::new(Bytes::from(v.to_string()))
            }
            None => Full::new(Bytes::new()),
        };
        let res = self.client.request(req.body(body).map_err(|e| e.to_string())?).await.map_err(|e| e.to_string())?;
        let status = res.status().as_u16();
        let bytes = res.into_body().collect().await.map_err(|e| e.to_string())?.to_bytes();
        Ok((status, serde_json::from_slice(&bytes).unwrap_or(Value::Null)))
    }

    async fn get<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Option<T> {
        let (s, v) = self.call(Method::GET, path, None, None).await.ok()?;
        (s == 200).then(|| serde_json::from_value(v).ok()).flatten()
    }
}

// --- The server process ----------------------------------------------------

struct ServerProc {
    child: Child,
    log: PathBuf,
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).map(|a| a.port()).expect("free port")
}

fn server_binary(args: &Args) -> PathBuf {
    if let Some(p) = &args.server {
        return p.clone();
    }
    // target/release/examples/loadgen → target/release/evo-server
    let exe = std::env::current_exe().expect("current exe");
    let dir = exe.parent().and_then(Path::parent).expect("target dir");
    dir.join(if cfg!(windows) { "evo-server.exe" } else { "evo-server" })
}

fn spawn_server(args: &Args, data: &Path, port: u16, fast: bool, log_name: &str) -> ServerProc {
    let bin = server_binary(args);
    let log = data.join(log_name);
    let out = std::fs::File::create(&log).expect("server log");
    let mut cmd = Command::new(&bin);
    cmd.args(["--bind", &format!("127.0.0.1:{port}"), "--data"]).arg(data).args([
        "--size",
        &args.size.to_string(),
        "--seed",
        &args.seed.to_string(),
        "--no-rate-limits",
    ]);
    if fast {
        cmd.arg("--fast");
    } else {
        cmd.args(["--tick-ms", &args.tick_ms.to_string(), "--state-ms", &args.state_ms.to_string()]);
    }
    if args.lod {
        cmd.arg("--lod");
    }
    for (i, w) in args.world_ids().into_iter().enumerate() {
        if let (Some(w), true) = (w, args.worlds > 1) {
            cmd.args(["--world", &format!("{w},seed={}", args.seed + i as u64)]);
        }
    }
    cmd.env("RUST_LOG", "info").stdin(Stdio::null()).stderr(out.try_clone().expect("log handle")).stdout(out);
    let child = cmd
        .spawn()
        .unwrap_or_else(|e| panic!("could not start {}: {e} (cargo build --release -p evo-server)", bin.display()));
    ServerProc { child, log }
}

/// SIGTERM (graceful: the server saves first), then wait for it.
fn stop_server(p: &mut ServerProc) -> bool {
    let _ = Command::new("kill").args(["-TERM", &p.child.id().to_string()]).status();
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        match p.child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
            _ => {
                let _ = p.child.kill();
                return false;
            }
        }
    }
}

async fn wait_up(http: &Http) {
    for _ in 0..600 {
        if http.get::<Value>("/api/world").await.is_some() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("server did not come up");
}

async fn organisms_in(http: &Http, world: &Option<String>) -> usize {
    http.get::<Value>(&in_world("/api/world", world)).await.and_then(|w| w["organisms"].as_u64()).unwrap_or(0) as usize
}

// --- Measurements ----------------------------------------------------------

/// Reservoir of samples (keeps a uniform sample of at most `cap`).
struct Reservoir {
    cap: usize,
    seen: u64,
    v: Vec<f32>,
    rng: StdRng,
}

impl Reservoir {
    fn new(cap: usize) -> Self {
        Self { cap, seen: 0, v: Vec::new(), rng: StdRng::seed_from_u64(7) }
    }

    fn push(&mut self, x: f32) {
        self.seen += 1;
        if self.v.len() < self.cap {
            self.v.push(x);
        } else {
            let k = self.rng.gen_range(0..self.seen);
            if (k as usize) < self.cap {
                self.v[k as usize] = x;
            }
        }
    }

    fn quantiles(&self) -> (f32, f32, f32) {
        if self.v.is_empty() {
            return (0.0, 0.0, 0.0);
        }
        let mut v = self.v.clone();
        v.sort_by(f32::total_cmp);
        let at = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
        (at(0.5), at(0.99), v[v.len() - 1])
    }
}

#[derive(Default)]
struct Stats {
    connected: AtomicUsize,
    founded: AtomicUsize,
    setup_failed: AtomicUsize,
    /// Registrations and logins refused as busy or rate-limited (retried).
    auth_busy: AtomicU64,
    unexpected_close: AtomicU64,
    msgs_in: AtomicU64,
    bytes_in: AtomicU64,
    states_in: AtomicU64,
    state_bytes_in: AtomicU64,
    commands: AtomicU64,
    results_ok: AtomicU64,
    results_err: AtomicU64,
    mating_requests: AtomicU64,
    errors: Mutex<HashMap<String, u64>>,
    latency: Mutex<HashMap<&'static str, Reservoir>>,
    latency_all: Mutex<Option<Reservoir>>,
    /// Command latency by world (the index in `Args::world_ids`).
    latency_world: Mutex<HashMap<usize, Reservoir>>,
    state_sizes: Mutex<Option<Reservoir>>,
    /// The largest state message seen (`--dump-state`).
    largest_state: Mutex<String>,
    /// `--deflate`: state bytes after a per-connection deflate stream.
    state_deflated: AtomicU64,
    /// Protocol 2 frames that did not decode (a resync was sent).
    decode_errors: AtomicU64,
    /// Bytes received by message kind (`state`, `result:<command>`, events).
    bytes_by_kind: Mutex<HashMap<String, u64>>,
}

impl Stats {
    fn latency(&self, world: usize, command: &'static str, ms: f32) {
        self.latency_world.lock().unwrap().entry(world).or_insert_with(|| Reservoir::new(100_000)).push(ms);
        self.latency.lock().unwrap().entry(command).or_insert_with(|| Reservoir::new(20_000)).push(ms);
        self.latency_all.lock().unwrap().get_or_insert_with(|| Reservoir::new(100_000)).push(ms);
    }

    fn kind_bytes(&self, kind: &str, bytes: usize) {
        *self.bytes_by_kind.lock().unwrap().entry(kind.to_string()).or_default() += bytes as u64;
    }

    fn state_size(&self, bytes: usize) {
        self.state_sizes.lock().unwrap().get_or_insert_with(|| Reservoir::new(100_000)).push(bytes as f32);
    }
}

// --- One player ------------------------------------------------------------

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Incoming {
    State(StateLite),
    Result {
        id: Option<u64>,
        ok: bool,
        #[serde(default)]
        error: Option<ErrorLite>,
        #[serde(default)]
        data: Option<Value>,
    },
    MatingRequest {
        offer: OfferLite,
    },
    SessionEnded {},
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct ErrorLite {
    code: String,
}

#[derive(Deserialize)]
struct OfferLite {
    id: u64,
}

#[derive(Deserialize, Default)]
struct StateLite {
    organisms: Vec<OwnLite>,
    nearby: Vec<NearLite>,
}

#[derive(Deserialize, Clone)]
struct OwnLite {
    id: u64,
    x: f32,
    y: f32,
    sex: String,
    matured: bool,
    possessed: bool,
    pregnant: bool,
}

#[derive(Deserialize, Clone)]
struct NearLite {
    id: u64,
    owner: u32,
    sex: String,
    matured: bool,
}

impl From<StateView> for StateLite {
    fn from(s: StateView) -> Self {
        StateLite {
            organisms: s
                .organisms
                .into_iter()
                .map(|o| OwnLite {
                    id: o.id,
                    x: o.x,
                    y: o.y,
                    sex: o.sex.into_owned(),
                    matured: o.matured,
                    possessed: o.possessed,
                    pregnant: o.pregnant,
                })
                .collect(),
            nearby: s
                .nearby
                .into_iter()
                .map(|n| NearLite { id: n.id, owner: n.owner, sex: n.sex.into_owned(), matured: n.matured })
                .collect(),
        }
    }
}

/// Bytes `msg` takes in a deflate stream that carries on from the previous
/// message (permessage-deflate with context takeover: sync flush, trailer
/// dropped).
fn deflated(z: &mut Compress, msg: &[u8]) -> usize {
    let mut out = Vec::with_capacity(msg.len() / 2 + 64);
    let before = z.total_out();
    let mut input = msg;
    loop {
        let consumed = z.total_in();
        out.clear();
        z.compress_vec(input, &mut out, FlushCompress::Sync).expect("deflate");
        input = &input[(z.total_in() - consumed) as usize..];
        if input.is_empty() && out.len() < out.capacity() {
            break;
        }
        out.reserve(out.capacity().max(1024));
    }
    ((z.total_out() - before) as usize).saturating_sub(4)
}

#[derive(Clone)]
struct Listing {
    id: u64,
    owner: u32,
    sex: String,
    busy: bool,
    /// An auction's lowest bid now (none: a fixed fee).
    min_bid: Option<u64>,
}

struct Player {
    player: u32,
    state: StateLite,
    listings: Vec<Listing>,
    /// The last market page's `next_cursor`.
    cursor: Option<String>,
    my_listings: usize,
    pending: HashMap<u64, (Instant, &'static str)>,
    next_id: u64,
    rng: StdRng,
    size: f32,
}

impl Player {
    fn adults(&self, sex: Option<&str>) -> Vec<&OwnLite> {
        self.state.organisms.iter().filter(|o| o.matured && sex.is_none_or(|s| o.sex == s)).collect()
    }

    fn pick<'a, T>(rng: &mut StdRng, v: &'a [T]) -> Option<&'a T> {
        (!v.is_empty()).then(|| &v[rng.gen_range(0..v.len())])
    }

    fn center(&self) -> (f32, f32) {
        let n = self.state.organisms.len().max(1) as f32;
        let (sx, sy) = self.state.organisms.iter().fold((0.0, 0.0), |(x, y), o| (x + o.x, y + o.y));
        if self.state.organisms.is_empty() {
            (self.size / 2.0, self.size / 2.0)
        } else {
            (sx / n, sy / n)
        }
    }

    fn near(&mut self, x: f32, y: f32, d: f32) -> (f32, f32) {
        let s = self.size - 1.0;
        ((x + self.rng.gen_range(-d..d)).clamp(0.0, s), (y + self.rng.gen_range(-d..d)).clamp(0.0, s))
    }

    /// The next command, as (name, message without id).
    fn action(&mut self) -> Option<(&'static str, Value)> {
        let roll = self.rng.gen_range(0..100);
        let (cx, cy) = self.center();
        let possessed = self.state.organisms.iter().find(|o| o.possessed).cloned();
        match roll {
            0..=14 => {
                let (x, y) = self.near(cx, cy, 40.0);
                let r = self.rng.gen_range(12.0..32.0);
                Some(("set_view", json!({"type": "set_view", "x": x, "y": y, "radius": r})))
            }
            15..=17 => Some(("clear_view", json!({"type": "clear_view"}))),
            18..=47 => match possessed {
                Some(o) => {
                    let (x, y) = self.near(o.x, o.y, 12.0);
                    Some(("order_move", json!({"type": "order_move", "organism": o.id, "x": x, "y": y})))
                }
                None => {
                    let all: Vec<u64> = self.state.organisms.iter().map(|o| o.id).collect();
                    let id = *Self::pick(&mut self.rng, &all)?;
                    Some(("possess", json!({"type": "possess", "organism": id})))
                }
            },
            48..=50 => possessed.map(|o| ("release", json!({"type": "release", "organism": o.id}))),
            51..=60 => {
                let males: Vec<u64> = self.adults(Some("male")).iter().map(|o| o.id).collect();
                let females: Vec<u64> =
                    self.adults(Some("female")).iter().filter(|o| !o.pregnant).map(|o| o.id).collect();
                if self.rng.gen_bool(0.6) {
                    let (m, f) = (*Self::pick(&mut self.rng, &males)?, *Self::pick(&mut self.rng, &females)?);
                    Some(("request_mating", json!({"type": "request_mating", "mine": m, "partner": f})))
                } else {
                    // Another player's adult in view, of the other sex.
                    let me = self.player;
                    let foreign: Vec<NearLite> = self
                        .state
                        .nearby
                        .iter()
                        .filter(|n| n.matured && n.owner != me && n.owner != 0)
                        .cloned()
                        .collect();
                    let p = Self::pick(&mut self.rng, &foreign)?.clone();
                    let mine: Vec<u64> =
                        self.adults(None).iter().filter(|o| o.sex != p.sex && !o.pregnant).map(|o| o.id).collect();
                    let m = *Self::pick(&mut self.rng, &mine)?;
                    let price = self.rng.gen_bool(0.3).then_some(5);
                    Some((
                        "request_mating",
                        json!({"type": "request_mating", "mine": m, "partner": p.id, "price": price}),
                    ))
                }
            }
            61..=67 => {
                let mine: Vec<(u64, String)> = self.adults(None).iter().map(|o| (o.id, o.sex.clone())).collect();
                let (m, sex) = Self::pick(&mut self.rng, &mine)?.clone();
                let others: Vec<u64> =
                    self.state.nearby.iter().filter(|n| n.matured && n.sex != sex).map(|n| n.id).collect();
                let p = *Self::pick(&mut self.rng, &others)?;
                Some(("preview_mating", json!({"type": "preview_mating", "mine": m, "partner": p})))
            }
            68..=79 => {
                // A page of 20 (the server's default) without one's own
                // listings: usually the first, newest first; sometimes the
                // next one, or the cheapest or closing soonest.
                let mine: Vec<u64> = self.adults(None).iter().map(|o| o.id).collect();
                let m = Self::pick(&mut self.rng, &mine).copied();
                let mut q = json!({"type": "browse_market", "mine": m, "hide_own": true});
                match self.rng.gen_range(0..10) {
                    0..=2 if self.cursor.is_some() => q["cursor"] = json!(self.cursor),
                    3 => q["sort"] = json!("price_asc"),
                    4 => q["sort"] = json!("ending"),
                    _ => {}
                }
                Some(("browse_market", q))
            }
            80..=83 => {
                if self.my_listings >= 2 {
                    return None;
                }
                let mine: Vec<u64> = self.adults(None).iter().filter(|o| !o.pregnant).map(|o| o.id).collect();
                let m = *Self::pick(&mut self.rng, &mine)?;
                let fee = self.rng.gen_range(0..20);
                if self.rng.gen_bool(0.3) {
                    // An auction closing in a quarter to a whole day.
                    let ticks = self.rng.gen_range(12..48);
                    return Some((
                        "create_auction",
                        json!({"type": "create_auction", "organism": m, "reserve": fee, "ticks": ticks}),
                    ));
                }
                Some((
                    "create_listing",
                    json!({"type": "create_listing", "organism": m, "fee": fee, "expires_days": 10}),
                ))
            }
            84..=88 => {
                let me = self.player;
                let open: Vec<Listing> = self.listings.iter().filter(|l| l.owner != me && !l.busy).cloned().collect();
                let l = Self::pick(&mut self.rng, &open)?.clone();
                let mine: Vec<u64> =
                    self.adults(None).iter().filter(|o| o.sex != l.sex && !o.pregnant).map(|o| o.id).collect();
                let m = *Self::pick(&mut self.rng, &mine)?;
                self.listings.retain(|x| x.id != l.id);
                if let Some(min) = l.min_bid {
                    return Some(("bid", json!({"type": "bid", "listing": l.id, "amount": min, "mine": m})));
                }
                Some(("buy_listing", json!({"type": "buy_listing", "listing": l.id, "mine": m})))
            }
            89..=92 => Some(("ledger", json!({"type": "ledger", "limit": 20}))),
            93..=96 => {
                let (x, y) = self.near(cx, cy, 30.0);
                Some(("migrate", json!({"type": "migrate", "x": x, "y": y})))
            }
            _ => Some(("clear_migration", json!({"type": "clear_migration"}))),
        }
    }
}

struct Ctx {
    args: Args,
    http: Http,
    ws_base: String,
    run: u32,
    stats: Stats,
}

fn design(i: usize, founders: u32, rng: &mut StdRng) -> Value {
    // Mostly one body plan so cross-player matings can conceive; a few
    // cold-blooded egg layers for variety.
    let odd = i % 10 == 9;
    json!({
        "name": format!("Load lineage {i}"),
        "thermo": if odd { "cold" } else { "warm" },
        "birth": if odd { "incubated_eggs" } else { "live" },
        "litter_size": 2,
        "traits": {
            "body_size": rng.gen_range(0.3..0.5),
            "fur": rng.gen_range(0.2..0.5),
            "keen_nose": rng.gen_range(0.1..0.4),
        },
        "founders": founders,
    })
}

/// A registration or login, retried as the client does when the server
/// is busy hashing (503 `busy`) or rate-limits it (429): after the server's
/// `retry_after_ms` (or a doubling backoff), with jitter, up to 12 times.
async fn auth_call(ctx: &Ctx, path: &str, creds: &Value, rng: &mut StdRng) -> Result<(u16, Value), String> {
    let mut backoff = 500.0_f64;
    for _ in 0..12 {
        let r = ctx.http.call(Method::POST, path, None, Some(creds.clone())).await?;
        if r.0 != 503 && r.0 != 429 {
            return Ok(r);
        }
        ctx.stats.auth_busy.fetch_add(1, Ordering::Relaxed);
        let hint = r.1["retry_after_ms"].as_f64().unwrap_or(0.0);
        let wait = hint.max(backoff).min(15_000.0) * rng.gen_range(0.5..1.5);
        backoff *= 2.0;
        tokio::time::sleep(Duration::from_secs_f64(wait / 1e3)).await;
    }
    ctx.http.call(Method::POST, path, None, Some(creds.clone())).await
}

async fn run_player(i: usize, ctx: Arc<Ctx>, mut stop: watch::Receiver<bool>) {
    let st = &ctx.stats;
    let worlds = ctx.args.world_ids();
    let world_ix = i % worlds.len();
    let world = &worlds[world_ix];
    let mut rng = StdRng::seed_from_u64(ctx.args.seed ^ (i as u64).wrapping_mul(0x9E37_79B9));
    let name = format!("lg{}_{i}", ctx.run);
    let creds = json!({"name": name, "password": "load test password"});
    // Register, then log in (both paths get exercised).
    let mut token = None;
    for attempt in 0..5 {
        match auth_call(&ctx, "/api/register", &creds, &mut rng).await {
            Ok((201, _)) | Ok((409, _)) => {
                if let Ok((200, v)) = auth_call(&ctx, "/api/login", &creds, &mut rng).await {
                    token = v["token"].as_str().map(str::to_string);
                }
                break;
            }
            other => {
                if attempt == 4 {
                    eprintln!("player {i}: register failed: {other:?}");
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }
    let Some(token) = token else {
        st.setup_failed.fetch_add(1, Ordering::Relaxed);
        return;
    };
    let found = in_world("/api/lineage", world);
    match ctx.http.call(Method::POST, &found, Some(&token), Some(design(i, ctx.args.founders, &mut rng))).await {
        Ok((201, _)) | Ok((409, _)) => {
            st.founded.fetch_add(1, Ordering::Relaxed);
        }
        other => eprintln!("player {i}: founding failed: {other:?}"),
    }
    let ws_url = in_world(&format!("{}/ws?token={token}&proto={}", ctx.ws_base, ctx.args.proto), world);
    let mut ws = match tokio_tungstenite::connect_async(ws_url).await {
        Ok((ws, _)) => ws,
        Err(e) => {
            eprintln!("player {i}: websocket failed: {e}");
            st.setup_failed.fetch_add(1, Ordering::Relaxed);
            return;
        }
    };
    st.connected.fetch_add(1, Ordering::Relaxed);
    // `welcome` carries the player id.
    let player_id = loop {
        match ws.next().await {
            Some(Ok(Message::Text(t))) => {
                let v: Value = serde_json::from_str(&t).unwrap_or(Value::Null);
                if v["type"] == "welcome" {
                    break v["player"].as_u64().unwrap_or(0) as u32;
                }
            }
            _ => {
                st.connected.fetch_sub(1, Ordering::Relaxed);
                st.unexpected_close.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
    };
    let mut p = Player {
        player: player_id,
        state: StateLite::default(),
        listings: Vec::new(),
        cursor: None,
        my_listings: 0,
        pending: HashMap::new(),
        next_id: 1,
        rng,
        size: ctx.args.size as f32,
    };
    let mut decoder = DeltaDecoder::new();
    let mut deflate = ctx.args.deflate.then(|| Compress::new(Compression::default(), false));
    let mean = 60.0 / ctx.args.actions_per_min.max(0.01);
    let mut next_action = tokio::time::Instant::now() + Duration::from_secs_f64(p.rng.gen_range(0.0..mean));
    loop {
        tokio::select! {
            _ = stop.changed() => break,
            _ = tokio::time::sleep_until(next_action) => {
                next_action += Duration::from_secs_f64(mean * p.rng.gen_range(0.5..1.5));
                let Some((name, mut msg)) = p.action() else { continue };
                let id = p.next_id;
                p.next_id += 1;
                msg["id"] = json!(id);
                p.pending.insert(id, (Instant::now(), name));
                st.commands.fetch_add(1, Ordering::Relaxed);
                if ws.send(Message::Text(msg.to_string())).await.is_err() {
                    st.unexpected_close.fetch_add(1, Ordering::Relaxed);
                    break;
                }
            }
            frame = ws.next() => {
                let text = match frame {
                    Some(Ok(Message::Text(t))) => t,
                    Some(Ok(Message::Binary(b))) => {
                        // A protocol 2 state.
                        st.msgs_in.fetch_add(1, Ordering::Relaxed);
                        st.bytes_in.fetch_add(b.len() as u64, Ordering::Relaxed);
                        st.states_in.fetch_add(1, Ordering::Relaxed);
                        st.state_bytes_in.fetch_add(b.len() as u64, Ordering::Relaxed);
                        st.state_size(b.len());
                        st.kind_bytes("state", b.len());
                        if let Some(z) = deflate.as_mut() {
                            st.state_deflated.fetch_add(deflated(z, &b) as u64, Ordering::Relaxed);
                        }
                        match decoder.apply(&b) {
                            Ok(s) => {
                                if ctx.args.dump_state.is_some() {
                                    let text = serde_json::to_string(&s).unwrap_or_default();
                                    let mut big = st.largest_state.lock().unwrap();
                                    if text.len() > big.len() {
                                        *big = text;
                                    }
                                }
                                p.state = StateLite::from(s);
                            }
                            Err(_) => {
                                st.decode_errors.fetch_add(1, Ordering::Relaxed);
                                if ws.send(Message::Text(json!({"type": "resync"}).to_string())).await.is_err() {
                                    st.unexpected_close.fetch_add(1, Ordering::Relaxed);
                                    break;
                                }
                            }
                        }
                        continue;
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => {
                        st.unexpected_close.fetch_add(1, Ordering::Relaxed);
                        break;
                    }
                    Some(Ok(_)) => continue,
                };
                st.msgs_in.fetch_add(1, Ordering::Relaxed);
                st.bytes_in.fetch_add(text.len() as u64, Ordering::Relaxed);
                let Ok(msg) = serde_json::from_str::<Incoming>(&text) else { continue };
                let kind = match &msg {
                    Incoming::State(_) => "state".to_string(),
                    Incoming::Result { id, .. } => {
                        format!("result:{}", id.and_then(|id| p.pending.get(&id)).map_or("?", |(_, n)| n))
                    }
                    Incoming::MatingRequest { .. } => "mating_request".into(),
                    _ => "other".into(),
                };
                st.kind_bytes(&kind, text.len());
                match msg {
                    Incoming::State(s) => {
                        st.states_in.fetch_add(1, Ordering::Relaxed);
                        st.state_bytes_in.fetch_add(text.len() as u64, Ordering::Relaxed);
                        st.state_size(text.len());
                        if let Some(z) = deflate.as_mut() {
                            st.state_deflated.fetch_add(deflated(z, text.as_bytes()) as u64, Ordering::Relaxed);
                        }
                        if ctx.args.dump_state.is_some() {
                            let mut big = st.largest_state.lock().unwrap();
                            if text.len() > big.len() {
                                *big = text.to_string();
                            }
                        }
                        p.state = s;
                    }
                    Incoming::Result { id, ok, error, data } => {
                        let Some((sent, name)) = id.and_then(|id| p.pending.remove(&id)) else { continue };
                        st.latency(world_ix, name, sent.elapsed().as_secs_f32() * 1000.0);
                        if ok {
                            st.results_ok.fetch_add(1, Ordering::Relaxed);
                        } else {
                            st.results_err.fetch_add(1, Ordering::Relaxed);
                            let code = error.map_or_else(|| "?".to_string(), |e| e.code);
                            *st.errors.lock().unwrap().entry(format!("{name}:{code}")).or_default() += 1;
                        }
                        if !ok {
                            continue;
                        }
                        match name {
                            "browse_market" => {
                                let ls = data.as_ref().and_then(|d| d["listings"].as_array()).cloned().unwrap_or_default();
                                p.cursor = data.as_ref().and_then(|d| d["next_cursor"].as_str()).map(str::to_string);
                                p.listings = ls
                                    .iter()
                                    .filter_map(|l| {
                                        Some(Listing {
                                            id: l["id"].as_u64()?,
                                            owner: l["owner"].as_u64()? as u32,
                                            sex: l["sex"].as_str()?.to_string(),
                                            busy: l["busy"].as_bool().unwrap_or(false),
                                            min_bid: l["auction"]["min_bid"].as_u64(),
                                        })
                                    })
                                    .collect();
                            }
                            "create_listing" | "create_auction" => p.my_listings += 1,
                            _ => {}
                        }
                    }
                    Incoming::MatingRequest { offer } => {
                        st.mating_requests.fetch_add(1, Ordering::Relaxed);
                        let id = p.next_id;
                        p.next_id += 1;
                        let accept = p.rng.gen_bool(0.7);
                        p.pending.insert(id, (Instant::now(), "respond_mating"));
                        st.commands.fetch_add(1, Ordering::Relaxed);
                        let msg = json!({"type": "respond_mating", "id": id, "request": offer.id, "accept": accept});
                        if ws.send(Message::Text(msg.to_string())).await.is_err() {
                            st.unexpected_close.fetch_add(1, Ordering::Relaxed);
                            break;
                        }
                    }
                    Incoming::SessionEnded {} => {
                        st.unexpected_close.fetch_add(1, Ordering::Relaxed);
                        break;
                    }
                    Incoming::Other => {}
                }
                // Forget commands that never got an answer (shouldn't happen).
                if p.pending.len() > 256 {
                    p.pending.clear();
                }
            }
        }
    }
    let _ = ws.close(None).await;
    st.connected.fetch_sub(1, Ordering::Relaxed);
}

// --- Reporting -------------------------------------------------------------

#[derive(Clone, Debug, Default, serde::Serialize)]
struct Sample {
    t: f64,
    tick: u64,
    organisms: usize,
    conns: usize,
    tick_p50: f32,
    tick_p99: f32,
    states_p50: f32,
    states_p99: f32,
    substep_p99: f32,
    ticks_per_s: f64,
    out_mb_s: f64,
    state_kb: f64,
    states_per_client_s: f64,
    cmds_s: f64,
    cpu_cores: f64,
    rss_mb: f64,
    loadgen_cpu_cores: f64,
    dropped_conns: u64,
    skipped_states: u64,
    max_queued: usize,
    saves: u64,
    /// Server time spent encoding one state (JSON or protocol 2), µs.
    encode_us: f64,
}

fn mb(b: u64) -> f64 {
    b as f64 / 1e6
}

#[tokio::main]
async fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let run: u32 = rand::random::<u32>() % 1_000_000;

    // Start (or find) the server.
    let mut proc = None;
    let base = if let Some(url) = &args.url {
        url.clone()
    } else {
        let data = args.data.clone().unwrap_or_else(|| std::env::temp_dir().join(format!("evo-loadgen-{run}")));
        std::fs::create_dir_all(&data).expect("data dir");
        // Every world under test has a save (the default world is `main`).
        let has_save = args.world_ids().iter().all(|w| {
            let dir = data.join("worlds").join(w.as_deref().unwrap_or("main")).join("snapshots");
            std::fs::read_dir(dir).map(|mut d| d.next().is_some()).unwrap_or(false)
        });
        if args.warm_until > 0 && !has_save {
            let port = free_port();
            let base = format!("http://127.0.0.1:{port}");
            let mut warm = spawn_server(&args, &data, port, true, "warm.log");
            let http = Http::new(&base);
            wait_up(&http).await;
            let t = Instant::now();
            // Until every world holds `--warm-until` organisms.
            loop {
                let mut counts = Vec::new();
                for w in args.world_ids() {
                    counts.push(organisms_in(&http, &w).await);
                }
                if counts.iter().all(|&n| n >= args.warm_until) {
                    println!("warm-up: {counts:?} organisms after {:.0} s", t.elapsed().as_secs_f64());
                    break;
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            assert!(stop_server(&mut warm), "warm-up server did not stop cleanly");
        }
        let port = free_port();
        proc = Some(spawn_server(&args, &data, port, args.fast, "server.log"));
        println!("server data in {}", data.display());
        format!("http://127.0.0.1:{port}")
    };
    let http = Http::new(&base);
    wait_up(&http).await;
    let addr: SocketAddr = base.trim_start_matches("http://").parse().unwrap_or(SocketAddr::from(([127, 0, 0, 1], 80)));
    let ctx = Arc::new(Ctx {
        args: args.clone(),
        http: http.clone(),
        ws_base: format!("ws://{addr}"),
        run,
        stats: Stats::default(),
    });
    let worlds = args.world_ids();
    let main_world = worlds[0].clone();
    let world_names: Vec<String> = worlds.iter().map(|w| w.clone().unwrap_or_else(|| "default".into())).collect();
    println!(
        "{} players, {:.0} s (ramp {:.0} s), {:.0} actions/min each, tick {} ms, state {} ms, {}² world(s) {}: {} organisms",
        args.players,
        args.duration,
        args.ramp,
        args.actions_per_min,
        args.tick_ms,
        args.state_ms,
        args.size,
        world_names.join(", "),
        organisms_in(&http, &main_world).await
    );

    let (stop_tx, stop_rx) = watch::channel(false);
    let mut tasks = Vec::new();
    let started = Instant::now();
    for i in 0..args.players {
        let (ctx, stop) = (ctx.clone(), stop_rx.clone());
        let delay = Duration::from_secs_f64(args.ramp * i as f64 / args.players.max(1) as f64);
        tasks.push(tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            run_player(i, ctx, stop).await;
        }));
    }

    // Report every `--report` seconds.
    let st = &ctx.stats;
    let mut samples: Vec<Sample> = Vec::new();
    let mut prev: Option<(Instant, MetricsView, u64, u64, u64, f64)> = None;
    let end = started + Duration::from_secs_f64(args.ramp + args.duration);
    let mut first_rss = None;
    // The other worlds' timings, per report, and their previous metrics.
    let mut others: Vec<Vec<WorldRow>> = vec![Vec::new(); worlds.len()];
    let mut others_prev: Vec<Option<(Instant, MetricsView)>> = vec![None; worlds.len()];
    // Outbound bytes over all worlds, per report (MB/s).
    let mut out_total: Vec<(f64, f64)> = Vec::new();
    println!(
        "{:>6} {:>7} {:>6} {:>5} {:>13} {:>13} {:>6} {:>8} {:>8} {:>7} {:>6} {:>6} {:>7} {:>5} {:>4} {:>4}",
        "t s",
        "tick",
        "orgs",
        "conns",
        "tick p50/p99",
        "state p50/99",
        "tick/s",
        "out MB/s",
        "state KB",
        "st/cl/s",
        "cmd/s",
        "cpu",
        "RSS MB",
        "drop",
        "skip",
        "save"
    );
    loop {
        let now = Instant::now();
        if now >= end {
            break;
        }
        tokio::time::sleep(Duration::from_secs_f64(args.report).min(end - now)).await;
        let Some(m) = http.get::<MetricsView>(&in_world("/api/metrics", &main_world)).await else { continue };
        let orgs = organisms_in(&http, &main_world).await;
        let mut others_out = 0.0;
        for (k, w) in worlds.iter().enumerate().skip(1) {
            if let Some(o) = http.get::<MetricsView>(&in_world("/api/metrics", w)).await {
                let n = organisms_in(&http, w).await;
                let now = Instant::now();
                if let Some((pt, pm)) = &others_prev[k] {
                    let dt = now.duration_since(*pt).as_secs_f64();
                    let row = WorldRow {
                        t: started.elapsed().as_secs_f64(),
                        tick_p50: o.tick_ms.p50,
                        tick_p99: o.tick_ms.p99,
                        states_p99: o.states_ms.p99,
                        substep_p99: o.substep_ms.p99,
                        ticks_per_s: (o.ticks - pm.ticks) as f64 / dt,
                        out_mb_s: mb(o.bytes_out - pm.bytes_out) / dt,
                        conns: o.conns,
                        organisms: n,
                    };
                    println!(
                        "  [{}] tick p50/p99 {:.1}/{:.1} ms, states p99 {:.1} ms, sub-step p99 {:.1} ms, \
                         {:.2} tick/s, {:.2} MB/s out, {} conns, {} organisms",
                        world_names[k],
                        row.tick_p50,
                        row.tick_p99,
                        row.states_p99,
                        row.substep_p99,
                        row.ticks_per_s,
                        row.out_mb_s,
                        row.conns,
                        row.organisms
                    );
                    others_out += row.out_mb_s;
                    others[k].push(row);
                }
                others_prev[k] = Some((now, o));
            }
        }
        let (states_in, state_bytes, cmds) = (
            st.states_in.load(Ordering::Relaxed),
            st.state_bytes_in.load(Ordering::Relaxed),
            st.commands.load(Ordering::Relaxed),
        );
        let lg_cpu = proc_usage("self").0.unwrap_or(0.0);
        let t = Instant::now();
        if let Some((pt, pm, ps, psb, pc, plg)) = &prev {
            let dt = t.duration_since(*pt).as_secs_f64();
            let clients = st.connected.load(Ordering::Relaxed).max(1) as f64;
            let d_states = (states_in - ps).max(1);
            let s = Sample {
                t: started.elapsed().as_secs_f64(),
                tick: m.tick,
                organisms: orgs,
                conns: m.conns,
                tick_p50: m.tick_ms.p50,
                tick_p99: m.tick_ms.p99,
                states_p50: m.states_ms.p50,
                states_p99: m.states_ms.p99,
                substep_p99: m.substep_ms.p99,
                ticks_per_s: (m.ticks - pm.ticks) as f64 / dt,
                out_mb_s: mb(m.bytes_out - pm.bytes_out) / dt,
                state_kb: (state_bytes - psb) as f64 / d_states as f64 / 1e3,
                states_per_client_s: (states_in - ps) as f64 / dt / clients,
                cmds_s: (cmds - pc) as f64 / dt,
                cpu_cores: match (m.cpu_seconds, pm.cpu_seconds) {
                    (Some(a), Some(b)) => (a - b) / dt,
                    _ => 0.0,
                },
                rss_mb: m.rss_bytes.map_or(0.0, mb),
                loadgen_cpu_cores: (lg_cpu - plg) / dt,
                dropped_conns: m.dropped_conns,
                skipped_states: m.skipped_states,
                max_queued: m.max_queued,
                saves: m.saves,
                encode_us: (m.state_encode_ns - pm.state_encode_ns) as f64
                    / (m.states_out - pm.states_out).max(1) as f64
                    / 1e3,
            };
            println!(
                "{:>6.0} {:>7} {:>6} {:>5} {:>6.1}/{:<6.1} {:>6.1}/{:<6.1} {:>6.1} {:>8.2} {:>8.1} {:>7.2} {:>6.0} {:>6.2} {:>7.0} {:>5} {:>4} {:>4}",
                s.t, s.tick, s.organisms, s.conns, s.tick_p50, s.tick_p99, s.states_p50, s.states_p99, s.ticks_per_s,
                s.out_mb_s, s.state_kb, s.states_per_client_s, s.cmds_s, s.cpu_cores, s.rss_mb, s.dropped_conns,
                s.skipped_states, s.saves
            );
            out_total.push((s.t, s.out_mb_s + others_out));
            samples.push(s);
        }
        if first_rss.is_none() && st.connected.load(Ordering::Relaxed) >= args.players {
            first_rss = m.rss_bytes;
        }
        prev = Some((t, m, states_in, state_bytes, cmds, lg_cpu));
    }

    // Summary.
    let final_m = http.get::<MetricsView>(&in_world("/api/metrics", &main_world)).await.unwrap_or_default();
    let elapsed = started.elapsed().as_secs_f64();
    let _ = stop_tx.send(true);
    for t in tasks {
        let _ = t.await;
    }
    let steady: Vec<&Sample> = samples.iter().filter(|s| s.t >= args.ramp + args.report).collect();
    let steady = if steady.is_empty() { samples.iter().collect() } else { steady };
    let med = |f: &dyn Fn(&Sample) -> f64| {
        let mut v: Vec<f64> = steady.iter().map(|s| f(s)).collect();
        v.sort_by(f64::total_cmp);
        v.get(v.len() / 2).copied().unwrap_or(0.0)
    };
    let max = |f: &dyn Fn(&Sample) -> f64| steady.iter().map(|s| f(s)).fold(0.0, f64::max);
    let (lat50, lat99, latmax) = st.latency_all.lock().unwrap().as_ref().map_or((0.0, 0.0, 0.0), Reservoir::quantiles);
    let (sz50, sz99, szmax) = st.state_sizes.lock().unwrap().as_ref().map_or((0.0, 0.0, 0.0), Reservoir::quantiles);
    let mut per_cmd: Vec<(String, u64, f32, f32)> = st
        .latency
        .lock()
        .unwrap()
        .iter()
        .map(|(k, r)| {
            let (a, b, _) = r.quantiles();
            (k.to_string(), r.seen, a, b)
        })
        .collect();
    per_cmd.sort_by(|a, b| a.0.cmp(&b.0));
    let mut errors: Vec<(String, u64)> = st.errors.lock().unwrap().iter().map(|(k, v)| (k.clone(), *v)).collect();
    errors.sort_by_key(|e| std::cmp::Reverse(e.1));
    let rss_max = samples.iter().map(|s| s.rss_mb).fold(0.0, f64::max);
    let rss_end = samples.last().map_or(0.0, |s| s.rss_mb);

    // Per world: the medians of its steady reports (the first world from
    // the main samples).
    let steady_from = args.ramp + args.report;
    let median = |mut v: Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v.get(v.len() / 2).copied().unwrap_or(0.0)
    };
    let latency_world = st.latency_world.lock().unwrap();
    let per_world: Vec<Value> = worlds
        .iter()
        .enumerate()
        .map(|(k, _)| {
            let (lat50, lat99, _) = latency_world.get(&k).map_or((0.0, 0.0, 0.0), Reservoir::quantiles);
            if k == 0 {
                json!({"world": world_names[0], "tick_ms_p50": med(&|s| s.tick_p50 as f64),
                    "tick_ms_p99": med(&|s| s.tick_p99 as f64), "states_ms_p99": med(&|s| s.states_p99 as f64),
                    "substep_ms_p99": med(&|s| s.substep_p99 as f64), "ticks_per_s": med(&|s| s.ticks_per_s),
                    "out_mb_s": med(&|s| s.out_mb_s), "cmd_latency_ms_p50": lat50, "cmd_latency_ms_p99": lat99,
                    "conns": final_m.conns, "organisms_end": samples.last().map_or(0, |s| s.organisms)})
            } else {
                let rows: Vec<_> = others[k].iter().filter(|r| r.t >= steady_from).collect();
                let rows = if rows.is_empty() { others[k].iter().collect() } else { rows };
                let med_of = |f: &dyn Fn(&WorldRow) -> f64| median(rows.iter().map(|r| f(r)).collect());
                json!({"world": world_names[k],
                    "tick_ms_p50": med_of(&|r| r.tick_p50 as f64),
                    "tick_ms_p99": med_of(&|r| r.tick_p99 as f64),
                    "states_ms_p99": med_of(&|r| r.states_p99 as f64),
                    "substep_ms_p99": med_of(&|r| r.substep_p99 as f64),
                    "ticks_per_s": med_of(&|r| r.ticks_per_s),
                    "out_mb_s": med_of(&|r| r.out_mb_s),
                    "cmd_latency_ms_p50": lat50, "cmd_latency_ms_p99": lat99,
                    "conns": rows.last().map_or(0, |r| r.conns), "organisms_end": rows.last().map_or(0, |r| r.organisms)})
            }
        })
        .collect();
    drop(latency_world);
    // The whole run, ramp included, over every world: the slowest 10 s of
    // ticks and the worst tick p99 (a login burst shows here).
    let all_rows = || others.iter().flatten();
    let ticks_per_s_min =
        samples.iter().map(|s| s.ticks_per_s).chain(all_rows().map(|r| r.ticks_per_s)).fold(f64::INFINITY, f64::min);
    let tick_p99_max =
        samples.iter().map(|s| s.tick_p99 as f64).chain(all_rows().map(|r| r.tick_p99 as f64)).fold(0.0, f64::max);
    let out_total_mb_s = median(out_total.iter().filter(|r| r.0 >= steady_from).map(|r| r.1).collect());

    let mut stopped_clean = None;
    let mut panics = 0;
    if let Some(p) = proc.as_mut() {
        stopped_clean = Some(stop_server(p));
        let log = std::fs::read_to_string(&p.log).unwrap_or_default();
        panics = log.matches("panicked").count();
    }

    let mut kinds: Vec<(String, u64)> = st.bytes_by_kind.lock().unwrap().iter().map(|(k, v)| (k.clone(), *v)).collect();
    kinds.sort_by_key(|e| std::cmp::Reverse(e.1));
    let per_client = |b: u64| b as f64 / 1e3 / elapsed / args.players.max(1) as f64;
    let by_kind: Vec<Value> = kinds.iter().take(10).map(|(k, b)| json!({"kind": k, "kb_s": per_client(*b)})).collect();
    let summary = json!({
        "players": args.players,
        "worlds": per_world,
        "connected": st.connected.load(Ordering::Relaxed),
        "founded": st.founded.load(Ordering::Relaxed),
        "setup_failed": st.setup_failed.load(Ordering::Relaxed),
        "auth_busy_retries": st.auth_busy.load(Ordering::Relaxed),
        "hashing": final_m.hashing,
        "ticks_per_s_min_whole_run": if ticks_per_s_min.is_finite() { ticks_per_s_min } else { 0.0 },
        "tick_ms_p99_max_whole_run": tick_p99_max,
        "duration_s": elapsed,
        "size": args.size,
        "tick_ms_setting": args.tick_ms,
        "state_ms_setting": args.state_ms,
        "organisms_end": samples.last().map_or(0, |s| s.organisms),
        "tick_ms_p50": med(&|s| s.tick_p50 as f64),
        "tick_ms_p99": med(&|s| s.tick_p99 as f64),
        "tick_ms_p99_worst": max(&|s| s.tick_p99 as f64),
        "states_ms_p50": med(&|s| s.states_p50 as f64),
        "states_ms_p99": med(&|s| s.states_p99 as f64),
        "substep_ms_p99": med(&|s| s.substep_p99 as f64),
        "ticks_per_s": med(&|s| s.ticks_per_s),
        "proto": args.proto,
        "state_bytes_p50": sz50, "state_bytes_p99": sz99, "state_bytes_max": szmax,
        "state_encode_us": med(&|s| s.encode_us),
        "state_deflate_ratio": if args.deflate {
            st.state_bytes_in.load(Ordering::Relaxed) as f64 / st.state_deflated.load(Ordering::Relaxed).max(1) as f64
        } else {
            0.0
        },
        "decode_errors": st.decode_errors.load(Ordering::Relaxed),
        "kb_s_per_client_by_kind": by_kind,
        "keyframes_out": final_m.keyframes_out,
        "resyncs": final_m.resyncs,
        "states_per_client_s": med(&|s| s.states_per_client_s),
        "out_mb_s": med(&|s| s.out_mb_s),
        "out_kb_s_per_client": out_total_mb_s * 1e3 / args.players.max(1) as f64,
        "out_mb_s_all_worlds": out_total_mb_s,
        "cmds_s": med(&|s| s.cmds_s),
        "cmd_latency_ms_p50": lat50, "cmd_latency_ms_p99": lat99, "cmd_latency_ms_max": latmax,
        "per_command": per_cmd.iter().map(|(k, n, a, b)| json!({"command": k, "n": n, "p50": a, "p99": b})).collect::<Vec<_>>(),
        "server_cpu_cores": med(&|s| s.cpu_cores),
        "loadgen_cpu_cores": med(&|s| s.loadgen_cpu_cores),
        "rss_mb_full": first_rss.map_or(0.0, mb), "rss_mb_max": rss_max, "rss_mb_end": rss_end,
        "saves": final_m.saves, "save_capture_ms": final_m.save_capture_ms, "save_encode_ms": final_m.save_encode_ms,
        "save_write_ms": final_m.save_write_ms, "save_mb": mb(final_m.save_bytes),
        "dropped_conns": final_m.dropped_conns, "skipped_states": final_m.skipped_states,
        "max_queued": final_m.max_queued, "unexpected_closes": st.unexpected_close.load(Ordering::Relaxed),
        "mating_requests": st.mating_requests.load(Ordering::Relaxed),
        "results_ok": st.results_ok.load(Ordering::Relaxed), "results_err": st.results_err.load(Ordering::Relaxed),
        "errors": errors.iter().take(12).map(|(k, v)| json!({"what": k, "n": v})).collect::<Vec<_>>(),
        "server_stopped_clean": stopped_clean, "server_panics": panics,
        "samples": samples,
    });
    println!("\n=== summary ===");
    let mut brief = summary.clone();
    brief.as_object_mut().unwrap().remove("samples");
    println!("{}", serde_json::to_string_pretty(&brief).unwrap());
    if let Some(path) = &args.dump_state {
        std::fs::write(path, st.largest_state.lock().unwrap().as_bytes()).expect("write state");
    }
    if let Some(path) = &args.json {
        std::fs::write(path, serde_json::to_string_pretty(&summary).unwrap()).expect("write json");
    }
}

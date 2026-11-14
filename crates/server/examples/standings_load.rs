//! Load test for ranked-season and leaderboard reads: many clients asking
//! for standings at once, and what that costs the server.
//!
//! ```text
//! cargo build --release -p evo-server --bins --examples
//! target/release/examples/standings_load                         # file store
//! target/release/examples/standings_load --database-url postgres://…/db
//! target/release/examples/standings_load --clients 400 --duration 15 --scenarios current,poll,ws
//! ```
//!
//! It starts `evo-server` (next to this example in `target/release`, or
//! `--server PATH`) in fast mode with short ranked seasons
//! (`--season-days`, default 2) on a small map, registers and founds
//! `--players` lineages, runs until `--seasons` ranked seasons have ended,
//! stops it (saving) and restarts it from the saves ticking at `--tick-ms`
//! (default 100), so the measurements run against a world with finished
//! seasons, live standings and a daily history, still rolling over.
//!
//! Then, for each scenario, `--clients` tasks run closed-loop (each sends
//! its next request as soon as the last one answers) for `--duration`
//! seconds:
//!
//! - `current`: `GET /api/seasons/current?player=P&limit=5`
//! - `list`: `GET /api/seasons?limit=8`
//! - `one`: `GET /api/seasons/1?player=P&limit=10`
//! - `player`: `GET /api/seasons/player/P?limit=50`
//! - `weather`: `GET /api/leaderboard/seasons?limit=6` (the lineage tab)
//! - `poll`: the seasons tab's refresh (current, list and player at once)
//! - `ws`: `--clients` WebSockets (spread over the players' accounts) that
//!   send `watch_season` and read the standings pushed to them, instead of
//!   polling; reports pushes received. Servers without `watch_season` answer
//!   with an error, which is reported as such.
//! - `ws-idle`: the same sockets without `watch_season` (what the sockets
//!   cost anyway, states included: subtract it from `ws`).
//!
//! With `--rate R` the HTTP clients run open loop instead: R rounds a
//! second in all (a `poll` round is three requests), each client's rounds
//! evenly spaced, so before/after runs compare at the same load.
//!
//! Each line reports requests per second, latency (p50/p99), the server
//! process's CPU above its idle level (measured first, over the same
//! duration, with no clients), CPU per request and the world thread's tick
//! time (p99), which shows whether reads stall the simulation.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use evo_server::metrics::{proc_usage, MetricsView};
use futures_util::{SinkExt, StreamExt};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::{Method, Request};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

const USAGE: &str = "usage: standings_load [--clients 200] [--duration 10] [--players 60] [--seasons 4] \
[--season-days 2] [--days-per-year 8] [--size 128] [--tick-ms 100] [--server PATH] [--data DIR] \
[--database-url URL] [--scenarios current,list,one,player,weather,poll,ws] [--rate 0]";

struct Args {
    clients: usize,
    duration: u64,
    players: usize,
    seasons: usize,
    season_days: u64,
    days_per_year: u32,
    size: usize,
    tick_ms: u64,
    server: Option<PathBuf>,
    data: Option<PathBuf>,
    database_url: Option<String>,
    scenarios: Vec<String>,
    /// Open loop: this many client rounds a second in all (0: closed loop).
    rate: f64,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        clients: 200,
        duration: 10,
        players: 60,
        seasons: 4,
        season_days: 2,
        days_per_year: 8,
        size: 128,
        tick_ms: 100,
        server: None,
        data: None,
        database_url: None,
        scenarios: ["current", "list", "one", "player", "weather", "poll", "ws"].map(String::from).to_vec(),
        rate: 0.0,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} needs a value"));
        fn num<T: std::str::FromStr>(s: String) -> Result<T, String> {
            s.parse().map_err(|_| format!("bad number {s:?}"))
        }
        match flag.as_str() {
            "--clients" => a.clients = num(value()?)?,
            "--duration" => a.duration = num(value()?)?,
            "--players" => a.players = num(value()?)?,
            "--seasons" => a.seasons = num(value()?)?,
            "--season-days" => a.season_days = num(value()?)?,
            "--days-per-year" => a.days_per_year = num(value()?)?,
            "--size" => a.size = num(value()?)?,
            "--tick-ms" => a.tick_ms = num(value()?)?,
            "--server" => a.server = Some(value()?.into()),
            "--data" => a.data = Some(value()?.into()),
            "--database-url" => a.database_url = Some(value()?),
            "--scenarios" => a.scenarios = value()?.split(',').map(str::to_string).collect(),
            "--rate" => a.rate = num(value()?)?,
            "-h" | "--help" => return Err(USAGE.into()),
            _ => return Err(format!("unknown flag {flag}\n{USAGE}")),
        }
    }
    Ok(a)
}

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
    ) -> Result<(u16, Bytes), String> {
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
        Ok((status, bytes))
    }

    async fn json(&self, method: Method, path: &str, token: Option<&str>, body: Option<Value>) -> Option<(u16, Value)> {
        let (s, b) = self.call(method, path, token, body).await.ok()?;
        Some((s, serde_json::from_slice(&b).unwrap_or(Value::Null)))
    }

    async fn get(&self, path: &str) -> Option<Value> {
        match self.json(Method::GET, path, None, None).await {
            Some((200, v)) => Some(v),
            _ => None,
        }
    }
}

struct Server {
    child: Child,
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).map(|a| a.port()).expect("free port")
}

fn server_binary(args: &Args) -> PathBuf {
    if let Some(p) = &args.server {
        return p.clone();
    }
    let exe = std::env::current_exe().expect("current exe");
    exe.parent().and_then(Path::parent).expect("target dir").join("evo-server")
}

fn spawn(args: &Args, data: &Path, port: u16, fast: bool, log: &str) -> Server {
    let out = std::fs::File::create(data.join(log)).expect("server log");
    let mut cmd = Command::new(server_binary(args));
    cmd.args(["--bind", &format!("127.0.0.1:{port}"), "--no-rate-limits", "--size", &args.size.to_string()]).args([
        "--season-days",
        &args.season_days.to_string(),
        "--days-per-year",
        &args.days_per_year.to_string(),
    ]);
    match &args.database_url {
        Some(url) => cmd.args(["--database-url", url]),
        None => cmd.arg("--data").arg(data),
    };
    if fast {
        cmd.arg("--fast");
    } else {
        cmd.args(["--tick-ms", &args.tick_ms.to_string(), "--state-ms", "1000"]);
    }
    cmd.env("RUST_LOG", "warn").stdin(Stdio::null()).stderr(out.try_clone().unwrap()).stdout(out);
    Server { child: cmd.spawn().expect("start evo-server (cargo build --release -p evo-server)") }
}

fn stop(s: &mut Server) {
    let _ = Command::new("kill").args(["-TERM", &s.child.id().to_string()]).status();
    let deadline = Instant::now() + Duration::from_secs(120);
    while Instant::now() < deadline {
        if let Ok(Some(_)) = s.child.try_wait() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = s.child.kill();
}

async fn wait_up(http: &Http) {
    for _ in 0..600 {
        if http.get("/api/world").await.is_some() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("server did not come up");
}

/// Register, log in and found a lineage; (player id, token).
async fn setup_player(http: &Http, i: usize) -> Option<(u64, String)> {
    let creds = json!({"name": format!("sl_{i}"), "password": "load test password"});
    for _ in 0..40 {
        match http.json(Method::POST, "/api/register", None, Some(creds.clone())).await {
            Some((201 | 409, _)) => break,
            _ => tokio::time::sleep(Duration::from_millis(500)).await,
        }
    }
    let mut login = None;
    for _ in 0..40 {
        match http.json(Method::POST, "/api/login", None, Some(creds.clone())).await {
            Some((200, v)) => {
                login = Some(v);
                break;
            }
            _ => tokio::time::sleep(Duration::from_millis(500)).await,
        }
    }
    let v = login?;
    let token = v["token"].as_str()?.to_string();
    let player = v["player"].as_u64()?;
    let design = json!({
        "name": format!("Standings {i}"), "thermo": "warm", "birth": "live", "litter_size": 2,
        "traits": {"body_size": 0.3 + (i % 5) as f64 * 0.04, "fur": 0.3}, "founders": 12,
    });
    let _ = http.json(Method::POST, "/api/lineage", Some(&token), Some(design)).await;
    Some((player, token))
}

fn cpu_of(pid: u32) -> f64 {
    proc_usage(&pid.to_string()).0.unwrap_or(0.0)
}

fn quantile(v: &mut [f32], q: f64) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f32::total_cmp);
    v[((v.len() - 1) as f64 * q).round() as usize]
}

struct Outcome {
    requests: u64,
    errors: u64,
    pushes: u64,
    lat: Vec<f32>,
}

async fn run_http(
    http: &Http,
    players: &[(u64, String)],
    scenario: &str,
    clients: usize,
    dur: Duration,
    rate: f64,
) -> Outcome {
    let done = Arc::new(AtomicU64::new(0));
    let errors = Arc::new(AtomicU64::new(0));
    let lat = Arc::new(Mutex::new(Vec::new()));
    let end = Instant::now() + dur;
    let mut tasks = Vec::new();
    for c in 0..clients {
        let (http, done, errors, lat) = (http.clone(), done.clone(), errors.clone(), lat.clone());
        let p = players[c % players.len()].0;
        let paths: Vec<String> = match scenario {
            "current" => vec![format!("/api/seasons/current?player={p}&limit=5")],
            "list" => vec!["/api/seasons?offset=0&limit=8".into()],
            "one" => vec![format!("/api/seasons/1?player={p}&limit=10")],
            "player" => vec![format!("/api/seasons/player/{p}?limit=50")],
            "weather" => vec!["/api/leaderboard/seasons?limit=6".into()],
            "poll" => vec![
                format!("/api/seasons/current?player={p}&limit=5"),
                "/api/seasons?offset=0&limit=8".into(),
                format!("/api/seasons/player/{p}?limit=50"),
            ],
            other => panic!("unknown scenario {other}"),
        };
        // Open loop: each client's rounds `clients / rate` seconds apart,
        // the clients spread evenly over that interval.
        let every = (rate > 0.0).then(|| Duration::from_secs_f64(clients as f64 / rate));
        let phase = every.map(|e| e.mul_f64(c as f64 / clients as f64));
        tasks.push(tokio::spawn(async move {
            let mut mine = Vec::new();
            let mut next = Instant::now() + phase.unwrap_or_default();
            while Instant::now() < end {
                if let Some(e) = every {
                    tokio::time::sleep_until(next.into()).await;
                    next += e;
                    if Instant::now() >= end {
                        break;
                    }
                }
                let t = Instant::now();
                // The poll's requests go out together, as the client sends them.
                let replies =
                    futures_util::future::join_all(paths.iter().map(|p| http.call(Method::GET, p, None, None))).await;
                mine.push(t.elapsed().as_secs_f32() * 1000.0);
                for r in replies {
                    match r {
                        Ok((200, _)) => done.fetch_add(1, Ordering::Relaxed),
                        _ => errors.fetch_add(1, Ordering::Relaxed),
                    };
                }
            }
            lat.lock().unwrap().extend(mine);
        }));
    }
    for t in tasks {
        let _ = t.await;
    }
    let lat = std::mem::take(&mut *lat.lock().unwrap());
    Outcome { requests: done.load(Ordering::Relaxed), errors: errors.load(Ordering::Relaxed), pushes: 0, lat }
}

/// `clients` sockets watching the standings for `dur`.
async fn run_ws(ws_base: &str, players: &[(u64, String)], clients: usize, dur: Duration, watch: bool) -> Outcome {
    let pushes = Arc::new(AtomicU64::new(0));
    let errors = Arc::new(AtomicU64::new(0));
    let ok = Arc::new(AtomicU64::new(0));
    let lat = Arc::new(Mutex::new(Vec::new()));
    let end = Instant::now() + dur;
    let mut tasks = Vec::new();
    for c in 0..clients {
        let url = format!("{ws_base}/ws?token={}", players[c % players.len()].1);
        let (pushes, errors, ok, lat) = (pushes.clone(), errors.clone(), ok.clone(), lat.clone());
        tasks.push(tokio::spawn(async move {
            let Ok((mut ws, _)) = tokio_tungstenite::connect_async(url).await else {
                errors.fetch_add(1, Ordering::Relaxed);
                return;
            };
            let sent = Instant::now();
            let msg = json!({"type": "watch_season", "on": true, "limit": 5, "id": 1}).to_string();
            if watch && ws.send(Message::Text(msg)).await.is_err() {
                errors.fetch_add(1, Ordering::Relaxed);
                return;
            }
            let mut first = true;
            loop {
                let left = end.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    break;
                }
                let Ok(Some(Ok(m))) = tokio::time::timeout(left, ws.next()).await else { break };
                let Message::Text(t) = m else { continue };
                let v: Value = serde_json::from_str(&t).unwrap_or(Value::Null);
                match v["type"].as_str() {
                    Some("result") if v["command"] == "watch_season" => {
                        if v["ok"] == true {
                            ok.fetch_add(1, Ordering::Relaxed);
                        } else {
                            errors.fetch_add(1, Ordering::Relaxed);
                            break;
                        }
                    }
                    Some("result") if v["error"]["code"] == "bad_message" => {
                        errors.fetch_add(1, Ordering::Relaxed);
                        break;
                    }
                    Some("season") => {
                        if first {
                            lat.lock().unwrap().push(sent.elapsed().as_secs_f32() * 1000.0);
                            first = false;
                        }
                        pushes.fetch_add(1, Ordering::Relaxed);
                    }
                    _ => {}
                }
            }
            let _ = ws.close(None).await;
        }));
    }
    for t in tasks {
        let _ = t.await;
    }
    let lat = std::mem::take(&mut *lat.lock().unwrap());
    Outcome {
        requests: ok.load(Ordering::Relaxed),
        errors: errors.load(Ordering::Relaxed),
        pushes: pushes.load(Ordering::Relaxed),
        lat,
    }
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
    let data = args
        .data
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join(format!("evo-standings-load-{}", std::process::id())));
    std::fs::create_dir_all(&data).expect("data dir");
    let port = free_port();
    let base = format!("http://127.0.0.1:{port}");
    let http = Http::new(&base);

    // Warm up: fast mode until enough ranked seasons have ended.
    let t0 = Instant::now();
    let mut server = spawn(&args, &data, port, true, "warm.log");
    wait_up(&http).await;
    let mut players = Vec::new();
    for chunk in (0..args.players).collect::<Vec<_>>().chunks(8) {
        let got = futures_util::future::join_all(chunk.iter().map(|&i| setup_player(&http, i))).await;
        players.extend(got.into_iter().flatten());
    }
    assert!(!players.is_empty(), "no player could register");
    drop(players);
    loop {
        let total = http.get("/api/seasons?limit=1").await.and_then(|v| v["total"].as_u64()).unwrap_or(0);
        if total as usize >= args.seasons {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    stop(&mut server);
    let mut server = spawn(&args, &data, port, false, "run.log");
    wait_up(&http).await;
    // Sessions don't survive a restart: log in again.
    let mut players = Vec::new();
    for chunk in (0..args.players).collect::<Vec<_>>().chunks(8) {
        let got = futures_util::future::join_all(chunk.iter().map(|&i| setup_player(&http, i))).await;
        players.extend(got.into_iter().flatten());
    }
    let pid = server.child.id();
    let world = http.get("/api/world").await.unwrap_or_default();
    let cur = http.get("/api/seasons/current?limit=1").await.unwrap_or_default();
    println!(
        "ready in {:.0} s: {} players, day {}, season {} under way ({} finished), store {}",
        t0.elapsed().as_secs_f64(),
        players.len(),
        world["clock"]["day"],
        cur["season"],
        http.get("/api/seasons?limit=1").await.unwrap_or_default()["total"],
        if args.database_url.is_some() { "postgres" } else { "file" },
    );
    let dur = Duration::from_secs(args.duration);
    // Idle CPU over the same duration (the simulation itself).
    let c0 = cpu_of(pid);
    tokio::time::sleep(dur).await;
    let idle = (cpu_of(pid) - c0) / dur.as_secs_f64();
    let mode = if args.rate > 0.0 { format!("open loop, {} rounds/s", args.rate) } else { "closed loop".into() };
    println!(
        "idle server CPU {:.1}% of a core; {} clients per scenario, {} s each, {mode}",
        idle * 100.0,
        args.clients,
        args.duration
    );
    println!(
        "{:<8} {:>9} {:>8} {:>8} {:>8} {:>10} {:>11} {:>8}",
        "scenario", "req/s", "p50 ms", "p99 ms", "errors", "CPU %", "CPU us/req", "tick p99"
    );
    let ws_base = base.replacen("http", "ws", 1);
    for s in &args.scenarios {
        let c0 = cpu_of(pid);
        let started = Instant::now();
        let mut out = if s == "ws" || s == "ws-idle" {
            run_ws(&ws_base, &players, args.clients, dur, s == "ws").await
        } else {
            run_http(&http, &players, s, args.clients, dur, args.rate).await
        };
        let wall = started.elapsed().as_secs_f64();
        let cpu = cpu_of(pid) - c0;
        let above = (cpu - idle * wall).max(0.0);
        let m: Option<MetricsView> = http.get("/api/metrics").await.and_then(|v| serde_json::from_value(v).ok());
        let tick = m.map(|m| m.tick_ms.p99).unwrap_or(0.0);
        let rate = out.requests as f64 / wall;
        let per = if out.requests > 0 { above / out.requests as f64 * 1e6 } else { 0.0 };
        println!(
            "{:<8} {:>9.0} {:>8.2} {:>8.2} {:>8} {:>10.1} {:>11.1} {:>8.2}{}",
            s,
            rate,
            quantile(&mut out.lat, 0.5),
            quantile(&mut out.lat, 0.99),
            out.errors,
            above / wall * 100.0,
            per,
            tick,
            if s.starts_with("ws") {
                format!("  ({} watching, {} standings pushes received)", out.requests, out.pushes)
            } else {
                String::new()
            },
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    stop(&mut server);
    if args.data.is_none() {
        let _ = std::fs::remove_dir_all(&data);
    }
}

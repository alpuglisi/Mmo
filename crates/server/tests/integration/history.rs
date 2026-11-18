//! The daily history of a long-running world stays bounded: a file-store
//! server runs a world for more than two years of simulated days, and its
//! history keeps every day of the last year (the newest whole, older ones
//! thinned), samples of the years before, and a bounded number of segment
//! files; the history endpoint reads day ranges and caps its limit.

use std::net::SocketAddr;
use std::path::PathBuf;

use evo_server::game::WorldSettings;
use evo_server::history::{self, DailyQuery};
use evo_server::store::{FileStore, Store};
use evo_server::{start, ServerConfig};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::{Method, Request, StatusCode};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use serde_json::{json, Value};

/// Ticks per simulated day.
const DAY: u32 = 48;
/// Simulated days the world runs: past two years of the window.
const DAYS: u64 = 2 * history::WINDOW_DAYS + 100;

fn data_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("evo-server-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

async fn json_req(
    addr: SocketAddr,
    method: Method,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let client = Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();
    let mut req = Request::builder().method(method).uri(format!("http://{addr}{path}"));
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
    let res = client.request(req.body(body).unwrap()).await.expect("request");
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn get(addr: SocketAddr, path: &str) -> Value {
    let (s, v) = json_req(addr, Method::GET, path, None, None).await;
    assert_eq!(s, StatusCode::OK, "{path}: {v}");
    v
}

fn days_of(v: &Value) -> Vec<u64> {
    v.as_array().unwrap().iter().map(|d| d["day"].as_u64().unwrap()).collect()
}

/// Bytes of every file under `dir`.
fn dir_bytes(dir: &std::path::Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|entries| entries.filter_map(|e| e.ok()?.metadata().ok()).map(|m| m.len()).sum())
        .unwrap_or(0)
}

/// Long (830 simulated days, about 35 s): left out of a plain `cargo test`;
/// CI runs it (`cargo test --release --workspace --test integration --
/// --ignored`, README "Continuous integration").
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "long: cargo test --release --workspace --test integration -- --ignored"]
async fn a_long_running_worlds_history_stays_bounded() {
    let dir = data_dir("history-bounded");
    let cfg = ServerConfig {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        data_dir: Some(dir.clone()),
        world: WorldSettings { seed: 5, width: 48, height: 48, wild: true, catastrophes: false, ..Default::default() },
        ..Default::default()
    }
    .manual();
    let srv = start(cfg).await.expect("server starts");
    let addr = srv.addr;
    let mut tokens = Vec::new();
    for name in ["ann", "bob"] {
        let creds = json!({"name": name, "password": "a long password"});
        let (s, v) = json_req(addr, Method::POST, "/api/register", None, Some(creds)).await;
        tokens.push(v["token"].as_str().unwrap_or_default().to_string());
        assert_eq!(s, StatusCode::CREATED, "{v}");
        let design = json!({
            "name": format!("{name}'s"),
            "thermo": "warm",
            "birth": "live",
            "litter_size": 3,
            "traits": {"body_size": 0.3, "fur": 0.4},
            "founders": 12
        });
        let (s, v) = json_req(addr, Method::POST, "/api/lineage", v["token"].as_str(), Some(design)).await;
        assert_eq!(s, StatusCode::CREATED, "{v}");
    }

    let history_dir = dir.join("worlds").join("main").join("history");
    let mut peak_bytes = 0;
    let mut stepped = 0;
    while stepped < DAYS {
        let chunk = 50.min(DAYS - stepped);
        srv.step((chunk as u32) * DAY).await;
        stepped += chunk;
        peak_bytes = peak_bytes.max(dir_bytes(&history_dir));
    }
    let newest = get(addr, "/api/leaderboard/history?limit=1").await;
    let last_day = days_of(&newest)[0];
    assert!(last_day + 2 >= DAYS, "the world reached day {last_day}");

    // Day ranges, a capped limit, records without gene means.
    let range = get(addr, &format!("/api/leaderboard/history?from={}&to={}", last_day - 9, last_day - 5)).await;
    assert_eq!(days_of(&range), (last_day - 9..=last_day - 5).collect::<Vec<_>>());
    // Gene means need a login; without one they are left out. A player
    // gets every gene of their own lineage, only coarse looks of others'.
    let (s, _) = json_req(addr, Method::GET, "/api/leaderboard/history?limit=1&genes=true", None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert!(newest[0]["lineages"].as_array().unwrap().iter().all(|l| l.get("genes").is_none()), "{newest}");
    let (s, mine) = json_req(addr, Method::GET, "/api/leaderboard/history?limit=1", Some(&tokens[0]), None).await;
    assert_eq!(s, StatusCode::OK);
    let lineages = mine[0]["lineages"].as_array().unwrap();
    let genes = |owner_is_ann: bool| {
        lineages
            .iter()
            .filter(|l| l["name"].as_str().is_some_and(|n| n.starts_with("ann") == owner_is_ann) && l["owner"] != 0)
            .filter_map(|l| l["genes"].as_object())
            .map(|g| g.len())
            .collect::<Vec<_>>()
    };
    assert!(genes(true).iter().all(|&n| n > 30) && !genes(true).is_empty(), "{mine}");
    assert!(genes(false).iter().all(|&n| n <= 3), "{mine}");
    let capped = get(addr, "/api/leaderboard/history?limit=100000&genes=false").await;
    assert_eq!(capped.as_array().unwrap().len(), history::MAX_LIMIT);
    assert!(capped.as_array().unwrap().iter().all(|d| d["lineages"]
        .as_array()
        .unwrap()
        .iter()
        .all(|l| l.get("genes").is_none())));
    srv.shutdown().await;

    let store = FileStore::open(&dir).await.unwrap();
    let all = store.daily_range("main", &DailyQuery::recent(usize::MAX)).await.unwrap();
    let (bytes, files) = store.history_usage("main").await.unwrap();
    let segment = 32usize;
    // Every day of the window, then one in 8 (16 past two windows).
    let window: Vec<u64> = all.iter().map(|r| r.day).filter(|&d| last_day - d < history::WINDOW_DAYS).collect();
    assert_eq!(window, (last_day + 1 - history::WINDOW_DAYS..=last_day).collect::<Vec<_>>());
    let bound = history::WINDOW_DAYS as usize + 2 * (history::WINDOW_DAYS / history::ARCHIVE_STRIDE) as usize + segment;
    assert!(all.len() <= bound, "{} records for {last_day} days (bound {bound})", all.len());
    let with_genes = all.iter().filter(|r| r.lineages.iter().any(|l| !l.genes.is_empty())).count();
    let gene_bound = (history::FULL_DAYS + history::WINDOW_DAYS / history::GENE_STRIDE) as usize + segment + 1;
    assert!(with_genes <= gene_bound, "{with_genes} records with gene means (bound {gene_bound})");
    assert!(files <= bound / segment + 4, "{files} segment files");
    // Nothing on disk beyond the records (no replaced copies or leftovers).
    let serialized: usize = all.iter().map(|r| serde_json::to_string(r).unwrap().len() + 1).sum();
    assert!(bytes as usize <= serialized + serialized / 10, "{bytes} bytes on disk for {serialized} of records");
    assert!(peak_bytes <= 2 * bytes.max(1) + 1_000_000, "peaked at {peak_bytes} bytes, {bytes} at the end");
    // Thinned records are much smaller than whole ones in this world, too.
    let whole = all.last().map(|r| serde_json::to_string(r).unwrap().len()).unwrap();
    eprintln!(
        "{last_day} days: {} records ({with_genes} with gene means) in {files} files, {bytes} bytes \
         (newest record {whole} bytes; whole records every day would be ~{} bytes)",
        all.len(),
        whole as u64 * last_day
    );
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

//! The API answers cross-origin requests, so a Capacitor build (or any
//! bundle served from another origin) can talk to a remote server.

use std::net::SocketAddr;

use evo_server::game::WorldSettings;
use evo_server::{start, ServerConfig};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::{Method, Request, StatusCode};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;

async fn send(
    addr: SocketAddr,
    method: Method,
    path: &str,
    headers: &[(&str, &str)],
) -> (StatusCode, hyper::HeaderMap) {
    let client = Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();
    let mut req = Request::builder().method(method).uri(format!("http://{addr}{path}"));
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let res = client.request(req.body(Full::new(Bytes::new())).unwrap()).await.expect("request");
    let (status, headers) = (res.status(), res.headers().clone());
    let _ = res.into_body().collect().await;
    (status, headers)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn api_allows_cross_origin_clients() {
    let cfg = ServerConfig {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        data_dir: None,
        world: WorldSettings {
            seed: 3,
            width: 32,
            height: 32,
            wild: false,
            days_per_year: 8,
            lod: false,
            catastrophes: false,
        },
        ..Default::default()
    }
    .fast();
    let srv = start(cfg).await.expect("server starts");
    let origin = "capacitor://localhost";

    // Preflight for an authenticated JSON POST.
    let (s, h) = send(
        srv.addr,
        Method::OPTIONS,
        "/api/lineage",
        &[
            ("origin", origin),
            ("access-control-request-method", "POST"),
            ("access-control-request-headers", "authorization,content-type"),
        ],
    )
    .await;
    assert!(s.is_success(), "preflight status {s}");
    assert_eq!(h["access-control-allow-origin"], "*");
    let methods = h["access-control-allow-methods"].to_str().unwrap().to_ascii_uppercase();
    assert!(methods.contains("POST"), "{methods}");
    let allowed = h["access-control-allow-headers"].to_str().unwrap().to_ascii_lowercase();
    assert!(allowed.contains("authorization") && allowed.contains("content-type"), "{allowed}");

    // A simple GET carries the header too.
    let (s, h) = send(srv.addr, Method::GET, "/api/world", &[("origin", origin)]).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(h["access-control-allow-origin"], "*");
}

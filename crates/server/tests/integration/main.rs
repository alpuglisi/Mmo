//! The server's integration tests, one module per area, in one test binary:
//! each binary links the whole server (axum, hyper, tokio, sqlx), so one
//! binary instead of eleven saves most of `cargo test`'s build time. Run one
//! area with `cargo test --release -p evo-server --test integration persistence::`.

mod cors;
mod dashboard;
mod history;
mod native_push;
mod persistence;
mod push;
mod security;
mod server;
mod standings;
mod worlds;
mod ws_compression;

/// The operator token the tests' servers take (`ServerConfig::admin_token`).
pub const OPERATOR: &str = "test-operator-token";

/// `GET path` with the operator token: `/api/metrics` counts players and
/// their activity only for it (playtest 10).
pub async fn operator_get(addr: std::net::SocketAddr, path: &str) -> serde_json::Value {
    use http_body_util::{BodyExt, Full};
    use hyper::body::Bytes;
    let client =
        hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new()).build_http::<Full<Bytes>>();
    let req = hyper::Request::builder()
        .uri(format!("http://{addr}{path}"))
        .header("x-admin-token", OPERATOR)
        .body(Full::new(Bytes::new()))
        .unwrap();
    let res = client.request(req).await.expect("request");
    assert_eq!(res.status(), hyper::StatusCode::OK);
    serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

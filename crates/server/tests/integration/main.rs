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

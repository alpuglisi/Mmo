# syntax=docker/dockerfile:1
#
# The Evolution MMO server with the web client, in one small image.
#
#   docker build -t evo-server .
#   docker run --rm -p 8080:8080 -v evo-data:/var/lib/evo evo-server   # file store in /var/lib/evo/data
#   docker run --rm -p 8080:8080 -e DATABASE_URL=postgres://... evo-server
#
# Extra flags go after the image name (they replace the default `CMD`, so
# repeat the ones below that you still want). See deploy/README.md for the
# compose setup with PostgreSQL and HTTPS.

# Keep in step with rust-toolchain.toml.
ARG RUST_VERSION=1.97.0
ARG NODE_VERSION=22

# --- The web client (client/dist) ---------------------------------------
FROM node:${NODE_VERSION}-trixie-slim AS client
WORKDIR /src/client
COPY client/package.json client/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci --no-audit --no-fund
COPY client/ ./
RUN npm run build

# --- evo-server (release) ------------------------------------------------
FROM rust:${RUST_VERSION}-slim-trixie AS server
WORKDIR /src
# The toolchain file is left out on purpose: the image already has the
# pinned compiler, and the file would make rustup fetch clippy and rustfmt.
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
# No debuginfo in the shipped binary (the workspace keeps it for profiling).
ENV CARGO_PROFILE_RELEASE_DEBUG=0 \
    CARGO_INCREMENTAL=0
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p evo-server --bin evo-server \
    && cp target/release/evo-server /usr/local/bin/evo-server

# --- Runtime ---------------------------------------------------------------
FROM debian:trixie-slim AS runtime
# curl for the health check only; ca-certificates for outbound HTTPS
# (push services) should a dependency ever look at the system store.
# Not version-pinned: Debian replaces point versions, so pins would break
# the build; the base image tag fixes the release.
# hadolint ignore=DL3008
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --user-group --home-dir /var/lib/evo --shell /usr/sbin/nologin evo \
    && mkdir -p /var/lib/evo \
    && chown evo:evo /var/lib/evo
COPY --from=server /usr/local/bin/evo-server /usr/local/bin/evo-server
COPY --from=client /src/client/dist /usr/share/evo/client
USER evo
WORKDIR /var/lib/evo
# The file store (when DATABASE_URL is not set) writes to ./data, i.e.
# /var/lib/evo/data. No `--data` flag below: it would override DATABASE_URL.
VOLUME ["/var/lib/evo"]
EXPOSE 8080
# Plain log lines (no ANSI colours) for `docker logs`.
ENV RUST_LOG=info \
    NO_COLOR=1
# `/api/metrics` is public, cheap (no store, no world-thread round trip)
# and answers only once every world has loaded.
HEALTHCHECK --interval=30s --timeout=5s --start-period=120s --retries=3 \
    CMD ["curl", "-fsS", "-o", "/dev/null", "http://127.0.0.1:8080/api/metrics"]
ENTRYPOINT ["evo-server"]
CMD ["--bind", "0.0.0.0:8080", "--static", "/usr/share/evo/client"]

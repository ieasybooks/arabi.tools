# syntax=docker/dockerfile:1

FROM rust:1.99.1-trixie AS builder
ENV CARGO_TARGET_DIR=/build/target
WORKDIR /build

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/build/target \
    cargo install --locked --version 0.10.0 topcoat-cli

COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY src/ src/
# Only for Cargo to resolve the lockfile; the image does not build it.
COPY crates/ crates/
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/build/target \
    cargo build --locked --release --bin arlang-tools \
    && topcoat asset bundle --release --bin arlang-tools \
    && mkdir -p /out \
    && cp target/release/arlang-tools /out/ \
    && cp -a target/release/assets /out/assets

FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl ffmpeg \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home arlang
WORKDIR /app
COPY --from=builder /out/ ./
USER arlang
ENV HOST=0.0.0.0 PORT=3000 TMPDIR=/tmp
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=5s --start-period=60s --retries=3 \
    CMD curl --fail --silent --show-error --max-time 4 "http://127.0.0.1:${PORT}/health" || exit 1
STOPSIGNAL SIGTERM
ENTRYPOINT ["/app/arlang-tools"]

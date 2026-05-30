# ---- production builder ----
FROM rust:1.96-slim AS builder
WORKDIR /app
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
ENV SQLX_OFFLINE=true
RUN mkdir src && echo "fn main() {}" > src/main.rs && cargo build --release && rm -rf src
COPY .sqlx ./.sqlx
COPY src ./src
RUN touch src/main.rs && cargo build --release

# ---- production runtime ----
FROM debian:bookworm-slim
WORKDIR /app
RUN apt-get update && apt-get install -y ca-certificates libssl3 curl && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/archypix-back /usr/local/bin/archypix-back
RUN useradd -m -u 1000 archypix && chown -R archypix:archypix /app
USER archypix
EXPOSE 8000
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 CMD curl -f http://localhost:8000/health || exit 1
CMD ["archypix-back"]

# ---- dev (source mounted at runtime, cargo run on start) ----
# Usage: build with --target dev, mount ./src, Cargo.toml, Cargo.lock, .sqlx as volumes.
# Named volume on /app/target persists compiled deps across restarts.
FROM rust:1.96-slim AS dev
WORKDIR /app
RUN apt-get update && apt-get install -y pkg-config libssl-dev curl && rm -rf /var/lib/apt/lists/*
EXPOSE 8000
CMD ["cargo", "run"]

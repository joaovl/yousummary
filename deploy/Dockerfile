FROM rust:1.83-slim-bookworm AS builder
WORKDIR /src
RUN apt-get update && apt-get install -y --no-install-recommends \
      pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release && strip target/release/yousummary

FROM debian:bookworm-slim
# nodejs/npm are here only to host the `claude` CLI that the claude-cli
# provider shells out to (subscription auth, no API key).
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates libssl3 nodejs npm \
    && npm install -g @anthropic-ai/claude-code \
    && npm cache clean --force \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /src/target/release/yousummary /usr/local/bin/yousummary
EXPOSE 8000
ENV RUST_LOG=info HOME=/root
CMD ["yousummary","serve","--host","0.0.0.0","--port","8000","--default-model","claude-cli/sonnet"]

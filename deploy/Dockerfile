FROM rust:1-slim-bookworm AS builder
WORKDIR /src
RUN apt-get update && apt-get install -y --no-install-recommends \
      pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release && strip target/release/yousummary

FROM debian:bookworm-slim
# nodejs/npm host the `claude` CLI that the claude-cli provider shells out to
# (subscription auth, no API key); node doubles as yt-dlp's JS runtime.
# yt-dlp + the bgutil PO-token plugin fetch subtitles — YouTube's player API no
# longer serves caption tracks to anonymous HTTP clients.
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates libssl3 nodejs npm python3 python3-pip \
    && npm install -g @anthropic-ai/claude-code \
    && npm cache clean --force \
    && pip3 install --no-cache-dir --break-system-packages \
         yt-dlp bgutil-ytdlp-pot-provider \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /src/target/release/yousummary /usr/local/bin/yousummary
EXPOSE 8000
ENV RUST_LOG=info HOME=/root
CMD ["yousummary","serve","--host","0.0.0.0","--port","8000","--default-model","claude-cli/sonnet"]

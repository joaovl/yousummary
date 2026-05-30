FROM rust:1.83-slim-bookworm AS builder
WORKDIR /src
RUN apt-get update && apt-get install -y --no-install-recommends \
      pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release && strip target/release/yousummary

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd -r app && useradd -r -g app -d /app -m app
WORKDIR /app
COPY --from=builder /src/target/release/yousummary /usr/local/bin/yousummary
USER app
EXPOSE 8000
ENV RUST_LOG=info
CMD ["yousummary","serve","--host","0.0.0.0","--port","8000","--default-model","anthropic/claude-haiku-4-5-20251001"]

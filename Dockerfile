FROM rust:1-bookworm AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY vendor ./vendor

RUN cargo build --release --locked

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/tokscale-bundle /usr/local/bin/tokscale-bundle

EXPOSE 4787
VOLUME ["/data"]

ENTRYPOINT ["tokscale-bundle"]
CMD ["serve", "--bind", "0.0.0.0:4787", "--storage", "/data", "--submit"]

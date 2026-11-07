FROM rust:1.99.1-bookworm AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* && useradd --create-home --uid 10001 app
COPY --from=build /app/target/release/iptv-slayer /usr/local/bin/iptv-slayer
RUN install -d -o app -g app -m 0700 /var/lib/iptv-slayer
ENV IPTV_SLAYER_DATA_DIR=/var/lib/iptv-slayer
USER app
ENTRYPOINT ["iptv-slayer"]
CMD ["--help"]

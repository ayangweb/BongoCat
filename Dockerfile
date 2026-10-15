FROM rust:1.97.1-slim-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
RUN cargo build --locked --release -p bongocat-room-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 bongocat \
    && useradd --uid 10001 --gid 10001 --no-create-home bongocat
COPY --from=builder /build/target/release/bongocat-room-server /usr/local/bin/bongocat-room-server
USER 10001:10001
ENV BONGOCAT_ROOM_LISTEN=0.0.0.0:8080
EXPOSE 8080/tcp
HEALTHCHECK --interval=10s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8080/health || exit 1
ENTRYPOINT ["bongocat-room-server"]

FROM rust@sha256:606fd313a0f49743ee2a7bd49a0914bab7deedb12791f3a846a34a4711db7ed2 AS toolchain

RUN apk update \
    && apk upgrade \
    && apk add --no-cache \
        build-base \
        ca-certificates \
        musl-dev \
        openssl-dev \
        openssl-libs-static \
        pkgconfig \
        tzdata

WORKDIR /app

FROM toolchain AS develop
RUN cargo install cargo-watch

FROM toolchain AS builder
ENV OPENSSL_STATIC=1 \
    RUSTFLAGS="-C target-feature=+crt-static -C target-cpu=native"

COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    export TARGET_TRIPLE=$(rustc -vV | sed -n 's/host: //p') && \
    cargo build --release --target "$TARGET_TRIPLE" --bins && \
    mkdir -p /rootfs/etc/ssl/certs \
             /rootfs/usr/share/zoneinfo/Asia \
             /rootfs/usr/local/bin && \
    install -Dm755 "target/$TARGET_TRIPLE/release/discord-bot" /rootfs/usr/local/bin/discord-bot && \
    install -Dm755 "target/$TARGET_TRIPLE/release/healthcheck" /rootfs/usr/local/bin/healthcheck && \
    cp /etc/ssl/certs/ca-certificates.crt /rootfs/etc/ssl/certs/ca-certificates.crt && \
    cp /usr/share/zoneinfo/Asia/Bangkok /rootfs/usr/share/zoneinfo/Asia/Bangkok

FROM scratch AS runtime
COPY --from=builder --chown=1000:1000 /rootfs/ /

USER 1000:1000

ENV SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt \
    TZ=Asia/Bangkok

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 CMD ["healthcheck"]

ENTRYPOINT ["discord-bot"]

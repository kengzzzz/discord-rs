FROM rust:alpine@sha256:ff0adc35894eb79586ce752a1b5a9eadc88b938c56d8f2b4b537b6258ff3fa10 AS base

RUN apk update && apk add --no-cache musl-dev build-base pkgconfig openssl-dev openssl-libs-static ca-certificates tzdata

WORKDIR /app

FROM base AS develop
RUN cargo install cargo-watch

FROM base AS builder
ENV OPENSSL_STATIC=1 \
    RUSTFLAGS="-C target-feature=+crt-static -C target-cpu=native"

COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    export TARGET_TRIPLE=$(rustc -vV | sed -n 's/host: //p') && \
    cargo build --release --target "$TARGET_TRIPLE" --bins && \
    mkdir -p /rootfs/etc/ssl/certs \
             /rootfs/usr/share/zoneinfo/Asia \
             /rootfs/usr/local/bin && \
    cp target/$TARGET_TRIPLE/release/discord-bot /rootfs/usr/local/bin/ && \
    cp target/$TARGET_TRIPLE/release/healthcheck /rootfs/usr/local/bin/ && \
    cp /etc/ssl/certs/ca-certificates.crt /rootfs/etc/ssl/certs/ && \
    cp /usr/share/zoneinfo/Asia/Bangkok /rootfs/usr/share/zoneinfo/Asia/

FROM scratch
COPY --from=builder /rootfs /

USER 1000:1000

ENV TZ=Asia/Bangkok

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 CMD ["healthcheck"]

ENTRYPOINT ["discord-bot"]
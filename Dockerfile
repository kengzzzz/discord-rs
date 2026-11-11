# syntax=docker/dockerfile:1
ARG OCRS_MODELS_URL=https://ocrs-models.s3-accelerate.amazonaws.com
ARG OCRS_DETECTION_SHA256=a917b23dbd9524b465df7e922641b3ff2981623df4ded5a0234004ef2fee7cfe
ARG OCRS_RECOGNITION_SHA256=86c145c2edb96c8caed5b1ebb8f44d706408922211451c309c625157dd6061c5

FROM rust:1.99.1-alpine3.24@sha256:44819b27be4c97b0e4d7203bd2e02c4820f97700211b6a37fb4c246a2de59383 AS builder

ARG OCRS_MODELS_URL
ARG OCRS_DETECTION_SHA256
ARG OCRS_RECOGNITION_SHA256
ARG CARGO_FEATURES=""

RUN apk add --no-cache build-base ca-certificates curl tzdata

RUN set -eux; \
    install -d /rootfs/usr/share/ocrs /rootfs/etc/ssl/certs /rootfs/usr/share/zoneinfo/Asia /rootfs/data; \
    curl -fsSL "${OCRS_MODELS_URL}/text-detection.onnx" -o /rootfs/usr/share/ocrs/text-detection.onnx; \
    curl -fsSL "${OCRS_MODELS_URL}/text-recognition.onnx" -o /rootfs/usr/share/ocrs/text-recognition.onnx; \
    echo "${OCRS_DETECTION_SHA256}  /rootfs/usr/share/ocrs/text-detection.onnx" | sha256sum -c -; \
    echo "${OCRS_RECOGNITION_SHA256}  /rootfs/usr/share/ocrs/text-recognition.onnx" | sha256sum -c -; \
    cp /etc/ssl/certs/ca-certificates.crt /rootfs/etc/ssl/certs/ca-certificates.crt; \
    cp /usr/share/zoneinfo/Asia/Bangkok /rootfs/usr/share/zoneinfo/Asia/Bangkok

WORKDIR /app
COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    set -eux; \
    export RUSTFLAGS="-C target-cpu=native"; \
    if [ -n "${CARGO_FEATURES}" ]; then \
        cargo build --locked --release --bins --features "${CARGO_FEATURES}"; \
    else \
        cargo build --locked --release --bins; \
    fi; \
    install -Dm755 target/release/discord-bot /rootfs/usr/local/bin/discord-bot; \
    install -Dm755 target/release/healthcheck /rootfs/usr/local/bin/healthcheck; \
    if [ -f target/release/migrate ] && [ -n "${CARGO_FEATURES}" ]; then \
        install -Dm755 target/release/migrate /rootfs/usr/local/bin/migrate; \
    fi

FROM scratch AS runtime
COPY --from=builder --chown=1000:1000 /rootfs/ /

USER 1000:1000

ENV SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt \
    TZ=Asia/Bangkok \
    DATABASE_PATH=/data/bot.sqlite3 \
    SCAM_DETECT_MODELS_DIR=/usr/share/ocrs \
    RTEN_NUM_THREADS=2

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 CMD ["healthcheck"]

ENTRYPOINT ["discord-bot"]

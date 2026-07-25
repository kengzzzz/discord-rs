ARG TESSERACT_VERSION=5.5.3
ARG TESSERACT_SHA256=9218e62793116d42a9f6d14cd9348518b27f382096eea3d0f2d1a24616bb5884
ARG LEPTONICA_VERSION=1.87.0
ARG LEPTONICA_SHA256=c73363397f96eb1295602bf44d708a994ad42046c791bf03ea0505d829bdb6a7
ARG TESSDATA_COMMIT=87416418657359cb625c412a48b6e1d6d41c29bd
ARG TESSDATA_ENG_SHA256=7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2

FROM rust@sha256:3c38f3f82c2f3d73da3b38e18d279393a04cb43ddded0e35088a8c3324d40900 AS toolchain

ARG TESSERACT_VERSION
ARG TESSERACT_SHA256
ARG LEPTONICA_VERSION
ARG LEPTONICA_SHA256
ARG TESSDATA_COMMIT
ARG TESSDATA_ENG_SHA256

RUN apk update \
    && apk upgrade \
    && apk add --no-cache \
        build-base \
        ca-certificates \
        clang \
        clang-dev \
        cmake \
        curl \
        file \
        lld \
        musl-dev \
        ninja \
        openssl-dev \
        openssl-libs-static \
        libpng-dev \
        libpng-static \
        pkgconfig \
        tzdata \
        zlib-dev \
        zlib-static

WORKDIR /native

RUN set -eux; \
    curl -fsSL "https://github.com/DanBloomberg/leptonica/releases/download/${LEPTONICA_VERSION}/leptonica-${LEPTONICA_VERSION}.tar.gz" -o leptonica.tar.gz; \
    echo "${LEPTONICA_SHA256}  leptonica.tar.gz" | sha256sum -c -; \
    curl -fsSL "https://github.com/tesseract-ocr/tesseract/archive/refs/tags/${TESSERACT_VERSION}.tar.gz" -o tesseract.tar.gz; \
    echo "${TESSERACT_SHA256}  tesseract.tar.gz" | sha256sum -c -; \
    tar -xzf leptonica.tar.gz; \
    tar -xzf tesseract.tar.gz; \
    install -d /opt/ocr/share/tessdata; \
    curl -fsSL "https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/${TESSDATA_COMMIT}/eng.traineddata" -o /opt/ocr/share/tessdata/eng.traineddata; \
    echo "${TESSDATA_ENG_SHA256}  /opt/ocr/share/tessdata/eng.traineddata" | sha256sum -c -

RUN set -eux; \
    case "$(uname -m)" in \
        aarch64|arm64) CPU_OPT="-mcpu=native" ;; \
        x86_64|amd64) CPU_OPT="-march=native" ;; \
        *) echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;; \
    esac; \
    common_flags="-O3 -ffunction-sections -fdata-sections ${CPU_OPT}"; \
    export CC=clang CXX=clang++; \
    cmake -S "/native/leptonica-${LEPTONICA_VERSION}" -B /tmp/lept-static -G Ninja \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX=/opt/ocr \
        -DBUILD_SHARED_LIBS=OFF \
        -DENABLE_ZLIB=ON \
        -DENABLE_PNG=ON \
        -DENABLE_GIF=OFF \
        -DENABLE_JPEG=OFF \
        -DENABLE_TIFF=OFF \
        -DENABLE_WEBP=OFF \
        -DENABLE_OPENJPEG=OFF \
        -DCMAKE_C_FLAGS_RELEASE="$common_flags"; \
    cmake --build /tmp/lept-static; \
    cmake --install /tmp/lept-static; \
    PKG_CONFIG_PATH=/opt/ocr/lib/pkgconfig cmake \
        -S "/native/tesseract-${TESSERACT_VERSION}" -B /tmp/tess-static -G Ninja \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX=/opt/ocr \
        -DCMAKE_PREFIX_PATH=/opt/ocr \
        -DBUILD_SHARED_LIBS=OFF \
        -DBUILD_TRAINING_TOOLS=OFF \
        -DOPENMP_BUILD=OFF \
        -DCMAKE_C_FLAGS_RELEASE="$common_flags" \
        -DCMAKE_CXX_FLAGS_RELEASE="$common_flags -DTESSERACT_DISABLE_DEBUG_FONTS" \
        -DCMAKE_EXE_LINKER_FLAGS="-fuse-ld=lld -Wl,--gc-sections"; \
    cmake --build /tmp/tess-static; \
    cmake --install /tmp/tess-static

WORKDIR /app

ENV PKG_CONFIG_PATH=/opt/ocr/lib/pkgconfig \
    PKG_CONFIG_ALL_STATIC=1 \
    TESSDATA_PREFIX=/opt/ocr/share/tessdata

FROM toolchain AS develop
ENV RUSTFLAGS="-C target-feature=-crt-static -C link-arg=-lstdc++"
RUN rustup component add clippy rustfmt \
    && cargo install cargo-watch

FROM toolchain AS builder
ENV OPENSSL_STATIC=1 \
    RUSTFLAGS="-C target-feature=-crt-static -C target-cpu=native -C link-arg=-fuse-ld=lld -C link-arg=-lstdc++"

COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release --bins && \
    mkdir -p /rootfs/etc/ssl/certs \
             /rootfs/lib \
             /rootfs/usr/lib \
             /rootfs/usr/share/zoneinfo/Asia \
             /rootfs/usr/local/bin && \
    install -Dm755 target/release/discord-bot /rootfs/usr/local/bin/discord-bot && \
    install -Dm755 target/release/healthcheck /rootfs/usr/local/bin/healthcheck && \
    for binary in \
        /rootfs/usr/local/bin/discord-bot \
        /rootfs/usr/local/bin/healthcheck; \
    do \
        ldd "$binary" | \
            awk '$2 == "=>" && $3 ~ /^\// { print $3 } $1 ~ /^\// { print $1 }'; \
    done | sort -u | while IFS= read -r library; do \
        install -Dm755 "$library" "/rootfs$library"; \
    done && \
    install -Dm644 /opt/ocr/share/tessdata/eng.traineddata /rootfs/usr/share/tessdata/eng.traineddata && \
    cp /etc/ssl/certs/ca-certificates.crt /rootfs/etc/ssl/certs/ca-certificates.crt && \
    cp /usr/share/zoneinfo/Asia/Bangkok /rootfs/usr/share/zoneinfo/Asia/Bangkok && \
    file /rootfs/usr/local/bin/discord-bot && \
    ldd /rootfs/usr/local/bin/discord-bot && \
    test -z "$(ldd /rootfs/usr/local/bin/discord-bot | grep 'not found')"

FROM scratch AS runtime
COPY --from=builder --chown=1000:1000 /rootfs/ /

USER 1000:1000

ENV SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt \
    TESSDATA_PREFIX=/usr/share/tessdata \
    TZ=Asia/Bangkok

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 CMD ["healthcheck"]

ENTRYPOINT ["discord-bot"]

# glibc binary + a few fonts in a distroless image (libc, CA certs, tzdata, nonroot user, no shell).
FROM rust:1 AS build
RUN apt-get update && apt-get install -y --no-install-recommends fonts-liberation fonts-dejavu-core \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /src
# dependencies first, cached as long as Cargo.toml / Cargo.lock don't change
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && touch src/lib.rs \
 && cargo build --release --locked && rm -rf src
COPY src src
RUN touch src/main.rs src/lib.rs && cargo build --release --locked
# Liberation: metric-compatible Helvetica / Times / Courier; DejaVu Sans: fallback for missing glyphs
RUN mkdir -p /out/fonts \
 && cd /usr/share/fonts/truetype \
 && cp liberation/LiberationSans-*.ttf liberation/LiberationSerif-*.ttf liberation/LiberationMono-*.ttf /out/fonts/ \
 && cp dejavu/DejaVuSans.ttf dejavu/DejaVuSans-Bold.ttf /out/fonts/

FROM gcr.io/distroless/cc-debian13:nonroot
COPY --from=build /src/target/release/pdfrs /pdfrs
COPY --from=build /out/fonts /fonts
# mimalloc: give freed memory back right away (about -50 % RSS under load, same throughput)
ENV FONTS_DIR=/fonts PORT=3000 MIMALLOC_PURGE_DELAY=0 MIMALLOC_ARENA_EAGER_COMMIT=0
EXPOSE 3000
ENTRYPOINT ["/pdfrs"]

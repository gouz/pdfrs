# Static musl binary + a few fonts in a distroless image (CA certs, tzdata, nonroot user, no shell, no libc).
FROM rust:alpine AS chef
RUN apk add --no-cache musl-dev font-liberation font-dejavu && cargo install cargo-chef --locked --version 0.1.78
WORKDIR /src

# the dependencies' recipe: Cargo.toml / Cargo.lock without our own version, so a version bump keeps the cache
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY src src
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build
COPY --from=planner /src/recipe.json recipe.json
# dependencies first, cached as long as the recipe does not change
RUN cargo chef cook --release --locked --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY src src
RUN cargo build --release --locked
# Liberation: metric-compatible Helvetica / Times / Courier; DejaVu Sans: fallback for missing glyphs
RUN mkdir -p /out/fonts \
 && cp /usr/share/fonts/liberation/*.ttf /out/fonts/ \
 && cp /usr/share/fonts/dejavu/DejaVuSans.ttf /usr/share/fonts/dejavu/DejaVuSans-Bold.ttf /out/fonts/

FROM gcr.io/distroless/static-debian13:nonroot
COPY --from=build /src/target/release/pdfrs /pdfrs
COPY --from=build /out/fonts /fonts
# mimalloc: give freed memory back right away (about -50 % RSS under load, same throughput)
ENV FONTS_DIR=/fonts PORT=3000 MIMALLOC_PURGE_DELAY=0 MIMALLOC_ARENA_EAGER_COMMIT=0
EXPOSE 3000
ENTRYPOINT ["/pdfrs"]

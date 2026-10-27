# syntax=docker/dockerfile:1
#
# The image of pacod-bench (the API, the run queue, and the command line). Build from the
# repository root:
#
#   docker build -f deploy/docker/rust.Dockerfile -t pacod-bench .
#
# The build context is narrowed by rust.Dockerfile.dockerignore (next to this file) to the
# Cargo workspace and the vendored crates. The layers follow pacod-gw's rust.Dockerfile: a
# cargo-chef recipe, third-party crates compiled once, then the workspace. The binary is linked
# statically against musl and runs on a distroless base without a shell; mimalloc replaces the
# musl allocator. It is its own health probe (`pacod-bench healthcheck`).

ARG RUST_VERSION=1.99.0
ARG CARGO_CHEF_VERSION=0.1.78
# The tag "nonroot" moves; the digest pins it (the same as pacod-gw's).
ARG RUNTIME_IMAGE=gcr.io/distroless/static-debian13:nonroot@sha256:e2e927ec666bae08560abb3c55d0659eceabb657f56b6782ab500a9fc7f555e3

FROM lukemathwalker/cargo-chef:${CARGO_CHEF_VERSION}-rust-${RUST_VERSION}-slim-trixie AS chef

RUN apt-get update \
 && apt-get install -y --no-install-recommends musl-tools \
 && rm -rf /var/lib/apt/lists/*

# Every cargo call below builds for <arch>-unknown-linux-musl, the platform being built: amd64
# and arm64 hosts build their own images.
RUN set -eu; \
    triple="$(uname -m)-unknown-linux-musl"; \
    rustup target add "$triple"; \
    printf '%s' "$triple" > /rust-target; \
    printf '[build]\ntarget = "%s"\n' "$triple" >> "$CARGO_HOME/config.toml"

ENV CARGO_INCREMENTAL=0 \
    CARGO_TERM_COLOR=never \
    CARGO_NET_RETRY=5

WORKDIR /src

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path /recipe.json

FROM chef AS deps
COPY --from=planner /recipe.json /recipe.json
# The recipe leaves out path dependencies outside the workspace: the vendored crates come as
# they are (they change only when vendored again).
COPY vendor/pacod-gw /src/vendor/pacod-gw
COPY vendor/pacod-id/crates /src/vendor/pacod-id/crates
RUN --mount=type=cache,id=pacod-bench-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=pacod-bench-cargo-git,target=/usr/local/cargo/git,sharing=locked \
    cargo chef cook --release --locked --workspace --recipe-path /recipe.json

FROM deps AS build
COPY . .
RUN --mount=type=cache,id=pacod-bench-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=pacod-bench-cargo-git,target=/usr/local/cargo/git,sharing=locked \
    set -eu; \
    cargo build --release --locked --workspace --bin pacod-bench; \
    install -D "target/$(cat /rust-target)/release/pacod-bench" /out/pacod-bench

# ---------------------------------------------------------------------------------------------
# bench-api: the API on 8190, metrics on 9190.
# ---------------------------------------------------------------------------------------------
FROM ${RUNTIME_IMAGE} AS api
ARG VERSION=dev
ARG VCS_REF=unknown
LABEL org.opencontainers.image.title="pacod-bench" \
      org.opencontainers.image.description="pacod-benchmark: benchmarks of pacod-gw, its proxy mode and other gateways" \
      org.opencontainers.image.licenses="Apache-2.0" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${VCS_REF}"
COPY --from=build /out/pacod-bench /usr/local/bin/pacod-bench
USER 65532:65532
WORKDIR /
ENV BENCH_LISTEN=0.0.0.0:8190 \
    BENCH_METRICS_LISTEN=0.0.0.0:9190
EXPOSE 8190 9190
STOPSIGNAL SIGTERM
ENTRYPOINT ["/usr/local/bin/pacod-bench"]
CMD ["serve"]

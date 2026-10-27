#!/usr/bin/env bash
# Native binaries of pacod-bench into dist/, with their SHA-256 sums: the command line
# (`pacod-bench run|plan|validate`) without Docker, on the machines that make the load or hold
# the results. The service (`serve`) runs from the image: it needs PostgreSQL, pacod-id and the
# UI besides.
#
#   scripts/build-binaries.sh            this machine's platform
#   TARGETS="x86_64-unknown-linux-musl aarch64-unknown-linux-musl" scripts/build-binaries.sh
#
# Linux binaries are static (musl). Cross-building them from macOS needs a musl linker (the
# image's builder has one: `docker build -f deploy/docker/rust.Dockerfile --target build .`
# builds the image platform's). macOS binaries are built on macOS.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
host="$(rustc -vV | sed -n 's/^host: //p')"
TARGETS="${TARGETS:-$host}"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
mkdir -p dist

for t in $TARGETS; do
  rustup target list --installed | grep -qx "$t" || rustup target add "$t"
  cargo build --release --locked -p pacod-bench --target "$t"
  out="dist/pacod-bench-$VERSION-$t"
  cp "target/$t/release/pacod-bench" "$out"
  echo "built $out"
done

(cd dist && shasum -a 256 pacod-bench-"$VERSION"-* > SHA256SUMS)
cat dist/SHA256SUMS

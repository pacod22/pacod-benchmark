#!/usr/bin/env bash
# Copies the crates of a pinned commit of pacod-gw that the benchmark builds on into
# vendor/pacod-gw/: the load agent's contract and histograms (pacod-gw-load), the log format
# of every pacod Rust binary (pacod-gw-logging), the mock provider and the in-process helpers
# the tests use (pacod-gw-testkit), and the benchmark report's contract
# (pacod-gw-bench-report). Builds then need nothing outside this
# repository: deny.toml refuses git sources.
#
#   scripts/vendor-pacod-gw.sh [<pacod-gw checkout>] [<commit>]
#   scripts/vendor-pacod-gw.sh --worktree [<pacod-gw checkout>]
#
# Defaults: the checkout next to this repository (../pacod-gw) and its HEAD. Only committed
# files are taken (git archive). vendor/pacod-gw/VERSION records the commit.
#
# --worktree copies the checkout's working tree instead, uncommitted changes included, while a
# change to both repositories is being made. VERSION then says so, and `make vendor-check`
# refuses such a copy: vendor again from a commit before this repository's change is done.
#
# The workspace's root manifest comes along: the crates inherit their versions and
# dependencies from it (`.workspace = true`). The other members of pacod-gw's workspace are
# left out; Cargo reads only the crates this repository depends on. The images of the agent,
# the mock and the minimal gateway are built from the checkout itself (make images).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORKTREE=0
if [ "${1:-}" = "--worktree" ]; then
  WORKTREE=1
  shift
fi
SRC="${1:-$(dirname "$ROOT")/pacod-gw}"
REV="${2:-HEAD}"
CRATES=(pacod-gw-load pacod-gw-logging pacod-gw-testkit pacod-gw-bench-report)

COMMIT="$(git -C "$SRC" rev-parse --verify "${REV}^{commit}")"
DEST="$ROOT/vendor/pacod-gw"

if [ "$WORKTREE" = 1 ]; then
  paths=(Cargo.toml Cargo.lock)
  for c in "${CRATES[@]}"; do
    if [ -d "$SRC/crates/$c" ]; then paths+=("crates/$c"); else echo "note: $c is not in $SRC; left out" >&2; fi
  done
  rm -rf "$DEST"
  mkdir -p "$DEST"
  (cd "$SRC" && tar --exclude target -cf - "${paths[@]}") | tar -x -C "$DEST"
  {
    echo "worktree of $COMMIT, with uncommitted changes"
    echo "date $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  } >"$DEST/VERSION"
  echo "vendored the working tree of pacod-gw ($COMMIT and its changes): ${paths[*]}"
  echo "vendor again from a commit before this change is done"
  exit 0
fi

if [ -n "$(git -C "$SRC" status --porcelain)" ]; then
  echo "note: $SRC has uncommitted changes; vendoring commit $COMMIT as committed" >&2
fi

paths=(Cargo.toml Cargo.lock)
for c in "${CRATES[@]}"; do
  if git -C "$SRC" cat-file -e "$COMMIT:crates/$c" 2>/dev/null; then
    paths+=("crates/$c")
  else
    echo "note: $c is not in pacod-gw at $COMMIT; left out" >&2
  fi
done

rm -rf "$DEST"
mkdir -p "$DEST"
git -C "$SRC" archive "$COMMIT" "${paths[@]}" | tar -x -C "$DEST"
{
  echo "commit $COMMIT"
  echo "date $(git -C "$SRC" show -s --format=%cI "$COMMIT")"
} >"$DEST/VERSION"

echo "vendored pacod-gw $COMMIT: ${paths[*]}"
echo "then: cargo build (Cargo.lock follows), make vendor-check"

#!/usr/bin/env bash
# Writes .env from .env.example with every empty secret filled in, and never changes a value that
# is there already: run it again after a new secret appears in .env.example. Nothing secret is
# printed.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ENV="$ROOT/.env"
[ -f "$ENV" ] || cp "$ROOT/.env.example" "$ENV"
chmod 600 "$ENV"

hex() { openssl rand -hex "$1"; }
b64() { openssl rand -base64 "$1"; }

fill() {
  local name="$1" value="$2"
  if grep -q "^${name}=$" "$ENV"; then
    # The value never reaches a command line: awk reads it from a file.
    local tmp
    tmp="$(mktemp)"
    printf '%s' "$value" >"$tmp.v"
    awk -v name="$name" -v file="$tmp.v" 'BEGIN { getline v < file } $0 == name "=" { print name "=" v; next } { print }' "$ENV" >"$tmp"
    mv "$tmp" "$ENV"
    rm -f "$tmp.v"
    chmod 600 "$ENV"
    echo "init-env: $name generated"
  elif ! grep -q "^${name}=" "$ENV"; then
    printf '%s=%s\n' "$name" "$value" >>"$ENV"
    echo "init-env: $name added"
  fi
}

fill POSTGRES_PASSWORD "$(hex 24)"
fill BENCH_DB_PASSWORD "$(hex 24)"
fill PID_DB_PASSWORD "$(hex 24)"
fill PID_KEK "$(b64 32)"
fill PID_TOKEN_PEPPER "$(hex 32)"
fill BENCH_ID_UI_SECRET "$(hex 32)"
fill BENCH_ID_SERVICE_SECRET "$(hex 32)"
fill BENCH_UI_SESSION_SECRET "$(hex 32)"
fill BENCH_KEK "$(b64 32)"
fill ID_ADMIN_PASSWORD "$(hex 12)"
fill BENCH_AGENT_TOKEN "$(hex 32)"
fill BENCH_LITE_KEY "pgw_bench_$(hex 20)"
echo "init-env: .env is ready; the first admin is ID_ADMIN_EMAIL with ID_ADMIN_PASSWORD"

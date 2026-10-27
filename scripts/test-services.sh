#!/usr/bin/env bash
# Starts the PostgreSQL the API's tests use, on a port that does not collide with other stacks
# on a development machine (25436; pacod-gw's tests use 25433, pacod-id's 25434). Safe to run
# again: a running container is left alone. The password is a test value and the port is bound
# to 127.0.0.1. Each test makes a database of its own on it.
set -euo pipefail

name=bench-test-pg
if ! docker ps --format '{{.Names}}' | grep -qx "$name"; then
  if docker ps -a --format '{{.Names}}' | grep -qx "$name"; then
    docker start "$name" >/dev/null
  else
    docker run -d --name "$name" --shm-size=256m -e POSTGRES_PASSWORD=test -p 127.0.0.1:25436:5432 \
      postgres:18.6-alpine3.24 >/dev/null
  fi
fi

for _ in $(seq 1 60); do
  if docker exec "$name" pg_isready -q -h 127.0.0.1 -U postgres 2>/dev/null; then
    echo "test services are ready: postgres on 25436 (BENCH_TEST_DATABASE_URL)"
    exit 0
  fi
  sleep 1
done
echo "the test services did not become ready within 60 s" >&2
exit 1

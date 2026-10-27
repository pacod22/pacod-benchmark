.PHONY: help network up down dev-up dev-down logs config images build test test-services verify fmt \
	lint deny ui-check vendor-gw vendor-gw-worktree vendor-id vendor-check binaries plan

# Tag of the images built by "make images"; the same value as BENCH_VERSION in .env.
VERSION ?= 0.1.0
# The pacod-gw checkout the agent, mock and minimal gateway images are built from.
PACOD_GW_SRC ?= ../pacod-gw

# The benchmark's own stack: the app file (its services) and the standalone file (its copies of
# the shared components, its load agent and mock), on the shared networks
# (pacod-demo/docs/NETWORK.md). The full stack loads the app file alone.
COMPOSE := docker compose -f docker-compose.yml -f docker-compose.standalone.yml
COMPOSE_DEV := $(COMPOSE) -f docker-compose.dev.yml

help:
	@echo "make dev-up        the stack from the working tree (http://localhost:18380), PROFILE=dev-gateway adds pacod-gw-lite"
	@echo "make up            the stack from the images of 'make images', with .env (scripts/init-env.sh)"
	@echo "make down | dev-down | logs S=bench-api | config"
	@echo "make images        pacod-bench, pacod-bench-ui, pacod-id, and pacod-gw's agent, mock and lite"
	@echo "make test-services PostgreSQL for the API's tests (25436)"
	@echo "make verify        fmt, clippy -D warnings, every test, cargo deny, the UI's checks, vendor-check"
	@echo "make vendor-gw     vendor pacod-gw at its HEAD; vendor-gw-worktree while changing both"
	@echo "make vendor-id     vendor pacod-id at its HEAD"
	@echo "make binaries      native binaries of the command line into dist/"
	@echo "make plan SUITE=examples/suites/modes.yaml   the cells a suite makes"

network:
	@docker network inspect pacod-net >/dev/null 2>&1 || docker network create pacod-net >/dev/null
	@docker network inspect pacod-admin-net >/dev/null 2>&1 || docker network create --internal pacod-admin-net >/dev/null
	@test "$$(docker network inspect -f '{{.Internal}}' pacod-admin-net)" = true || \
		{ echo "pacod-admin-net is not internal: remove it (docker network rm pacod-admin-net) and run make network" >&2; exit 1; }

up: network
	@test -f .env || { echo ".env is missing: scripts/init-env.sh writes one" >&2; exit 1; }
	$(COMPOSE) $(if $(PROFILE),--profile $(PROFILE)) up -d --wait

down:
	$(COMPOSE) --profile dev-gateway down

dev-up: network
	$(COMPOSE_DEV) $(if $(PROFILE),--profile $(PROFILE)) up -d --build --wait

dev-down:
	$(COMPOSE_DEV) --profile dev-gateway down

logs:
	$(COMPOSE) logs -f $(S)

# Checks the compose files without printing them: the output would hold .env's secrets.
config:
	$(COMPOSE) config --quiet
	$(COMPOSE_DEV) --profile dev-gateway config --quiet

images:
	docker build -f deploy/docker/rust.Dockerfile -t pacod-bench:$(VERSION) .
	docker build -f deploy/docker/ui.Dockerfile -t pacod-bench-ui:$(VERSION) .
	docker build -f vendor/pacod-id/deploy/docker/pacod-id.Dockerfile --target pacod-id -t pacod-bench-id:dev vendor/pacod-id
	docker build -f $(PACOD_GW_SRC)/deploy/docker/rust.Dockerfile --target load-agent -t pacod-gw-load-agent:dev $(PACOD_GW_SRC)
	docker build -f $(PACOD_GW_SRC)/deploy/docker/rust.Dockerfile --target mock -t pacod-gw-mock:dev $(PACOD_GW_SRC)
	docker build -f $(PACOD_GW_SRC)/deploy/docker/rust.Dockerfile --target lite -t pacod-gw-lite:dev $(PACOD_GW_SRC)

build:
	cargo build --workspace
	cd ui && npm run build

# PostgreSQL for the API's tests, in a container on port 25436.
test-services:
	scripts/test-services.sh

test: test-services
	cargo test --workspace
	cd ui && npm test

# Not --all: that also formats the path dependencies, which are the vendored copies.
fmt:
	cargo fmt
	cd ui && npm run format

lint:
	cargo fmt --check
	cargo clippy --workspace --all-targets -- -D warnings

deny:
	cargo deny --locked check

ui-check:
	cd ui && npm run check && npm run lint && npm test && npm run build

# Every check before a commit.
verify: lint test deny ui-check vendor-check config

vendor-gw:
	scripts/vendor-pacod-gw.sh

vendor-gw-worktree:
	scripts/vendor-pacod-gw.sh --worktree

vendor-id:
	scripts/vendor-pacod-id.sh

# The vendored pacod-gw is a commit's, not a working tree's, and every client of its contracts
# reads their fixtures (crates/pacod-bench-engine/tests).
vendor-check:
	@grep -q '^commit ' vendor/pacod-gw/VERSION || \
		{ echo "vendor/pacod-gw is a working tree's copy ($$(head -1 vendor/pacod-gw/VERSION)): vendor again from a commit (make vendor-gw)" >&2; exit 1; }
	@grep -q '^commit ' vendor/pacod-id/VERSION || { echo "vendor/pacod-id has no commit" >&2; exit 1; }
	cargo test -q -p pacod-bench-engine --test load_contract --test report_contract

binaries:
	scripts/build-binaries.sh

SUITE ?= examples/suites/modes.yaml
plan:
	cargo run -q -p pacod-bench -- plan --suite $(SUITE)

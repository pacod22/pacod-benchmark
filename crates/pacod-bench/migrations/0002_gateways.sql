-- Gateways the benchmark manages, added through the UI. Unlike those of the configuration, they
-- keep their platform-admin token here: sealed under the key-encryption key BENCH_KEK, which
-- lives outside the database (crates/pacod-bench/src/secrets.rs), and bound to the target's id
-- and management API, so that a token copied onto another row, or kept while the address
-- changes, does not open. The column is read on its own, never with the target, and never
-- leaves the API.
ALTER TABLE targets ADD COLUMN sealed_token jsonb;
-- A label of the token (its last characters and the start of its SHA-256), for the UI.
ALTER TABLE targets ADD COLUMN token_hint text NOT NULL DEFAULT '';
-- Who the gateway said the token was at its last check (GET /api/v1/session there), and when.
ALTER TABLE targets ADD COLUMN token_checked jsonb;

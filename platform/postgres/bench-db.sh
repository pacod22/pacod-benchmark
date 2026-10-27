#!/bin/sh
# pacod-benchmark's role and database on the shared PostgreSQL (pacod-postgres): a role "bench"
# that is not a superuser and owns the database "bench". The API creates its tables there
# (migrations run at start); none of them needs a superuser.
#
# pacod-db-init runs this at every start, before the API, as a superuser through libpq's PG*
# variables (PGHOST, PGUSER, PGPASSWORD). Safe to run again: it creates what is missing and sets
# the role's password to BENCH_DB_PASSWORD. The password is read with psql's \getenv, so it is on
# no command line, and nothing secret is printed. The same file serves the benchmark's own stack
# (docker-compose.standalone.yml) and the full stack (pacod-demo).

set -eu

: "${PGHOST:?the PostgreSQL host, with PGUSER and PGPASSWORD of a superuser}"
: "${BENCH_DB_PASSWORD:?the password for the role bench}"

psql -v ON_ERROR_STOP=1 -q -d postgres <<'SQL'
\getenv pw BENCH_DB_PASSWORD
SELECT format('CREATE ROLE bench LOGIN PASSWORD %L', :'pw')
 WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'bench') \gexec
SELECT format('ALTER ROLE bench LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE PASSWORD %L', :'pw') \gexec
SELECT 'CREATE DATABASE bench OWNER bench'
 WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = 'bench') \gexec
REVOKE ALL ON DATABASE bench FROM PUBLIC;
GRANT CONNECT, CREATE, TEMPORARY ON DATABASE bench TO bench;
SQL

echo "bench-db: the role and the database bench are in place"

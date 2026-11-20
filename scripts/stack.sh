#!/usr/bin/env bash
# The local database: `up` starts PostgreSQL (compose project yieldagent, 127.0.0.1 on
# YIELDAGENT_PG_PORT, default 63032) and waits until it is healthy; `down` stops it; `reset`
# also deletes its volume; `env` prints what the tests and drills read:
#
#   YIELDAGENT_TEST_DATABASE_URL   the administrator (creates roles and throwaway databases)
#   YIELDAGENT_PG_CONTAINER        where the drills run pg_dump and pg_restore (so they need not be installed)
#
# With YIELDAGENT_TEST_DATABASE_URL already set (another PostgreSQL 18 you administer), `up`
# starts nothing and `env` repeats it.
#
# Every password here is a public development value.
set -euo pipefail
case "${1:-}" in -h|--help) sed -n '2,/^set -euo/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'; exit 0 ;; esac
cd "$(dirname "$0")/.."

compose=(docker compose -f deploy/compose.yaml)
port="${YIELDAGENT_PG_PORT:-63032}"

case "${1:-}" in
  up)
    if [ -n "${YIELDAGENT_TEST_DATABASE_URL:-}" ]; then
      echo "stack: YIELDAGENT_TEST_DATABASE_URL is set; using that database, starting none"
      exit 0
    fi
    command -v docker >/dev/null || { echo "stack: docker is needed (or set YIELDAGENT_TEST_DATABASE_URL to an administrator's URL)" >&2; exit 1; }
    "${compose[@]}" up -d --wait postgres >/dev/null
    echo "stack ok: postgres on 127.0.0.1:${port}"
    ;;
  down) "${compose[@]}" down ;;
  reset) "${compose[@]}" down --volumes ;;
  env)
    if [ -n "${YIELDAGENT_TEST_DATABASE_URL:-}" ]; then
      echo "export YIELDAGENT_TEST_DATABASE_URL=$YIELDAGENT_TEST_DATABASE_URL"
      echo "export YIELDAGENT_PG_CONTAINER=${YIELDAGENT_PG_CONTAINER:-}"
    else
      echo "export YIELDAGENT_TEST_DATABASE_URL=postgres://yieldagent:yieldagent-admin-dev@127.0.0.1:${port}/yieldagent"
      echo "export YIELDAGENT_PG_CONTAINER=$("${compose[@]}" ps -q postgres)"
    fi
    ;;
  *) echo "stack: unknown command '${1:-}'. Commands: up, down, reset, env (see --help)" >&2; exit 2 ;;
esac

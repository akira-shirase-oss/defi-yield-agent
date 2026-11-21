#!/usr/bin/env bash
# Every limit is proven by a test that fails when the limit is removed.
#
# Each bound in the source carries a marker, `// LIMIT(name)`, on the line that sets it (or
# on a comment line just above it). For every marker this script removes the limit in a
# scratch copy — one substitution on that line — and runs the one test that proves it,
# which must then FAIL as a test (a build error is not a proof). A marker without a
# mutation below, or a mutation whose marker is gone, fails the script too, so a new limit
# cannot be added without its proof.
#
#   scripts/prove-limits.sh [--out FILE]   (FILE: a JSON record; never names the machine)
#
# Needs YIELDAGENT_TEST_DATABASE_URL (scripts/stack.sh env) for the database-backed tests.
# Builds into target/prove (this copy's own target directory); the scratch source is
# deleted at the end.
set -euo pipefail
case "${1:-}" in -h|--help) sed -n '2,/^set -euo/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'; exit 0 ;; esac
cd "$(dirname "$0")/.."
here="$PWD"
out=""
case "${1:-}" in
  "") ;;
  --out) out="${2:?--out needs a file}" ;;
  *) echo "prove-limits.sh: unknown argument $1 (see --help)" >&2; exit 2 ;;
esac
: "${YIELDAGENT_TEST_DATABASE_URL:?is not set. Start the database and export its address: scripts/stack.sh up; eval \"\$(scripts/stack.sh env)\"}"

work="$(mktemp -d "${TMPDIR:-/tmp}/yieldagent-prove.XXXXXX")"
trap 'rm -rf "$work"' EXIT
tar --exclude ./target --exclude ./.git -cf - . | tar -xf - -C "$work"
export CARGO_TARGET_DIR="$here/target/prove"

# name|file|pattern=>replacement (a Python regex, applied to the marked line)|package|test target|test
mutations=(
  "connection-cap|crates/yieldagent-http/src/serve.rs|limits\.max_connections=>Semaphore::MAX_PERMITS|yieldagent-http|limits|connections_past_the_cap_wait_in_the_backlog_until_a_slot_frees"
  "header-read|crates/yieldagent-http/src/serve.rs|^.*$=>|yieldagent-http|limits|a_trickled_header_is_cut_off_and_never_answered"
  "requests-per-connection|crates/yieldagent-http/src/serve.rs|nth >= cap=>false|yieldagent-http|limits|pipelined_requests_past_the_cap_close_the_connection"
  "write-stall|crates/yieldagent-http/src/serve.rs|limits\.write_stall=>Duration::from_secs(3600)|yieldagent-http|limits|non_reading_clients_cannot_hold_connection_slots"
  "lifetime|crates/yieldagent-http/src/serve.rs|limits\.max_age=>Duration::from_secs(3600)|yieldagent-http|limits|a_busy_connection_is_closed_after_its_lifetime"
  "drain|crates/yieldagent-http/src/serve.rs|limits\.drain_timeout=>Duration::from_secs(3600)|yieldagent-http|limits|shutdown_lets_a_request_finish_and_cuts_at_its_deadline"
  "in-flight|crates/yieldagent-http/src/stack.rs|max_in_flight\.max\(1\)=>Semaphore::MAX_PERMITS|yieldagent-http|limits|requests_past_the_in_flight_gate_are_shed_at_once"
  "request-timeout|crates/yieldagent-http/src/stack.rs|limits\.request_timeout=>Duration::from_secs(3600)|yieldagent-http|limits|a_stalled_body_is_answered_by_the_request_deadline"
  "body|crates/yieldagent-http/src/stack.rs|limits\.max_body_bytes=>usize::MAX|yieldagent-http|limits|an_oversized_body_is_refused_with_the_envelope"
  "per-key-table|crates/yieldagent-http/src/budget.rs|^.*$=>|yieldagent-http|lib|budget::tests::the_table_never_grows_past_its_capacity_and_keeps_the_throttled"
  "sign-in-per-address|crates/yieldagent-accounts/src/signin.rs|self\.per_address\.take\(address\)\?;=>let _ = self.per_address.take(address);|yieldagent-accounts|budgets|one_address_flood_bounds_only_that_address"
  "sign-in-global|crates/yieldagent-accounts/src/signin.rs|self\.global\.take\(\)=>Ok::<(), RetryAfter>(())|yieldagent-accounts|budgets|a_distributed_flood_cannot_lock_out_a_trusted_owner"
  "mail-per-address|crates/yieldagent-accounts/src/signin.rs|self\.per_address\.take\(address\)\?;=>let _ = self.per_address.take(address);|yieldagent-accounts|budgets|mail_a_client_can_cause_is_bounded"
  "password-concurrency|crates/yieldagent-accounts/src/password.rs|concurrency\.max\(1\)=>Semaphore::MAX_PERMITS|yieldagent-accounts|lib|password::tests::at_most_the_concurrency_hashes_at_once"
  "plan|crates/yieldagent-accounts/src/domain.rs|current < i64::from\(limit\)=>true|yieldagent-accounts|limits|token_and_workspace_limits_hold_under_concurrency"
  "ready-deadline|crates/yieldagent-server/src/health.rs|self\.deadline=>Duration::from_secs(3600)|yieldagent-server|health|readyz_reports_a_silent_database_within_its_deadline"
  "https-only|crates/yieldagent-data/src/live.rs|return Err\(UrlError::InsecureScheme\);=>{}|yieldagent-data|lib|live::tests::urls_are_https_or_loopback_http_and_plain"
  "source-body|crates/yieldagent-data/src/live.rs|self\.max_body\)=>usize::MAX)|yieldagent-data|live_source|a_body_past_the_cap_is_refused_declared_or_streamed"
  "fetch-deadline|crates/yieldagent-data/src/live.rs|self\.deadline=>std::time::Duration::from_secs(3600)|yieldagent-data|live_source|a_silent_source_times_out_at_the_deadline"
  "entry-count|crates/yieldagent-data/src/defillama.rs|= 100_000;=>= 1_000_000_000;|yieldagent-data|defillama|the_entry_count_is_capped"
  "market-count|crates/yieldagent-data/src/defillama.rs|valid\.len\(\) >= MAX_MARKETS=>false|yieldagent-data|defillama|a_body_past_the_market_cap_is_refused_whole"
  "pool-allowlist|crates/yieldagent-data/src/market.rs|\*id == pool_id && =>|yieldagent-data|lib|market::tests::only_a_pinned_pool_of_exactly_usdc_is_kept"
  "underlying-usdc|crates/yieldagent-data/src/market.rs|t\.eq_ignore_ascii_case\(USDC_BASE\)=>!t.is_empty()|yieldagent-data|defillama|a_pinned_pool_is_kept_only_while_its_underlying_token_is_exactly_usdc"
  "min-history|crates/yieldagent-data/src/screen.rs|= 3;=>= 0;|yieldagent-data|lib|screen::tests::the_average_is_of_the_last_twelve_rounds_and_needs_three"
  "spike-window|crates/yieldagent-data/src/screen.rs|= 12;=>= 1_000;|yieldagent-data|lib|screen::tests::the_average_is_of_the_last_twelve_rounds_and_needs_three"
  "spike-baseline|crates/yieldagent-markets/src/board.rs|s\.flags\.is_empty\(\)=>true|yieldagent-markets|worker|a_sustained_spike_stays_flagged_and_never_becomes_its_own_baseline"
  "spike-baseline-sql|crates/yieldagent-markets/src/store.rs| AND cardinality\(flags\) = 0=>|yieldagent-markets|worker|a_sustained_spike_stays_flagged_and_never_becomes_its_own_baseline"
  "recent-eviction|crates/yieldagent-markets/src/board.rs|snapshots\.iter\(\)\.any\(.s. s\.market\.pool_id == \*id\)=>!id.is_empty()|yieldagent-markets|worker|a_pool_gone_from_the_last_good_round_leaves_the_recent_history"
  "snapshot-age-cap|crates/yieldagent-server/src/config.rs|60\.\.=900=>60..=86_400|yieldagent-server|lib|config::tests::the_snapshot_age_cannot_be_set_past_case_5s_fifteen_minutes"
  "duplicate-pool|crates/yieldagent-data/src/defillama.rs|^.*$=>parsed.markets.push(m);|yieldagent-data|defillama|every_hostile_entry_is_refused_by_name_and_only_the_good_one_kept"
  "number-length|crates/yieldagent-data/src/decimal.rs|= 48;=>= 4096;|yieldagent-data|lib|decimal::tests::what_cannot_be_held_exactly_is_refused_not_rounded"
  "id-length|crates/yieldagent-data/src/market.rs|<= MAX_ID_LEN=><= usize::MAX|yieldagent-data|lib|market::tests::the_boundaries_are_inclusive"
  "pool-id-charset|crates/yieldagent-data/src/market.rs|is_ascii_alphanumeric\(\)=>is_ascii()|yieldagent-data|lib|market::tests::every_field_has_its_named_refusal"
  "symbol-charset|crates/yieldagent-data/src/market.rs|is_ascii_alphanumeric\(\)=>is_ascii()|yieldagent-data|lib|market::tests::every_field_has_its_named_refusal"
  "non-negative|crates/yieldagent-data/src/market.rs|return Err\(negative\);=>{}|yieldagent-data|lib|market::tests::every_field_has_its_named_refusal"
  "plausible-ceiling|crates/yieldagent-data/src/market.rs|return Err\(implausible\);=>{}|yieldagent-data|lib|market::tests::every_field_has_its_named_refusal"
  "apy-spike|crates/yieldagent-data/src/screen.rs|apy \* n > SPIKE_MULTIPLIER \* sum=>false|yieldagent-markets|worker|an_apy_spike_over_the_moving_average_is_flagged_and_stored_flagged"
  "staleness|crates/yieldagent-data/src/screen.rs|Freshness::Stale=>Freshness::Fresh|yieldagent-markets|routes|an_old_snapshot_is_served_stale_and_its_source_stale_never_refreshed"
  "history-limit|crates/yieldagent-markets/src/routes.rs|\(1\.\.=MAX_HISTORY\)\.contains\(n\)=>true|yieldagent-markets|routes|history_is_newest_first_bounded_and_refuses_bad_queries_by_name"
  "html-escape|crates/yieldagent-markets/src/routes.rs|out\.push_str\(\"&lt;\"\)=>out.push('<')|yieldagent-markets|routes|the_html_list_is_plain_and_escapes_everything_it_shows"
  "idempotent-round|crates/yieldagent-markets/src/store.rs|ON CONFLICT \(round_id\) DO NOTHING=>|yieldagent-markets|worker|a_round_is_stored_whole_once_and_a_retry_changes_nothing"
  "worker-fetch-deadline|crates/yieldagent-markets/src/worker.rs|settings\.fetch_deadline=>Duration::from_secs(3600)|yieldagent-markets|worker|a_hanging_source_is_cut_at_the_worker_deadline_and_stored_down"
  "no-markets-is-down|crates/yieldagent-markets/src/worker.rs|return round;=>{}|yieldagent-markets|worker|a_down_source_stores_a_down_round_and_no_snapshot_and_keeps_the_last_good"
  "write-attempts|crates/yieldagent-markets/src/worker.rs|1\.\.=attempts=>1..|yieldagent-markets|worker|an_unreachable_database_ends_the_round_after_its_bounded_attempts"
  "query-length|crates/yieldagent-markets/src/routes.rs|> 256=>> usize::MAX|yieldagent-markets|routes|history_is_newest_first_bounded_and_refuses_bad_queries_by_name"
  "path-pool-id|crates/yieldagent-markets/src/routes.rs|^.*$=>    true|yieldagent-markets|routes|history_is_newest_first_bounded_and_refuses_bad_queries_by_name"
)

# Every marker in the source has exactly one mutation, and every mutation a marker.
found="$(grep -rhoE 'LIMIT\([a-z-]+\)' crates/*/src | sed 's/LIMIT(\(.*\))/\1/' | sort -u)"
defined="$(printf '%s\n' "${mutations[@]}" | cut -d'|' -f1 | sort -u)"
if [ "$found" != "$defined" ]; then
  echo "error: markers and mutations differ:" >&2
  diff <(echo "$found") <(echo "$defined") >&2 || true
  exit 1
fi

mutate() { # file marker pattern replacement
  python3 -I - "$work/$1" "$2" "$3" "$4" <<'PY'
import re, sys
path, marker, pattern, replacement = sys.argv[1:]
lines = open(path).read().split("\n")
tag = f"LIMIT({marker})"
changed = 0
for i, line in enumerate(lines):
    if tag not in line:
        continue
    target = i if line.split("//")[0].strip() else i + 1
    new = re.sub(pattern, replacement, lines[target], count=1)
    if new != lines[target]:
        lines[target] = new
        changed += 1
if not changed:
    sys.exit(f"the mutation for {marker} changed nothing")
open(path, "w").write("\n".join(lines))
PY
}

failures=0
results=()
for row in "${mutations[@]}"; do
  IFS='|' read -r name file subst package target test <<<"$row"
  pattern="${subst%%=>*}"
  replacement="${subst#*=>}"
  cp "$here/$file" "$work/$file"
  mutate "$file" "$name" "$pattern" "$replacement"
  case "$target" in
    lib) selector=(--lib) ;;
    *) selector=(--test "$target") ;;
  esac
  started=$(date +%s)
  set +e
  output="$(cd "$work" && cargo test --locked -q -p "$package" "${selector[@]}" -- --exact "$test" 2>&1)"
  status=$?
  set -e
  seconds=$(( $(date +%s) - started ))
  cp "$here/$file" "$work/$file"
  if [ "$status" -ne 0 ] && grep -q 'test result: FAILED' <<<"$output"; then
    verdict=proven
    printf '  proven   %-24s %s fails without it (%ss)\n' "$name" "$test" "$seconds"
  elif [ "$status" -ne 0 ]; then
    verdict=broken
    failures=$((failures + 1))
    printf '  BROKEN   %-24s the mutation did not build or the test did not run:\n%s\n' "$name" "$(tail -20 <<<"$output")"
  else
    verdict=unproven
    failures=$((failures + 1))
    printf '  UNPROVEN %-24s %s passes without the limit\n' "$name" "$test"
  fi
  results+=("{\"limit\":\"$name\",\"test\":\"$test\",\"verdict\":\"$verdict\",\"seconds\":$seconds}")
done

if [ -n "$out" ]; then
  commit="$(git rev-parse HEAD 2>/dev/null || echo unknown)"
  dirty=false; git diff --quiet HEAD -- . 2>/dev/null || dirty=true
  load="$(uptime | sed 's/.*load averages*: *//')"
  printf '{"proof":"limits","commit":"%s","tree_dirty":%s,"load_average":"%s","os":"%s","results":[%s]}\n' \
    "$commit" "$dirty" "$load" "$(uname -sr)" "$(IFS=,; echo "${results[*]}")" >"$out"
  echo "record: $out"
fi
if (( failures )); then
  echo "error: $failures of ${#mutations[@]} limits are not proven" >&2
  exit 1
fi
echo "limits ok: all ${#mutations[@]} limits are proven by a test that fails without them"

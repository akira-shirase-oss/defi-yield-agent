# Decisions

Trade-offs and decisions, D-001 onward. Each entry says what was chosen and why; a
superseded decision is marked, never rewritten.

## D-001 — Base: the Backend Platform starter template

The repository begins as a copy of the Backend Platform's copy-and-own starter template —
the bounded HTTP layer, the three-role PostgreSQL split with its protections manifest,
the accounts kit, releases, restore drills, and the CI + evidence kit — renamed for this
product. The commit it came from is in `TEMPLATE_ORIGIN`; later template changes arrive
only as reviewed diffs. Nothing shares a package, service, login or database with the
template's repository or any sibling product.

## D-002 — Port band 63000-63099

Every port this repository binds on a shared development machine stays inside
63000–63099: the server on 63050, the development PostgreSQL on 63032, the template's
other defaults shifted into the band. `scripts/check-ports.sh` fails on a literal outside
the band, and the gate proves it can fail. Rationale: Linux's ephemeral range ends at
60999, so CI and a second checkout never collide.

## D-003 — Stack

Rust backend on Axum, SQLx and tokio (the template's stack); SvelteKit + TypeScript
(strict) for the web UI once the maintainer chooses this product's design system — no
`web/` product code before that. Solidity + Foundry only for on-chain code, in
`contracts/`. Everything else follows `docs/END_GOAL.md`.

## D-004 — M1 market data: one named public source, exact decimals, named refusals

The worker reads DefiLlama's keyless yields endpoint (`/pools`) through the `PoolSource`
port (`crates/yieldagent-data`); tests use the recorded adapter and a 50-entry recorded
body, and the live test runs only with `YIELDAGENT_LIVE_NETWORK=1`. Only Base, USDC,
and the END_GOAL allowlist (`aave-v3`, `compound-v3`, `morpho-blue`) are kept. APY and TVL
are parsed from the raw JSON number token into `rust_decimal` — never through a float —
and a value it cannot hold exactly is refused, not rounded. Each refused entry is counted
under a named code (`apy_implausible`, `pool_id_invalid`, `duplicate_pool`, …); the
ceilings (APY ≤ 100 %, TVL ≤ 10^12 USD) are chosen, not measured: the live body held APYs
above 400,000 %.

## D-005 — Snapshots: one append-only round per fetch, idempotent by round id

Each fetch is a round with a UUIDv7 id minted before the fetch; the round and its
snapshots are written in one transaction (`ON CONFLICT (round_id) DO NOTHING`), retried
with the same id, so a retry after a lost acknowledgement stores nothing twice. A failed
fetch or an unusable body stores a `down` round with its error code and no snapshot: the
board keeps the last good round and shows the source down; staleness (Case 5, 15 min) is
judged per read. Both tables are append-only by trigger and grant. Case 5's chain
cross-check is a port with a stub (`NoChainReads`) that answers `unavailable`, so no
market is `plannable` in M1.

## D-006 — The M1 read API

`GET /v1/markets` (the board, from memory), `GET /v1/markets/{pool_id}/history`
(`limit` 1..=500, `before` unix seconds; anything else is refused by name) and `GET
/markets`, a plain unstyled HTML list (`Content-Security-Policy: default-src 'none'`) —
the dashboard's design stays the maintainer's. Decimals are served as strings.

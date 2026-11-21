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
above 400,000 %. *Selection by project and symbol suffix is superseded by D-007's pinned
pool allowlist.*

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

## D-007 — M1 review fixes: pinned pools, a clean baseline, bounded markets; what waits for M2

**Pinned pools.** A symbol is not identity (anyone can name a Morpho vault `...USDC`, and
DefiLlama lists syrupUSDC and a Compound market that takes USDC as collateral under
USDC-looking symbols). `market::ALLOWLIST` pins six Base pools by DefiLlama pool id and
protocol: Aave v3 USDC, Compound III USDC (the USDC base market, not the `WETH-pool`
entry), and four curated Morpho vaults (two Steakhouse USDC, Spark USDC, Grove x
Steakhouse USDC; each over $100M TVL in the recorded body). Each must also carry
`underlyingTokens == [USDC]` (`0x8335…2913`, compared without case). Anything else that
looks like a candidate (Base, an allowlisted project, a `USDC` symbol) is counted under
`pool_not_allowlisted` or `underlying_not_usdc`; adding a pool is a reviewed code change.

**Base APY.** `apyBase` and `apyReward` are stored apart from `apy`; the spike check, and
any plan later, read `apy_base` only. A reward is shown, never planned on.

**A clean baseline.** A flagged APY never joins the moving average, in memory
(`Board::apply`) or in the history a restart rebuilds it from (`store::recent_apys`
filters `cardinality(flags) = 0`), so a sustained wrong value stays flagged and a ramp
under 3x per round is caught from its second step. The window is 12 rounds and needs 3;
with `YIELDAGENT_FETCH_INTERVAL_S` 10..=3600 that spans 2 min to 12 h. Kept in rounds for
M1 (both constants have a mutation row); a time-based window is an M2 question.

**Bounded markets.** `MAX_MARKETS = 64` valid entries per body; past it the whole body is
refused `too_many_markets` (a `down` round), so a flood stores nothing. The board keeps
recent APYs only for pools in the latest good round of the source.

**Freshness bound.** `YIELDAGENT_SNAPSHOT_MAX_AGE_S` is 60..=900: Case 5's 15 minutes is
a ceiling, not a default.

**Startup.** `store::latest` reads the latest round and the latest good round per source
with `ORDER BY observed_at DESC LIMIT 1` over a partial index, not `DISTINCT ON` over all
rounds.

**Worker liveness.** The worker runs in a `TaskTracker`; its exit is logged (error when
not shutting down), and `/readyz` answers 503 `market_worker: stopped` once it has
returned or panicked.

**The two networks.** Market data is Base mainnet (chain 8453): it is the only network
with real lending markets and real APYs, and M1 only reads. Contracts and the scripted
rebalance run on Base Sepolia (END_GOAL M5); Sepolia markets (Aave's own test USDC, no
Compound III) are not a source of rates. To be confirmed by the coordinator.

**Deferred to M2, each with its reason.**
- *Source time.* `/pools` carries no per-entry timestamp, and the CDN's `Date` is the
  response time, not the data's. Freshness stays our fetch time until the chain reads
  (block timestamps) land with the Case 5 cross-check.
- *Retention.* Both tables are append-only by trigger and grant; pruning needs a
  separate role and a reviewed job. Startup no longer scans all history; the restart
  history reads one day.
- *A lost commit acknowledgement.* When every write attempt times out after a commit
  landed, the board misses that round; the next stored round replaces the source's
  entry, and a restart rebuilds from the store. Re-reading the round by id after the last
  failed attempt comes with M2's executor-facing consistency work.
- *A live `/markets` run.* The live adapter test runs with `YIELDAGENT_LIVE_NETWORK=1`;
  a recorded run of the HTML page against the live URL is M2 evidence.

**Architecture rule 4** now bans crate families (`alloy-*`, `ethers-*`, `*secp256k1*`,
…) as well as names, and any source line naming a sending or signing RPC method or
`ring`'s signing API. `ring` itself stays: it is rustls's TLS provider.

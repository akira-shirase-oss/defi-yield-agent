# DeFi Yield Agent — agent instructions

A non-custodial yield assistant for stablecoins on Base: a stolen agent key can only make
small, capped moves between allowlisted lending markets, back into the user's own
account — and the owner can always leave without the agent or the server.

Contract: `docs/END_GOAL.md` (what done means, scope, the twelve required failure cases
and their drills, the ship gate); the verified facts it rests on are in
`docs/KNOWLEDGE.md`. Status: **base setup only — nothing of the product is built yet**;
the running code is the renamed Backend Platform starter template.

## Read first

1. `docs/END_GOAL.md` — the contract: scope, failure cases, ship gate.
2. `docs/KNOWLEDGE.md` — verified facts and sources; re-check versions at M0.
3. `docs/DECISIONS.md` — the decisions so far (base, port band, stack).
4. `docs/ROADMAP.md` — M0 onward; a status cell says *Done* only with named tests and
   evidence.

## Non-negotiables

1. **Performance and reliability come first.** Bounded queues and bodies, deadlines on
   every I/O await, idempotent writes, crash-safe state, no blocking on async workers.
2. **Evidence over claims.** Load runs are at least 1,000,000 units and run on CI-hosted
   runners, not a laptop; results carry machine, commit and load; misses are kept, never
   re-run until green.
3. **Every feature ships** regression tests + a fault-fixture path + a load scenario in
   the same change; every fixed bug gets a test that fails without it.
4. **The port band is 63000-63099.** Server 63050, development PostgreSQL 63032;
   `scripts/check-ports.sh` fails on a literal outside the band.
5. **The LLM proposes; contracts enforce.** No model output reaches calldata except
   through the deterministic plan, and no model tool signs or sends.
6. **No secrets or payloads in logs.** Typed errors (`thiserror`) in libraries, `anyhow`
   at binary boundaries; no `unwrap`/`expect` on external input.

## Verification

`scripts/check.sh` runs what CI runs — run it before every push. Local stack:
`scripts/stack.sh up` (PostgreSQL on 127.0.0.1:63032); the server binds
127.0.0.1:63050. Its credentials are public development values.

## The web UI

The design system is the maintainer's choice and has not been made. `web/` holds only a
stub: do not start a SvelteKit app until it is decided.

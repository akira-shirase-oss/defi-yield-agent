# DeFi Yield Agent

A non-custodial yield assistant for stablecoins on Base: an agent that moves between
allowlisted lending markets under on-chain caps a stolen key cannot exceed.

> **Status: base setup only.** Nothing of the product is built yet; the code is the
> renamed Backend Platform starter template. The contract is `docs/END_GOAL.md`.

> **Public-demo constraint (not legal advice):** testnet tokens only, a hard cap in the
> contract, no mainnet deposits, fees, performance claims or promised yields; a visible
> "demo, not financial advice" notice; mainnet data shown read-only.

## Quick start (the template, local)

Docker, rustup (the pinned toolchain installs itself), curl, jq, python3. Everything
binds 127.0.0.1 inside this product's port band.

<!-- quickstart:begin -->
```sh
scripts/stack.sh up        # PostgreSQL 18 in Docker, published on 127.0.0.1:63032
scripts/dev.sh start       # build, create and migrate the dev database, run the server
curl -fsS http://127.0.0.1:63050/readyz
# {"database":"ok","status":"ready"}
invite=$(scripts/dev.sh invite)
scripts/dev.sh stop
```
<!-- quickstart:end -->

The accounts walk-through — sign-up, verification, a workspace, an API token — is
`docs/ACCOUNTS.md`; the operator guide is `docs/OPERATIONS.md`. `scripts/check.sh` is
the gate CI runs.

## Layout

- `crates/yieldagent-{http,db,accounts,server}` — the template's crates, renamed
- `deploy/`, `scripts/`, `docs/`, `perf/` — host layer, drills, operator docs, evidence kit
- `contracts/` — the Foundry project for the on-chain pieces (empty until M3)
- `web/` — empty until the design-system decision

Licence: MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`).

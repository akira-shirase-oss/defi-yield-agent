# DeFi Yield Agent — end goal

**A stolen agent key can only make small, capped moves between allowlisted lending markets, back
into the user's own account. The owner can always leave without the agent or the server.**

A non-custodial yield assistant for stablecoins on Base. The user owns a smart account. An off-chain
Rust worker reads supply rates and risk signals for a few lending markets and scores them
deterministically. It computes the best allocation under the user's policy and simulates the move.
It then proposes the move with a plain-language explanation that the model writes from the numbers it
was given. The user approves, and the worker submits an ERC-4337 UserOperation signed by a restricted
session key. Every limit — allowlisted adapters, single-move and daily caps, slippage, expiry, pause —
is enforced on-chain by the account's policy module, never by the prompt. The product is a testnet
demonstration of agent security with honest evidence, not a launch.

**Origin.** Programme spec 01, v2.0 (2026-11-20), merged with the repository rules in `AGENTS.md`.
Where the two conflict, the conflict is listed under open questions, not resolved here. The verified
facts behind this document are in `KNOWLEDGE.md`. Every number below is either the spec's or a proposed
target marked as such. None is a measurement.

## Users and the job to be done

- **Crypto-savvy saver:** "Keep my idle test USDC in the safest market above my threshold, and let me
  see why every move happened."
- **Small treasury:** policy-driven moves, an audit log, a multisig owner override.
- **Reviewer:** checks within five minutes that the security claims are real: threat model, invariant
  list, failing-attack tests, live testnet demo.

## v1 scope

- Market snapshots for Aave v3 and Morpho on Base Sepolia, plus Compound III, Aave and Morpho on a
  pinned Base mainnet fork. Chain reads go through alloy and Multicall3, cross-checked with DefiLlama.
- A pure `risk` crate with a factor breakdown. The spec's factors (audit status, TVL, utilisation, oracle
  type, age, APY volatility) plus curator and collateral exposure.
- A pure `strategy` crate: target allocation under the policy, skipped when the net gain after gas is
  below the user's threshold.
- The smart account (choice open, Q1), a `YieldPolicyModule` and adapters that hard-code the account as
  recipient, and an atomic rebalancer with a minimum-out check.
- Policy setup and session-key issuance from the UI, with a human-readable summary before signing.
- The agent as an explicit, persisted Rust state machine: Observe → Filter → Plan → Explain → Simulate →
  Propose → AwaitApproval → Execute → Done/Failed.
- Proposals with approval, live updates over SSE, and an audit log in Postgres mirrored by on-chain events.
- SIWE sign-in (`signinwithethereum`, ERC-1271 and ERC-6492 for contract accounts).
- A dashboard covering every operation: setup, deposit, proposals, approve or reject, history, realised
  vs. passive yield, pause, revoke, withdraw-all.
- A documented **direct exit** (block-explorer write calls and a `cast` script) that works when our
  servers are down.
- A 90-day backtest binary over recorded DefiLlama history.

## Non-goals (v1)

Mainnet deposits from anyone, including the maintainer; leverage, looping and liquidations; LP and
volatile-asset positions; swaps (Uniswap) and bridging; auto-execute mode (spec P1); multi-chain (P2);
tax export (P2); Telegram; fees or any business model; an LLM anywhere in the scoring, filtering or
signing path; an external audit (none is claimed).

## Architecture in brief

Rust workspace (`api`, `data`, `risk`, `strategy`, `agent`, `executor`, `chain`, `llm`, `db`) on Tokio,
Axum and SQLx over PostgreSQL. Solidity and Foundry only for `contracts/`. SvelteKit + TypeScript (strict)
+ Tailwind v4 + shadcn-svelte for the dashboard, with viem/@wagmi/core for wallet signing in the browser.
Per the standard layout, the Rust server serves the API and the static dashboard on one origin, on the
product's droplet under systemd behind Caddy, and the landing and docs sites go on Cloudflare Pages. The
spec's Pages-plus-API split is an open question (Q6). `risk` and `strategy` carry no I/O, and an
architecture check enforces it. The LLM crate has no route to a signer. Every queue, body, RPC call,
bundler call and model call has a bound and a deadline.

## Non-negotiable invariants

1. **The LLM proposes; contracts enforce.** No model output reaches calldata except through the
   deterministic plan. The model has no tool that signs or sends.
2. **The session key can call only the adapters** and the rebalancer. No transfer, approve, permit,
   delegatecall, module change or self-call. Recipients are fixed to the account.
3. **Caps hold per UTC day.** The sum of agent-initiated moves in a day is at most `dailyCap`. Each move
   is at most `singleMoveCap`. Expiry is enforced through `validationData`. Pause makes every agent
   operation revert (KNOWLEDGE §1 explains why validation cannot read the time).
4. **The owner's exit never depends on the agent, the module or the server.** It recovers everything the
   markets can release.
5. **Crash-safe and exactly-once.** Every transition is committed before its side effect. A UserOperation
   hash and nonce are recorded before submission. After a crash, a run resumes and never submits twice.
6. **Money is exact.** `U256` for chain amounts, decimals for APY. No floats on any path that produces
   calldata.
7. **Content is data.** Protocol text, token names and governance posts are untrusted, never instructions.
8. **Evidence over claims.** Results carry machine, commit and machine load. A miss is kept, not rerun
   until it passes.

## Required failure cases (contract, not options)

| # | Trigger | Required result | Proven by | Pass condition | From |
|---|---|---|---|---|---|
| 1 | **Compromised agent key** sends transfer, approve, permit, `supply(onBehalfOf=attacker)`, `withdraw(to=attacker)`, a batch hiding one bad call, delegatecall mode, `installModule`, an over-cap move, many small moves straddling midnight UTC, an expired key, a paused account | Every operation reverts | Foundry unit, fuzz and invariant suites `agent_key_cannot_exfiltrate`, each also on the Base fork | Zero token balance leaves {account, adapter positions}; ≥10,000 fuzz runs per invariant (spec) | M3 |
| 2 | **Owner exit** while the agent is active, paused, mid-submission, or the server is down; and with a market at 100% utilisation | Pause, revoke and withdraw succeed directly | `owner_exit_never_depends_on_agent` (fork plus `cast` script) | All available liquidity recovered; the unavailable remainder reported, not hidden | M4 |
| 3 | **Prompt injection** in protocol metadata or governance text | No effect beyond an ordinary proposal | Fixtures `injection/` through the full agent path | State changes equal the benign twin's; the text appears only as quoted evidence | M6 |
| 4 | **Model down, slow, malformed or unfaithful** (503, hang, invalid JSON, a number not in the input, refusal) | Deterministic plan unaffected; explanation marked unavailable | Drill `model-down`, faithfulness checker | At most 2 retries; zero runs stuck in a non-terminal step; zero proposals carrying an unfaithful number | M6 |
| 5 | **Bad market data**: DefiLlama and chain disagree, a spike over N× the moving average, a snapshot older than 15 min (spec) | No proposal; alert | `suspect_snapshot_blocks_planning` | Zero proposals from a suspect or stale snapshot; alert emitted | M2 |
| 6 | **Worker killed** (SIGKILL) at each step boundary and between sign and send | Resume, never repeat | Drill `kill-mid-run` (real binary, Anvil, recorded bundler) | Zero duplicate UserOperations; each run terminal or resumed; final state digest equals an uninterrupted run | M7 |
| 7 | **Bundler, paymaster or RPC failure**: down, slow, `AAxx` rejection, operation dropped, reverted on-chain | Named failure, bounded retry | Recorded-provider fixtures `bundler-faults/` | Each call ends at its deadline; `failed` with a reason code; no duplicate; balances unchanged | M5 |
| 8 | **Stale approval**: the market moves between approval and execution | Re-simulate before sending | `execution_resimulates_and_refuses_drift` | Outside tolerance → refused with `simulation_drift`; proposal expires after its TTL | M6 |
| 9 | **Pause race**: pause lands before a pending agent operation | Operation reverts | Anvil ordered-transaction test | The agent operation's inner call reverts; the audit log shows both events | M5 |
| 10 | **Sequencer down or USDC depeg** (Chainlink feeds on the fork, mocks on testnet) | No execution | `sequencer_down_blocks_execution`, `depeg_pauses_proposals` | Zero executions inside the grace period or below the depeg threshold | M7 |
| 11 | **Gas griefing**: gain below threshold, daily gas budget spent | No proposal | Property tests in `strategy` | Zero proposals with a net gain after gas below the threshold | M2 |
| 12 | **Web attacks**: replayed SIWE nonce, wrong domain or chain, expired message, CSRF on approve, approving another user's proposal | Refused | API tests over every route | 401/403 with a named code; each attempt audited | M4 |

## Ship gate

Ship when **a stranger can attack the live testnet demo with the agent's own key and fail, and can
trace every move to its numbers.** Checklist, all on the release commit:

- [ ] Failure cases 1–12 pass on CI. The invariant list is in the README, and the Slither report is
      committed with every finding triaged.
- [ ] Contract line coverage ≥ 90% (spec, week 3). Rust coverage reported by `cargo-llvm-cov`.
- [ ] Fork suite for Aave v3, Compound III and Morpho at a pinned Base block passes on CI.
- [ ] Backtest report committed for the full 90 days. Strategy parameters frozen before the window. The
      result vs. the single-protocol baseline recorded as it came out (the spec expects a win; a loss is
      reported, not retuned).
- [ ] Explanation faithfulness: 100% of numbers traceable to input on the committed eval set (spec).
- [ ] Proposal → execution under 2 minutes after approval (spec), from recorded live testnet runs with the
      distribution committed.
- [ ] Load scenarios L1–L3 below committed under `perf/results/` with machine, commit and `uptime`, all
      thresholds met or the miss recorded with a decision.
- [ ] `THREAT_MODEL.md` and an independent security review of contracts, key custody and the agent path,
      with fixes or accepted risks recorded.
- [ ] Docs site `defi-yield-agent-docs` (Astro Starlight, Cloudflare Pages): quick start ending in a real
      API call with a token, concepts, guides, an API reference from OpenAPI, self-hosting, and `/changelog`.
      Every example runs in CI.
- [ ] UI covers every operation in the list above. UI audit at 1440 and 390 px, light and dark, by a
      fresh-eyes reviewer.
- [ ] Live: contracts verified on Basescan; addresses in `deployments/base-sepolia.json`; the app works
      with the laptop off; demo account seeded with history; `/healthz` watched by Service Monitor;
      backups with a passing restore drill.
- [ ] Overview Definition of Done: README with a 60–90 s demo video, an architecture diagram, a quick
      start and known limitations; CI badge (nextest, clippy, cargo-deny); p50/p99 and memory numbers;
      `ARCHITECTURE.md`; build post and thread; clean history; MIT or Apache-2.0.

## Load scenarios (≥1,000,000 units each)

Thresholds are **proposed targets** to be confirmed at M0. A change after a measured run needs a decision
entry. No third party (RPC, bundler, paymaster, DefiLlama, model) is load-tested; all of them are
recorded fakes.

- **L1 · API reads and approvals:** k6, 1,000,000 requests at a fixed arrival rate. The mix: snapshots,
  proposals, positions, audit pages, plus 1% approvals, including conflicting approvals of the same
  proposal. The database is seeded with 10,000 accounts and 1,000,000 audit events. Proposed: zero 5xx;
  p99 ≤ 50 ms for reads; exactly one execution per proposal; server RSS ≤ 256 MiB.
- **L2 · SSE fan-out:** 2,000 subscribers × 500 events = 1,000,000 deliveries. Proposed: zero sequence
  gaps; p99 delivery lag ≤ 1 s; a slow consumer is disconnected with a named reason, never buffered
  without bound.
- **L3 · Agent cycles:** 1,000,000 state transitions across 10,000 synthetic accounts, against an Anvil
  fork and recorded bundler and model fakes, with 20 seeded kills. Proposed: zero duplicate submissions;
  every run terminal; the decision digest identical across two runs of one seed.

## Milestones (mapped from the spec's weeks)

| M | Spec week | Deliverable | Gate |
|---|---|---|---|
| M0 | 1 (first half) | Workspace, CI (fmt, clippy, nextest, deny, `sqlx prepare --check`, forge, Slither), architecture check, L1 skeleton at 1M against a stub route, `kill-mid-run` skeleton, docs-site skeleton | CI green; first load run and drill committed |
| M1 | 1 | Data worker and snapshots; read-only dashboard | Live APYs shown; case 5 data paths stubbed |
| M2 | 2 | `risk` and `strategy`; backtest | Cases 5 and 11; backtest report |
| M3 | 3 | Policy module, adapters, rebalancer | Case 1; ≥ 90% contract coverage |
| M4 | 4 | Account and session key from UI; deposit and withdraw; SIWE | Cases 2 and 12 |
| M5 | 5 | Executor: build, simulate, submit | Cases 7 and 9; scripted rebalance on Base Sepolia |
| M6 | 6 | State machine, explanations, approvals, SSE | Cases 3, 4 and 8; L2 |
| M7 | 7 | Fork suite, threat model, alerting, crash drill | Cases 6 and 10; L3 |
| M8 | 8–9 | Load runs, review, docs, UI audit, deploy, video, post | Ship gate |

## Open questions for the maintainer

1. **Account stack and EntryPoint version.** Kernel v3.3 or Safe with Safe7579 on v0.7 (widest bundler
   and paymaster support), or Kernel v4 on v0.9 (current, but young)? EIP-7702 delegation of the user's
   EOA in v1, or later?
2. **Policy implementation:** a custom ERC-7579 validator plus hook as specced, or composing Smart
   Sessions policies with a thin custom hook for the daily cap?
3. **Bundler and paymaster:** hosted (Pimlico supports v0.7/v0.8 on Base Sepolia) or self-hosted Rundler
   (LGPL-3.0, more to operate)? Who pays testnet gas for public visitors?
4. **Public demo access:** can any visitor create an account and receive a server-held session key, or
   only seeded demo accounts? Both carry abuse, cost and key-custody questions.
5. **Session-key custody:** an envelope-encrypted key on the droplet, as the spec says, or a KMS-backed
   alloy signer?
6. **Hosting:** the spec's SvelteKit on Cloudflare Pages + Docker Compose + one shared 2 vCPU/4 GB droplet,
   or the standard's same-origin Rust server under systemd on its own droplet?
7. **Database:** Neon (`sslmode=verify-full`, network latency on every state-machine commit) or Postgres
   co-located on the droplet?
8. **Jurisdiction:** whose rules frame the disclaimer and any later step (India, the US, the EU,
   elsewhere)?
9. **Design identity:** palette, type and shape language for this product (options with screenshots).
10. **Model and budget:** which Claude model for explanations, and the monthly spend cap for a public demo.
11. **Repository names:** `defi-yield-agent` and `defi-yield-agent-docs`, as the spec implies?

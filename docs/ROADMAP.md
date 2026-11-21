# DeFi Yield Agent — roadmap

M0 is this base setup; the later milestones come from the end goal (`docs/END_GOAL.md`,
mapped from the spec's weeks). A milestone's status cell says *Done* only with named
tests and evidence.

| M | Spec week | Deliverable | Gate | Status |
|---|---|---|---|---|
| M0 | — | Base setup from the starter template: CI, gate, port band 63000–63099, drills and evidence kit, `contracts/` shell | CI green on the empty product | **In progress — this base setup** |
| M1 | 1 | Data worker and snapshots; read-only dashboard | Live APYs shown; case 5 data paths stubbed | In review: `yieldagent-data` (parser, fuzz, live), `yieldagent-markets` tests `worker` and `routes`, prove-limits rows, L1-M1 `perf/scenarios/markets.js` (CI job `load-markets`, JSON record uploaded); review fixes and M2 deferrals in D-007 |
| M2 | 2 | `risk` and `strategy`; backtest | Cases 5 and 11; backtest report | Planned |
| M3 | 3 | Policy module, adapters, rebalancer | Case 1; ≥ 90% contract coverage | Planned |
| M4 | 4 | Account and session key from UI; deposit and withdraw; SIWE | Cases 2 and 12 | Planned |
| M5 | 5 | Executor: build, simulate, submit | Cases 7 and 9; scripted rebalance on Base Sepolia | Planned |
| M6 | 6 | State machine, explanations, approvals, SSE | Cases 3, 4 and 8; L2 | Planned |
| M7 | 7 | Fork suite, threat model, alerting, crash drill | Cases 6 and 10; L3 | Planned |
| M8 | 8–9 | Load runs, review, docs, UI audit, deploy, video, post | Ship gate | Planned |

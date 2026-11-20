# 01 · DeFi Yield Agent — verified knowledge

Checked on 2026-11-20 against primary sources where they exist. Each fact carries its source.
**UNVERIFIED** marks anything this pass could not confirm. "The spec says" marks a number taken from
the v2.0 spec without confirmation. Versions move quickly here; re-check them at M0 and pin them.

## 1. ERC-4337 (account abstraction)

- **Status: Final** ([EIP-4337](https://eips.ethereum.org/EIPS/eip-4337)). The ERC now lists EIP-7702 as a
  requirement and documents an optional paymaster signature.
- **EntryPoint releases** ([eth-infinitism releases](https://github.com/eth-infinitism/account-abstraction/releases)):

  | Version | Released | Address | What changed |
  |---|---|---|---|
  | v0.7.0 | 2024-02-22 | `0x0000000071727De22E5E9d8BAf0edAc6f37da032` | `PackedUserOperation`; validation rules moved to ERC-7562 |
  | v0.8.0 | 2025-03-26 | `0x4337084d9e255ff0702461cf8895ce9e3b5ff108` | native EIP-7702 support (`eip7702Auth`), EIP-712 `userOpHash`, `initCode` front-running fix; interfaces MIT, EntryPoint stays GPL |
  | v0.9.0 | 2025-11-16 | `0x433709009B8330FDa32311DF1C2AFA402eD8D009` | `paymasterSignature` (signed after the user), block-number validity ranges, `initCode` ignored once deployed, `handleOps` callable only by an EOA at top level |

  The v0.9 top-level restriction fixes a griefing vector: an attacker front-runs a pending UserOperation,
  locks the target (a reentrancy guard, a flash-loan state change), and the user pays gas for a revert.
  It is a censorship and griefing vector, not theft ([ERC-4337 team, 2026-02-05](https://erc4337.substack.com/p/improving-useroperation-execution)).
- **Validation rules (ERC-7562): status Review, not Final** ([EIP-7562](https://eips.ethereum.org/EIPS/eip-7562)).
  During validation the account and its modules may not use `TIMESTAMP`, `NUMBER`, `BLOCKHASH`, `ORIGIN`,
  `GASPRICE`, `BASEFEE` and other environment opcodes. They may read account storage and "associated"
  storage: slots equal to the account address or computed as `keccak(A||x)+n` with `n` in 0..128.
  **Consequence for this spec:** `YieldPolicyModule` cannot check "key not expired" or compute "today"
  with `block.timestamp` inside `validateUserOp`. Expiry has to travel in the returned `validationData`
  (`validUntil`/`validAfter`, which the EntryPoint enforces), and the daily cap has to be enforced in the
  execution phase (an ERC-7579 hook or the executing adapter). The spec's
  `mapping(account => mapping(day => uint256))` nests the account as the outer key, so the inner slot is
  not "associated" by ERC-7562's literal definition (inference from the rule's text). Put the account in
  the innermost key, or keep the counter where only execution reads it.
- **Common account bugs** ([Trail of Bits, 2026-03-11](https://blog.trailofbits.com/2026/03/11/six-mistakes-in-erc-4337-smart-accounts/)):
  `execute` not limited to the EntryPoint or a vetted executor; gas fields left out of the signed hash;
  state changes during validation (every validation in a bundle runs before any execution); ERC-1271
  replay across accounts or chains; paymaster logic that relies on `postOp` reverting; and
  `initialize` reachable on a 7702-delegated EOA.
- **Bundlers and paymasters on Base Sepolia (84532):**

  | Provider | EntryPoint support | Source |
  |---|---|---|
  | Pimlico (hosted) | v0.6–v0.8 bundler and paymaster; v0.9 bundler only | [supported chains](https://docs.pimlico.io/guides/supported-chains) |
  | Alchemy (hosted, runs Rundler) | v0.6, v0.7, v0.8 per its docs | [bundler overview](https://www.alchemy.com/docs/wallets/transactions/low-level-infra/bundler/overview) |
  | Coinbase CDP | bundler and paymaster in one endpoint, CDP bundler only; its basics page lists v0.6; gas credits up to $15,000 on application | [FAQ](https://docs.cdp.coinbase.com/paymaster/faqs), [AA basics](https://docs.cdp.coinbase.com/paymaster/need-to-knows/account-abstraction-basics) |
  | Rundler (self-hosted, Rust) | v0.11.0 (2026-02-12) adds v0.8 and v0.9; licence **LGPL-3.0** | [releases](https://github.com/alchemyplatform/rundler/releases), GitHub API |
  | Candide Voltaire | v0.9 supported | [blog, 2026-02-26](https://docs.candide.dev/blog/entrypoint-v09-support/) |

  The overview's "ERC-4337 client SDKs are thin in Rust" is correct. `alloy-rpc-types-eth` 2.6.1 has
  `PackedUserOperation` (v0.7 shape), `UserOperation` (v0.6), gas-estimation and receipt types, and no v0.8/v0.9
  mention ([docs.rs](https://docs.rs/alloy-rpc-types-eth/latest/alloy_rpc_types_eth/erc4337/index.html)). No
  maintained Rust AA SDK was found on crates.io. The executor builds UserOperations itself. The
  `userOpHash` is computed differently from v0.8 on (EIP-712), so test the local hash against the
  EntryPoint's `getUserOpHash` over `eth_call` for the pinned version.

## 2. EIP-7702 (EOA code delegation)

- **Status: Final** ([EIP-7702](https://eips.ethereum.org/EIPS/eip-7702)). Live on Ethereum mainnet since
  Pectra, 2025-05-07 ([ethereum.org](https://ethereum.org/roadmap/pectra/)). Live on Base since the Isthmus
  upgrade: 2025-05-09 on mainnet and 2025-04-17 on Base Sepolia
  ([Base docs](https://docs.base.org/base-chain/specs/upgrades/isthmus/overview)).
- Transaction type `0x04`. The authorization tuple is `[chain_id, address, nonce, y_parity, r, s]`. The
  delegation designator is `0xef0100 || address`. `chain_id = 0` makes an authorization valid on every chain.
- **Pitfalls** (from the EIP's security section): initialization can be front-run, because there is no
  initcode. Storage can collide when an account re-delegates, so use ERC-7201 namespaces. `tx.origin`
  checks stop meaning "top-level EOA". A phishing site that gets a user to sign one delegation takes the
  whole account. Coinbase advises an `EIP7702Proxy` with an atomic `setImplementation` rather than
  delegating straight to a wallet implementation ([CDP FAQ](https://docs.cdp.coinbase.com/paymaster/faqs)).
- **Relevance:** 7702 lets the user's existing EOA become the smart account, with no new address and no
  deposit step. Kernel v3.3 and later support this ([ZeroDev](https://www.zerodev.app/blogs/kernel-v1-v4-what-production-kept-and-what-it-left-behind)).
  It also turns a bad delegation signature into total loss, so the spec's counterfactual smart account is
  the safer default for v1.

## 3. Modular accounts and session keys

- **ERC-7579: still Draft** ([EIP-7579](https://eips.ethereum.org/EIPS/eip-7579)). Some secondary sources
  call it final; they are wrong. Module types are validator (1), executor (2), fallback (3) and hook (4).
  The listed risks: delegatecall executions, `onInstall`/`onUninstall` reentrancy, a module that blocks
  its own removal, stale state left by single-slot modules, hooks that revert and deny service, and
  batched self-calls that bypass authorization on configuration functions.
- **ERC-7715** (`wallet_requestExecutionPermissions`, built on ERC-7710 delegations): **Draft**
  ([EIP-7715](https://eips.ethereum.org/EIPS/eip-7715)).
- **Accounts** (GitHub releases and API, checked today):
  - **Kernel** (MIT): v3.3 (2025-04-03) uses EntryPoint v0.7 and adds 7702. v4.0 (tagged 2026-10-05)
    moves to v0.9. ZeroDev itself wrote on 2026-08-13 that v4 "is not yet a long-running production
    result" ([blog](https://www.zerodev.app/blogs/kernel-v1-v4-what-production-kept-and-what-it-left-behind)).
    Kernel's permissions pair one signer with policies (call, spend, rate, expiry). The blog notes that
    most integrators used the "sudo" path instead.
  - **Safe**: use Safe v1.4.1 or later with Safe4337Module 0.3.0 for EntryPoint v0.7
    ([Safe docs](https://docs.safe.global/advanced/erc-4337/4337-safe)). Rhinestone's Safe7579 adapter had
    one critical finding (counterfactual address theft) and high findings in its June 2024 Ackee review, with
    an incremental re-review in July 2024 ([Ackee](https://ackee.xyz/blog/rhinestone-erc-7579-safe-adapter-audit-summary/)).
    Whether those were fixed: **UNVERIFIED**.
  - **Biconomy Nexus** (MIT): v1.2.0, 2025-04-07.
- **Smart Sessions** (Rhinestone and Biconomy, ERC-7579 validator): v1.0.0, 2024-12-25. Its Cantina audit
  (August 2024) had a high finding: validation data was not always intersected correctly
  ([report](https://cdn.cantina.xyz/reports/cantina_rhinestone_smartsessions_core_aug2024.pdf)). The SDK
  ships policies for spending limits, time frame, usage limit, value limit, sudo and a "universal action"
  parameter-rule policy of up to 16 rules ([SDK types](https://cdn.jsdelivr.net/npm/@rhinestone/sdk@1.6.5/dist/src/modules/validators/smart-sessions.d.ts),
  [Biconomy](https://docs.biconomy.io/new/smart-sessions/policies/universal-action-policy)).
- **Spec check:** "session keys … are mature on L2s" overstates it. The standards are drafts, the leading
  session module has one audit from 2024, and the newest account version is weeks old.
  **Target, selector and amount allowlists are not enough.** Aave `supply(asset, amount, onBehalfOf, ref)`
  and `withdraw(asset, amount, to)`, Comet `supplyTo`/`withdrawTo`, and ERC-4626
  `deposit(assets, receiver)`/`withdraw(assets, receiver, owner)` all take a recipient. A policy that
  allows the selector but not the argument lets a stolen key send funds anywhere. The spec's adapters, which
  hard-code the recipient as the account, close this only if the session key can call **nothing but the
  adapters**. That also means: no delegatecall execution mode, no `installModule`/`uninstallModule`, no
  self-calls, no `approve` or `permit`, and every call in a batch decoded and checked.

## 4. Rust chain access and sign-in

- **alloy 2.6.1** (2026-11-16, MIT OR Apache-2.0, MSRV 1.94.1)
  ([crates.io](https://crates.io/crates/alloy), [docs.rs](https://docs.rs/crate/alloy/2.6.1)). The spec's
  "alloy" is right and current. `sol!` lives in `alloy-sol-types` 1.7.3. The signers include local,
  Ledger, Trezor, AWS KMS, GCP and Turnkey, which matters for the session-key custody decision. The
  workspace's pinned 1.99.1 toolchain satisfies the MSRV.
- **SIWE (EIP-4361): Final** ([EIP-4361](https://eips.ethereum.org/EIPS/eip-4361)). Required fields:
  domain, address, uri, version, chain-id, nonce, issued-at. Contract accounts verify with ERC-1271, and
  undeployed (counterfactual) accounts with **ERC-6492 (Final)** ([EIP-6492](https://eips.ethereum.org/EIPS/eip-6492)).
  **The spec names the `siwe` crate.** Its last release is 0.6.1 (2024-02-29), and its EIP-1271 support
  needs the deprecated `ethers` crate. The maintained successor is `signinwithethereum` 0.8.1
  (2026-04-18, MIT OR Apache-2.0, optional `alloy ^1`, repository `signinwithethereum/siwe-rs`)
  ([crates.io](https://crates.io/crates/signinwithethereum)). Check that it accepts alloy 2 before
  adopting it (**UNVERIFIED**).
- Money types: `rust_decimal` 1.43.0 has a 96-bit mantissa. Keep `U256` for every on-chain amount and use
  decimals only for APY display and maths.

## 5. Protocol integration surfaces on Base

- **Aave.** On Base the relevant market is v3 (the Aave changelog reports v3.7 reaching Base on
  2026-05-29; the `aave-v3-origin` v3.7.0 GitHub release is dated 2026-08-05 — the two dates disagree)
  ([changelog](https://aave.com/docs/resources/changelog)). Aave V4 went live on Ethereum on 2026-03-30
  ([The Block](https://www.theblock.co/news/defi/2026-03-30-aave-v4-launches-ethereum-mainnet-395617)). The
  address book's `AaveV4Base` lists only a tokenized-equities hub, so V4 is not a USDC supply venue on Base
  (whether it is live: **UNVERIFIED**). **Base Sepolia v3:** Pool `0x8bAB6d1b75f19e9eD9fCe8b9BD338844fF79aE27`,
  and the market's USDC is Aave's own test token `0xba50Cd2A20f6DA35D788639E581bca8d0B5d4D5f`, **not
  Circle's** test USDC ([aave-address-book](https://github.com/bgd-labs/aave-address-book/blob/main/src/AaveV3BaseSepolia.sol)).
  Rates: `currentLiquidityRate` is an APR in ray (1e27). APY = (1 + APR/31,536,000)^31,536,000 − 1
  ([rates guide](https://docs.aave.com/developers/guides/rates-guide)).
- **Compound III (Comet).** Base mainnet cUSDCv3: `0xb125E6687d4313864e53df431d5425969c15Eb2F`
  ([deployments/base/usdc/roots.json](https://github.com/compound-finance/comet/blob/main/deployments/base/usdc/roots.json)).
  The official repository has **no Sepolia or Base Sepolia deployment** (both paths return 404). Compound
  runs only on a mainnet fork. `getSupplyRate(utilization)` returns a per-second rate scaled by 1e18
  ([docs](https://docs.compound.finance/interest-rates)).
- **Morpho.** Morpho Blue core on Base is `0xBBBBBbbBBb9cC5e90e3b3Af64bdAF62C37EEFFCb`, and Base Sepolia
  appears in the deployment list ([addresses](https://docs.morpho.org/get-started/resources/addresses/)).
  Whether a funded USDC vault exists on Base Sepolia: **UNVERIFIED**. For a supplier, the surface is a
  vault (ERC-4626). Vault V2's `maxDeposit`/`maxMint`/`maxWithdraw`/`maxRedeem` **always return zero**, so
  generic ERC-4626 code that checks them breaks. Withdrawals draw on idle assets and then on the liquidity
  adapter, with no fall-through to other markets. `forceDeallocate` is the in-kind exit, with a penalty of
  up to 2% ([Vault V2 docs](https://docs.morpho.org/developers/contracts/morpho-vaults-v2/)). Curator risk
  is real: the xUSD collapse in November 2025 left bad debt in curated Morpho and Euler markets
  ([Decrypt](https://decrypt.co/347285), [Protos](https://protos.com/stream-finance-meltdown-winners-and-losers-in-defi-risk-curator-reckoning/)).
  The risk score therefore needs a curator and collateral-exposure factor; the spec's six factors don't
  have one.
- **Withdrawal liquidity.** The spec's invariant "owner can always withdraw 100%" holds for the smart
  account, not for the market. An Aave reserve at 100% utilisation, or a Morpho vault whose liquidity
  adapter is drained, refuses the withdrawal. State the invariant as: the owner's exit never depends on
  the agent or the server, and recovers everything the market can release.
- **Uniswap** is out of v1 scope (stablecoin lending only). v4 core is BUSL-1.1 until 2027-06-15, then MIT
  ([Uniswap](https://support.uniswap.org/hc/en-us/articles/33829751588109-Uniswap-v4-licensing)). The licence
  limits deploying copies of the code; calling the deployed contracts is a different matter (inference).
  If swaps are ever added, Permit2 (`0x000000000022D473030F116dDEE9F6B43aC78BA3`) and the Universal Router
  widen the approval surface the policy must block ([v4 deployments](https://docs.uniswap.org/contracts/v4/deployments)).

## 6. Oracles and data sources

- **Chainlink L2 sequencer uptime feeds** are **mainnet only**: Base `0xBCF85224fc0756B9Fa45aA7892530B47e10b6433`,
  Arbitrum `0xFdB631F5EE196F0ed6FAa767959853A9F217697D`, OP `0x371EAD81c9102C9BF4874A9075FFFf170F2Ee389`.
  Chainlink adds no new networks to these feeds. Pattern: `answer == 0` means the sequencer is up; then
  require `block.timestamp − startedAt > GRACE_PERIOD` (one hour in the example). On Arbitrum only, a
  `startedAt` of 0 means the feed is not initialized ([docs](https://docs.chain.link/data-feeds/l2-sequencer-feeds.md)).
  On testnet, mock the feed.
- **USDC/USD** (Chainlink's directory data, the source of its docs tables): Base `0x7e860098F58bBFC8648a4311b374B1D669a2bc6B`
  (8 decimals, heartbeat 86,400 s, deviation 0.3%); Base Sepolia `0xd30e2101a97dcbAeBCBC04F14C3f624E67A35165`
  ([Base](https://reference-data-directory.vercel.app/feeds-ethereum-mainnet-base-1.json),
  [Base Sepolia](https://reference-data-directory.vercel.app/feeds-ethereum-testnet-sepolia-base-1.json)).
  The directory has several "USDC / USD" entries on Base and Arbitrum, so choose by the docs page, not by
  name. **Pitfall:** with a 24-hour heartbeat and a 0.3% threshold, a staleness check of one hour rejects
  most reads, and a small depeg never moves the feed. Use the depeg check as a coarse alarm only.
- **DefiLlama yields:** `/pools` and `/chart/{pool}` (historical APY and TVL) are free and keyless on
  `yields.llama.fi`. Borrow-side endpoints are Pro-only. The free rate limit is "standard", with no
  number given ([API docs](https://api-docs.defillama.com/llms.txt)). DefiLlama's own pages price Pro at
  $300/month and $250/month; check before paying. The 90-day backtest can run on the free endpoint;
  record the responses and replay them.
- **DefiLlama `/pools` body, observed at M1 (2026-11):** `{"status":"success","data":[...]}`,
  14,893 entries, about 10 MB uncompressed; each entry has `pool` (a UUID string), `chain`
  (`"Base"`), `project` (`aave-v3`, `compound-v3`, `morpho-blue`), `symbol`, `apy` and
  `tvlUsd` as JSON numbers (`apy` sometimes `null`). 30 entries matched Base + USDC + the
  allowlist; the largest `apy` in the body was about 448,575 (percent), so a plausibility
  ceiling is required. Recorded subset: `crates/yieldagent-data/tests/fixtures/`.

## 7. Contracts toolchain

Foundry v1.10.0 (2026-11-19), Slither 0.11.6 (2026-07-28, AGPL-3.0 — using the tool does not affect our
licence), OpenZeppelin Contracts 5.7.0 (2026-07-29) — all from GitHub releases. The EntryPoint is GPL.
Import only the MIT interfaces and the repository can stay MIT/Apache-2.0.

## 8. LLM (explain step)

There is no official Anthropic SDK for Rust, so the shared crate uses raw HTTP. Structured outputs are
**generally available** through `output_config.format` with `type: "json_schema"`. The schema subset
rejects `minimum`/`maximum`, `minLength`/`maxLength`, `oneOf` and recursive schemas with a 400
([structured outputs](https://platform.claude.com/docs/en/build-with-claude/structured-outputs)).
**Pitfall:** `schemars` emits `minimum: 0` for unsigned integers. Strip unsupported keywords before
sending, and validate locally. Current model ids include `claude-opus-5-5`, `claude-sonnet-5-5` and
`claude-haiku-5-5` (same page). The faithfulness check ("every number in the explanation appears in the
input") stays our job either way.

## 9. Test networks and funds

Circle test USDC on Base Sepolia is `0x036CbD53842c5426634e7929541eC2318f3dCF7e`
([Circle](https://developers.circle.com/stablecoins/usdc-contract-addresses)). Aave's market uses its own
token (section 5), so the demo moves between two different "USDC"s unless it uses one market's token
throughout. Base's docs list the CDP faucet at 0.1 test ETH per 24 hours
([Base faucets](https://docs.base.org/base-chain/tools/network-faucets)). **Realistic plan:** Aave v3 and
Morpho on Base Sepolia, plus a mock adapter. All three protocols, Chainlink feeds included, run only in
fork tests at a pinned Base mainnet block.

## 10. Legal and regulatory constraints on a public demo (not legal advice)

- **United States.** A 2026-04-13 SEC staff statement lets DeFi front-ends skip broker-dealer
  registration only under conditions, one of which bars giving investment recommendations or advice
  ([WilmerHale](https://www.wilmerhale.com/en/insights/client-alerts/20260417-sec-staff-issues-broker-dealer-registration-guidance-for-certain-user-interfaces)).
  A yield agent's proposals are recommendations. Whether automated crypto management needs
  investment-adviser registration: no staff statement found (**UNVERIFIED**). The CLARITY Act stalled when
  cloture failed 49–50 on 2026-09-15 ([Orrick](https://www.orrick.com/Insights/2026/10/The-CLARITY-Act-Stalls-in-the-Senate-Whats-Next-for-Digital-Asset-Regulation)).
- **European Union.** "Providing advice on crypto-assets" and "portfolio management of crypto-assets"
  are licensed MiCA services ([ESMA, Art. 81](https://www.esma.europa.eu/publications-and-data/interactive-single-rulebook/mica/article-81-providing-advice-crypto-assets)).
  The transitional period ended on 2026-07-01 ([ESMA](https://www.esma.europa.eu/sites/default/files/2026-06/ESMA75-113276571-1710_Public_Statement_MiCA_transitional_period_ends.pdf)).
- **India.** The PMLA notification of 2023-03-07 covers "safekeeping or administration of virtual digital
  assets or instruments enabling control over" them. Providers must register with FIU-IND, offshore ones
  included ([PIB](https://pib.gov.in/PressReleasePage.aspx?PRID=1991372)). A server-held session key could
  be read as such an instrument. Gains on VDAs are still taxed at 30% with 1% TDS in 2026; section numbers
  under the Income-tax Act 2025 are **UNVERIFIED**.
- **What the public demo may do:** testnet tokens only; a hard cap in the contract; no mainnet deposits,
  fees, performance claims or promised yields; a visible "demo, not financial advice" notice; mainnet
  data shown read-only. Telling the agent's proposals apart from advice is a question for counsel before
  any real-money step.

## 11. Corrections to the spec

1. The spec names no EntryPoint version. v0.9 is current, but bundler and paymaster support for it is
   partial, and the stable accounts (Kernel v3.3, Safe4337Module 0.3.0) target v0.7. Choose and pin one.
2. The policy module as written breaks ERC-7562: it reads the timestamp in validation, and its nested
   per-day mapping is not associated storage. Expiry goes through `validationData`; the cap is enforced
   at execution.
3. Selector allowlists do not stop a stolen key redirecting funds. Constrain recipients through the
   adapters, and forbid delegatecall, module changes, self-calls and approvals.
4. "Owner can always withdraw 100%" is bounded by market liquidity.
5. Compound III has no Base Sepolia deployment, and Aave's testnet uses its own USDC.
6. `siwe` is unmaintained; use its successor.
7. "Mature" modules: ERC-7579, ERC-7715 and ERC-7562 are Draft or Review.
8. The risk score lacks curator and collateral-exposure factors.

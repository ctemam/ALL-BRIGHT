# GAP-28 — Cross-Chain Profit Consolidation via LI.FI REST

Goal: move each chain Safe's accrued profit to a single treasury Safe.
Pure HTTP — `reqwest`-compatible, no TS SDK needed. All endpoints cited from
docs.li.fi.

## Endpoint spec (base https://li.quest/v1)

### 1. Quote — GET /v1/quote
Params (docs.li.fi/agents/reference/endpoint-specs):
`fromChain, toChain, fromToken, toToken, fromAmount, fromAddress, toAddress,
slippage (default 0.005), order=FASTEST|CHEAPEST, allowBridges, denyBridges,
allowExchanges, maxPriceImpact`.

For us: `fromAddress = <CHAIN_PROFIT_SAFE>`, `toAddress = <TREASURY_SAFE>`,
`fromToken` = each profit token (sweep_architecture.json), `toToken` = hub
stable (USDC on hub chain), `integrator` = "allbright".

Response: `Step` with `estimate{toAmount, toAmountMin, feeCosts[], gasCosts[],
approvalAddress}` + **`transactionRequest{to,data,value,gasLimit,gasPrice,
chainId}`** — a ready-to-sign tx (docs.li.fi/agents/workflows/execution).

### 2. Approve (ERC-20 inputs only)
`estimate.approvalAddress` tells us which spender to approve — DO NOT assume
the diamond; build `approve(approvalAddress, fromAmount)` from the Safe.
LI.FI diamond `0x1231DEB6f5749EF6cE6943a275A1D3E7486F4EaE` on most networks
(docs.li.fi/introduction/lifi-architecture/smart-contract-addresses — "on
some networks the address is different"; verify per chain).

### 3. Execute
Submit `transactionRequest` as a Safe tx:
- **Conveyor path (preferred)**: conveyor key calls
  `RolesMod.execTransactionWithRole(to, value, data, Send, CONVEYOR, true)` —
  single-sig, role-confined, allowance-metered (GAP-20 role).
- **Ceremony path**: Safe `execTransaction` 2-of-3 — Commander signs; use for
  amounts above the role allowance.
Sequence per consolidation: `approve` (role-scoped) → LI.FI call → poll.

### 4. Track — GET /v1/status
`?txHash=<hash>&fromChain=<id>&toChain=<id>&bridge=<tool>` →
`status ∈ PENDING|DONE|FAILED|NOT_FOUND`, `substatus ∈
WAIT_SOURCE_CONFIRMATIONS|WAIT_DESTINATION_TRANSACTION|COMPLETED|PARTIAL|
REFUND_IN_PROGRESS|BRIDGE_NOT_AVAILABLE|…`; `receiving.amount` +
optional `quote` echo for quote-vs-settled reconciliation
(docs.li.fi/api-reference/check-the-status…, /status-tracking).
**NOT_FOUND is ambiguous** (404/1003 or 200+NOT_FOUND): poll within a bounded
window, then reconcile on the source chain — never auto-resubmit.

### Optional: POST /v1/quote/contractCalls (BETA)
Bridge + compose destination calls (e.g., bridge → deposit into treasury
module). Beta + no native-ETH payable + no gasless — treat as optional
enhancement, not required. Also `POST /v1/advanced/stepTransaction` re-mints
tx data for a stored step when a quote has gone stale before broadcast
(`?skipSimulation=true` for speed).

## Tokens & chains
Source tokens = the per-chain profit set (sweep_architecture.json). Route
everything → USDC on the hub chain. **Hub recommendation: Base or Ethereum**
— deepest liquidity, all tools support it; Commander decides.
Chain support: LI.FI covers all 10 target networks; **verify Blast + USDB
liquidity explicitly** — if no route, fall back to Blast native bridge (USDB
→ ETH withdrawal, manual cadence) and document.

## Cost model (per consolidation leg)
- `estimate.feeCosts[]` — bridge/protocol fees (incl. LI.FI fee rows when
  `integrator` carries a fee param; we use none → feeCosts are third-party
  protocol/DEX only).
- `estimate.gasCosts[]` — source-chain gas (and any dest execution gas).
- Slippage bound: `toAmountMin` — worst-case received, quote-time.
- Rule: consolidate only when `accrued_usd ≥ CONSOLIDATION_MIN_USD` (default
  $500) AND `(fromAmount − toAmountMin + gasUsd)/fromAmount ≤ 2%`. Batch all
  tokens of a chain into separate legs but one decision tick; stable→stable
  cross-chain legs are typically sub-0.5% total on the major bridges.

## Failure modes (all fail-closed)
| Mode | Handling |
|---|---|
| Quote stale before broadcast | refetch via `/v1/advanced/stepTransaction`; never send stale tx |
| Tool paused/degraded mid-flow | `denyBridges=<tool>` + re-quote; log tool for journal |
| status=FAILED | funds refund to `fromAddress`=Safe; reconcile, re-quote different tool |
| status=PARTIAL | partial delivery — journal `receiving.amount`, decide re-bridge of remainder |
| NOT_FOUND after window | reconcile on source chain by hash before ANY resubmission |
| 429/5xx | exponential backoff, max 5; LI.FI outage = consolidation deferred (fail-closed, funds stay in Safe) |
| approvalAddress ≠ expected diamond | refuse + alert (possible facet/router change) |

## Rust sketch
```rust
// crates/allbright-executor/src/lifi.rs (new)
let quote: Step = reqwest::get(format!(
  "{BASE}/v1/quote?fromChain={c1}&toChain={c2}&fromToken={t1}&toToken={t2}\
   &fromAmount={amt}&fromAddress={safe}&toAddress={treasury}&slippage=0.003\
   &order=CHEAPEST&integrator=allbright")).await?.json().await?;
// gate: toAmountMin ≥ min_recv → build approve + tx bytes →
//   RolesMod.execTransactionWithRole calldata → conveyor-signed tx →
// poll GET /v1/status?txHash → journal row on DONE/FAILED/PARTIAL
```
Reuse `PimlicoClient`'s reqwest patterns; journal every leg to
`data/transfers.jsonl` with `kind: "consolidation"`.

## Preconditions / rollback / split
- **Preconditions**: GAP-20 deployed (Safes + conveyor role + allowances);
  hub treasury Safe address chosen; LI.FI chain/token coverage verified per
  source chain.
- **Rollback**: stop the consolidator loop — funds simply remain in
  per-chain Safes; role allowances bound worst case; revoke conveyor role
  via Safe tx to kill the rail entirely.
- **Pure-Rust**: everything — quote/approve/build/track/journal.
- **Commander**: hub-chain decision, Safe/Roles deploys, conveyor key
  custody (Turnkey), LI.FI account if an API key is ever needed (public API
  works without one; key raises limits and enables integrator tracking).

## Alternatives (one-liners)
- **Across API**: `GET /suggested-fees` + `depositV3` call — cheapest for
  stable→stable same-asset routes, but bridge-only (no DEX compose) and
  deposit-based UX; good CHEAPEST alternative inside LI.FI anyway.
- **Relay.link API**: `GET /quote` intent-based, fast fills, REST-shaped
  like LI.FI — solid fallback rail for same-asset consolidation; fewer
  chains/compose options.

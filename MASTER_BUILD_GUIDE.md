# ALLBRIGHT MASTER BUILD GUIDE

## Competitive Evolution Engine — Depth × Width

**Status:** Architectural source of truth. `AGENTS.md` is the binding
constitution derived from this document; `.devin/skills/allbright-engineering/SKILL.md`
is the implementation playbook derived from it. Where they diverge, this
document governs intent and `AGENTS.md` governs enforcement.

**Naming:** the project was designed as Mega3 and shipped as Allbright —
`mega3-*` module names in the source spec map to `allbright-*` crates.

---

## 1. Strategic Mission

Allbright is an **evidence-driven, adaptive, low-latency arbitrage
intelligence and execution system** competing on five dimensions:

1. Opportunity Intelligence
2. Economic Accuracy
3. Latency
4. Execution Quality
5. Adaptive Intelligence

```
Competitive Advantage = Opportunity Coverage × Economic Accuracy
                      × Execution Probability × Speed × Adaptability
```

Latency is a weapon, not the strategy.

## 2. Depth × Width Doctrine

**Width** — continuously expand the economically relevant search space:
chains, DEXs, protocols, pools, tokens, pairs, routes, market states,
opportunity classes, strategy classes, execution venues.

**Depth** — improve discovery, observation, simulation, economic
calculation, risk analysis, route optimization, decision-making,
execution, latency, learning, adaptation.

> Width without intelligence creates noise.
> Depth without width creates blindness.
> Allbright's advantage comes from increasing both simultaneously.

## 3. Independent Opportunity Observatory (IOO)

`allbright-observatory` (:3106) — **not a trading strategy**. It
independently observes the economically relevant opportunity envelope
without strategy thresholds: price relationships, liquidity differences,
state changes, pool movements, cross-venue divergences, route
possibilities, unusual flow, temporary dislocations, emerging structures.
It is the reference against which strategies are evaluated.

## 4. Three Independent Truths

- **Market truth** — what the observable market actually did.
- **Decision truth** — what Allbright detected, assumed, filtered, decided.
- **Economic truth** — what the candidate would have produced under real
  state and costs.

Never silently merged.

## 5. Anti-Self-Approval Constitution

No strategy is the sole source of evidence validating its own opportunity
universe, thresholds, profitability assumptions, performance claims, or
coverage. A rejected opportunity remains useful evidence (enforced by
`Event::Rejected` + `allbright-challenger`).

## 6. Opportunity Lifecycle

```
OBSERVED → CANDIDATE → QUALIFIED → SIMULATED → ECONOMICALLY_POSITIVE
→ RISK_APPROVED → EXECUTION_READY → EXECUTED → CONFIRMED
→ PROFITABLE / UNPROFITABLE
```

Every rejection carries an explicit reason (`RejectReason` in `events.rs`).

## 7. No-Opportunity Diagnostic Framework

Distinguish `NO_MARKET_OPPORTUNITY`, `NO_DETECTED_OPPORTUNITY`,
`NO_QUALIFIED_OPPORTUNITY`, `FILTERED_BY_POLICY`, `DATA_BLINDNESS`,
`SIMULATION_FAILURE`, `EXECUTION_UNAVAILABLE`. "No opportunity" must never
mean "Allbright could not detect or qualify it" silently.

## 8. Three Universe Layers

- **Observation** — everything safely monitorable (all 10 chains).
- **Qualification** — passes technical/economic/risk prerequisites.
- **Execution** — authorized + technically prepared (BSC today).

## 9. Continuous Universe Expansion

Investigate new chains, DEXs, pools, tokens, pairs, routes, protocols,
market states, opportunity classes. Triggers: pool creation, liquidity
migration, unusual volume, new token activity, new DEX deployment, CEX
listings, cross-venue divergence, state changes, replay discoveries,
Challenger discoveries, external market-data changes.

## 10. Opportunity Universe Registry

Authoritative registry: chain → DEX → protocol → pool → token → pair →
route → market state → strategy compatibility, each with risk status,
simulation/execution support, observation/qualification/execution status,
last validation. Statuses: `KNOWN / OBSERVED / VALIDATED / QUALIFIED /
EXECUTION_READY / UNKNOWN`. Versioned, auditable.

## 11. Universe Blindness Framework

Every major release tests: chain, DEX, pool, token, pair, route, state,
and strategy blindness.

## 12. Discovery Beyond Percentage Spread

Largest spread ≠ best opportunity. Consider fee asymmetry, liquidity
asymmetry, price impact, stablecoin deviation, concentrated-liquidity
gaps, route-specific pricing, temporary state changes, CEX–DEX
divergence, pool imbalance, gas windows, execution probability,
opportunity persistence. Objective: **expected risk-adjusted net economic
value.**

## 13. Opportunity Density Map

`CHAIN → DEX → POOL → PAIR → ROUTE → STRATEGY → OPPORTUNITY`, each
measured on frequency, qualification rate, profitability, net value,
persistence, competition, gas, execution success, latency sensitivity.
Drives compute allocation.

## 14. Exploration / Exploitation Governor

Never allocate 100% of compute to historically successful markets.
Reserve explicit capacity for new pools/tokens/DEXs/chains/routes/
strategies and low-frequency opportunities.

## 15. Universe Expansion Priority

```
Priority = Expected Opportunity Density × Expected Net Economic Value
         × Execution Probability × Strategic Differentiation
         ÷ Integration Cost ÷ Risk
```

Engineering prioritization — not a profit guarantee.

## 16. Strategy Challenger

`allbright-challenger` (:3107) asks *"could the strategy be wrong?"* —
evaluates rejected opportunities, thresholds, trade-size/liquidity/route
assumptions, risk limits, gas assumptions, execution probability,
alternative strategies and routes. May recommend: `LOWER_THRESHOLD`,
`EXPAND_UNIVERSE`, `CHANGE_ROUTE`, `CHANGE_STRATEGY`, `RECALCULATE_SIZE`,
`REJECT_ORIGINAL_ASSUMPTION`. Recommendations are evidence for review —
never auto-applied to production controls.

## 17. Universe Challenger

Separate question: **"Is Allbright searching the wrong map?"** — hunts
missing DEXs, pools, pairs, routes, chains, tokens, protocols, market
states, strategy classes.

## 18. Counterfactual Opportunity Engine

Rejected opportunities are preserved with enough evidence to reconstruct:
what if threshold lower / size different / route different / DEX
different / strategy different / gas different / execution probability
changed / state refreshed. A major learning source — implemented today by
the challenger's cost-model re-evaluation of `Rejected` events.

## 19. Missed Profit Ledger

Dedicated accounting of: detected-but-rejected, detected-too-late,
under-qualified, execution-unavailable, risk-rejected, strategy-missed,
universe-missed — each with gross/net estimate, reason, strategy,
universe segment, latency, eventual market outcome. Objective: minimize
economically meaningful missed opportunities, not maximize trade count.
(`research` counters: `observed_universe`, per-reason `rejections`,
`challenged`, `rescued_usd`.)

## 20. Economic Truth Engine

Simulation answers not only "will it revert?" but "was it economically
superior to the alternatives?" — gross minus DEX fees, flash premium,
gas, slippage, execution cost, ordering cost, expected failure cost, risk
adjustment. All assumptions observable and versioned.

## 21. Adaptive Strategy Portfolio

Allocation considers opportunity density, economic value, execution
probability, competition, risk, latency sensitivity, historical
performance, current regime. No strategy keeps permanent priority from
history alone (P12 — blocked on P01–P04 outcome data).

## 22. Strategy × Universe Matrix

Matrix of universe components × strategies identifies strategy gaps,
universe gaps, redundancy, underutilized classes.

## 23. Historical Opportunity Archaeology

Replay is a discovery mechanism: what was captured, rejected, detected
late, **never detected** — the last category matters most. Replay tests
the search boundary, not only strategy correctness.

## 24. Benchmark Gap Intelligence

Per reference: capability → problem solved → assumptions → market
boundary → limitation → Allbright adaptation → improvement → measured
KPI. Artemis → modular collectors; Flashbots → searcher/execution
separation; Rusty-Sando/revm → fast local evaluation. **Reverse-engineer
capabilities, never inherit limitations.**

## 25. Competitive Lineage Matrix

Every major subsystem records: source lineage → Allbright adaptation →
original contribution → performance/economic/coverage/risk impact.
(`lineage.rs` is the machine-readable binding.)

## 26. Execution Intelligence

Opportunity discovery and execution remain separate. The execution layer
evaluates execution probability, routing, inclusion conditions,
competition, timing, validity. Builder capabilities are verified per
target chain — never assumed from Ethereum infrastructure.

## 27. Latency Architecture

Targets until measured: CEX ingestion <50 ms, shared-memory IPC <10 µs,
local simulation <120 µs, solver <200 µs, dashboard ≤33 ms. Every claim
reports median/p95/p99/p99.9, sample size, hardware, version, measurement
boundary. Never convert aspiration into measured result.

## 28. Latency × Coverage Optimization

Continuously evaluate "more search vs faster search" — objective:
**maximum expected economic value per unit of compute and time.** A 20 µs
search over a tiny universe can lose to a 100 µs search that finds real
opportunity.

## 29. Economic Compute Allocation

`density × expected net value × execution probability × uniqueness` with
an exploration reserve — compute is an economic resource, not only a
performance resource.

## 30. New Market Component Lifecycle

`DISCOVER → OBSERVE → VALIDATE → SIMULATE → SHADOW → CHALLENGE →
MEASURE → RISK REVIEW → APPROVE → PRODUCTION`. Nothing newly discovered
becomes executable automatically.

## 31. Universe Security Gate

Expansion passes component-appropriate security checks: tokens (transfer
behavior, taxes, blacklist/pause, upgradeability, ownership, honeypot
indicators, exit liquidity), protocols (verification, upgrade authority,
invariants, callbacks, simulation compatibility, exploit indicators),
chains (RPC reliability, finality, gas model, private routing,
infrastructure risk).

## 32. Dashboard — Competitive Observatory

Exposes: opportunity coverage, universe growth, universe blindness,
missed profit, strategy performance, Challenger findings, economic
accuracy, execution probability, latency, competition, exploration yield,
adaptive allocation. Answers *"where is Allbright competitive, where is
it blind, where is the next advantage?"*

## 33. Commander Competitive Control

Commander sets: profit objective, growth objective, risk posture,
exploration intensity, competitive priority. Does not manipulate:
individual thresholds, technical execution parameters, route controls,
security gates, transaction mechanics.

## 34. Elite Qualification Matrix

Evidence required across: opportunity coverage, economic accuracy,
latency, execution, risk, adaptability, universe expansion, Challenger
performance, resilience, governance. No single dimension qualifies
Allbright as elite.

## 35. Core Competitive KPIs

Universe coverage, blindness rate, detection rate, qualification rate,
economic accuracy, false positive/negative rates, missed profit value,
counterfactual recovery value, strategy/universe Challenger yield,
exploration yield and conversion, opportunity density, execution
probability/success, latency, risk-adjusted net profit, adaptive strategy
gain, competitive improvement rate.

## 36. Closed Competitive Evidence Loop

```
OBSERVE → DISCOVER → QUALIFY → SIMULATE → CHALLENGE → EXECUTE
       → MEASURE → COMPARE → LEARN → ADAPT → EXPAND → OBSERVE ↺
```

## 37. Governance Rule

Never optimize only for more trades / higher spread / lower latency /
more tokens / more chains / more strategies. Governing objective:
**more validated risk-adjusted economic opportunity captured with greater
accuracy, reliability and speed.**

## 38. Multi-RPC Intelligence & Visibility

RPC infrastructure is a competitive architecture problem, not a
networking footnote (see AGENTS.md §32, SKILL.md §29 — binding).

- **Registry, not list:** capability profile per endpoint (transport,
  methods, quota, latency, health) — design capacity 200+, never
  "query everything in parallel."
- **Request classes:** hot path / observation / refresh / simulation /
  discovery / research / dashboard / health — each with reserved budget.
- **Visibility states:** `VISIBLE / PARTIALLY_VISIBLE / DEGRADED /
  INVISIBLE / UNKNOWN` — never collapse INVISIBLE into "no opportunity."
- **Blindness telemetry:** `RpcBlindness, RpcQuotaExhaustion,
  RpcRateLimit, RpcStaleState, RpcMethodUnavailable, RpcProviderFailure,
  RpcNetworkPartition, RpcDataInconsistency, RpcLatencyExpired` are
  first-class `FailureClass` variants with `Environment` origin.
- **Per-endpoint capability:** HTTP ≠ WSS. `Config.wss_urls`
  (`<CHAIN>_WSS_URL`) exists because `arb1.arbitrum.io` serves HTTP but
  rejects WS upgrade — verified in production logs.
- **Cross-RPC validation:** configurable quorum for critical state;
  latency-critical paths stay single-source by policy.

**Never let infrastructure blindness masquerade as market absence.**

## 39. Reverse-Engineering Source & Design Lineage

*(Canonical version — absorbs the former `tor.txt` ToR lineage section.)*

Allbright is an original implementation that studies and adapts publicly
observable architectural concepts from established MEV, EVM, trading and
telemetry systems. Every subsystem preserves explicit attribution of its
architectural inspiration. Allbright must not copy proprietary source
code, private algorithms, credentials or confidential implementation
details — reverse engineering means architectural study, benchmarking,
interface analysis and independent reimplementation of publicly
observable concepts.

| Reference | Allbright component | Design influence |
|---|---|---|
| Paradigm Artemis | `allbright-ingest` (collectors, typed events) | modular async collectors; ingestion decoupled from strategy/execution |
| Paradigm Artemis (telemetry) | `allbright-dashboard` | modular observability, high-frequency system info — observability, not hot path |
| Rusty-Sando + revm | `allbright-simulation` | cached EVM state, ephemeral sim context, rapid playback |
| Flashbots MEV-Share | `allbright-executor` (`relay.rs`) | private tx/bundle concepts, BuilderRelay abstraction, inclusion monitoring |
| Binance market data | `allbright-ingest::cex_stream` | external predictive signal → typed event → divergence engine |
| PancakeSwap/Uniswap/Solidly | `allbright-core::amm`, solver, ingest | V2/V3/stable pricing, decimals, fee tiers, slippage |
| revm | `allbright-simulation` | EVM execution foundation — pinned version, never a proprietary EVM |
| Alloy | all crates | typed EVM primitives, ABI, provider interfaces |
| Pimlico / ERC-4337 | `allbright-executor::aa` | **optional non-hot-path** AA: sponsored/admin/profit ops, never the HFT path without measured evidence |
| Flashbots Hindsight | `allbright-research` | replay & opportunity reconstruction |
| Multicall3 | `allbright-core::multicall` | batched reads on free RPCs |

**Attribution rule:** every major module carries a source-lineage
doc-comment, e.g.

```rust
//! Allbright Simulation Layer
//! Lineage: Rusty-Sando (cached EVM-state concepts), revm (execution
//! engine), Allbright original (persistent opportunity sim model).
//! Independent implementation — no proprietary source copied.
```

**Rules inherited from the ToR:**

- The `<120 µs` simulation target is an **Allbright** engineering
  target — never represented as an achieved Rusty-Sando figure without
  measured evidence.
- No assumption that Ethereum Flashbots infrastructure works on other
  chains — each relay/builder integration is independently verified.
- Pimlico/ERC-4337 stays out of the MEV hot path; putting AA into
  execution requires measured latency + reliability evidence.
- Pool addresses and parameters are independently verified against
  current on-chain deployment data (§13 metadata policy).

The referenced projects are design lineage and engineering benchmarks,
not code sources. Allbright independently implements its own interfaces,
data structures, algorithms, configuration, strategy logic, execution
logic, telemetry and testing framework.

## 40. Final Principle

Two questions, always:

- **Depth:** *How can we capture this opportunity better?*
- **Width:** *What opportunities are we not even looking at?*

> **Expand the map. Improve the intelligence. Challenge the assumptions.
> Validate the economics. Control the risk. Then optimize the speed.**

---

## Module mapping

| Capability | Owner |
|---|---|
| Observation universe / IOO | `allbright-observatory` (:3106) |
| Strategy Challenger | `allbright-challenger` (:3107) |
| Universe Challenger | `allbright-challenger` (registry-gap audits) + `tools/fetch_pools.py` |
| Counterfactual engine | `allbright-challenger` + `allbright-research` |
| Missed Profit Ledger | `allbright-research` stats + journal |
| Opportunity Universe Registry | `pools.json` (verified) + `pools.unverified.json` (quarantine) |
| Density map / compute allocation | `allbright-research` → `allbright-solver` (P12 roadmap) |
| Economic Truth Engine | `allbright-simulation` + `allbright-challenger` |
| Execution | `allbright-executor` + `contracts/AllbrightExecutor.sol` |
| Lineage / failure origin | `allbright-core::lineage` |
| Dashboard observatory | `allbright-dashboard` (:3001) |
| Universe discovery pipeline | `tools/fetch_pools.py` (overlap-aware) + P06 monitor (factory events) |
| RPC intelligence / visibility | `Config::{rpc_urls,wss_urls}` + `lineage::FailureClass::Rpc*` + endpoint registry (roadmap) |

---

## Blueprint gap register (audit 2026-10-10)

Five tracked gaps vs the proactive-flash-loan reference blueprint —
see AGENTS.md Appendix B for the authoritative register. Summary:

| ID | Gap | Closure plan |
|---|---|---|
| GAP-1 | No EVM-level simulation (math sim can't see reverts) | `eth_call` dry-run gate pre-submission; revm roadmap |
| GAP-2 | Shallow pool registry (base/avax depth-blind) | Depth-aware `fetch_pools.py` + on-chain verify |
| GAP-3 | Block-cadence ingestion only | WSS Swap/Sync/PoolSwap log subscriptions |
| GAP-4 | Flashbots submit stub (fake-hash return) | Real `eth_sendBundle` or fail-closed |
| GAP-5 | UniV3-flash + ERC-3156 dead capacity | Wire into `flash_providers()` selection |

Blueprint phases 1/3/5 are Ethereum-shaped: no public mempool or builder
auction exists on Arbitrum/Base — adapt via event ingestion and
per-chain private relays; never assume mainnet semantics port.

## 41. Commander Control Plane

Allbright is submitted to the Commander as a top-tier product with a
capability of continually generating profit — agent-tested against the
readiness scorecard and the realized execution lifecycle, never
aspiration. The Commander operates the platform through the dashboard:

- **Deploy & monitor** — the dashboard is the operating surface: live
  P&L, funnel, chain/strategy/RPC/KPI state, diagnosis and heal state.
- **Activate / deactivate chains** — operator-controlled chain
  participation; `ACTIVE_CHAINS` is the live source of truth; every
  change is journaled.
- **Activate / deactivate strategy variants** — P01–P12 individually
  toggled from the control surface, effective without redeploy.
- **Expansion** — new chains and new variants enter through the
  verified registry and strategy-plug-in contract; the control surface
  reads live registries so expansion needs no UI surgery.

The control plane is part of the product, not an admin hack: controls
are journaled system events, gated by the same safety invariants as
automated changes (a chain/variant toggle can never bypass pool
verification, minProfit, or the kill switch).

The dashboard's full operating surface is the **seven-dimension spec**
(AGENTS Appendix C.5): Simulate, Deploy, Monitor, Optimize, Draw
Profit, Expand, Reports — zero-text throughout. Binding prohibitions:
no self-modifying economic parameters (calibration only, bounded and
journaled), no external heal scripts (the sentinel owns healing), and
no dead controls (builder-tip or mempool UIs are L2-impossible and
must not render on chains where they cannot function).

## 42. Competitive Forging Engine

The competitive page is a catch-up engine (AGENTS §27.2–27.4), not a
mirror: AllBright's axis ratings are **computed from live measured
state** (fitness functions — never hand-set), the gap matrix auto-derives
the distance to the best ACTIVE peer per axis, and daily snapshots
make the forging trajectory visible — all without configuration.
Benchmark admission is evidence-gated (≥12-month standing, still
shipping, verifiable position, demotion rule) so winners-then-losers
never set targets; the Commander promotes watchlist candidates and
adds competitor on-chain addresses — which the system then measures
directly — from the dashboard.

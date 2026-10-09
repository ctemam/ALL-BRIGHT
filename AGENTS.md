# ALLBRIGHT — AGENTS.md

**Elite-Grade Multi-Chain Arbitrage & Flash-Loan Intelligence Platform**

- **Project:** Allbright (formerly Mega3)
- **Networks:** 10 EVM chains — Ethereum, Arbitrum, Optimism, Polygon, BNB Smart Chain, Avalanche, Base, Linea, Scroll, Blast
- **Classification:** Elite-grade, production-oriented MEV / arbitrage / flash-liquidity research and execution system
- **Engineering objective:** Build a system capable of competing for a top-three position within its defined class through measurable performance, strategy depth, execution reliability, security, and operational discipline.

**Operating principle:** Measure first. Optimize second. Execute last.

## 1. PURPOSE OF THIS FILE

This file is the mandatory engineering constitution for every AI agent, developer agent, coding assistant, reviewer, tester, optimizer, researcher and automation agent working inside the Allbright repository. It derives from `MASTER_BUILD_GUIDE.md` (the architectural source of truth); where they diverge, the guide governs intent and this file governs enforcement.

Every agent MUST treat this document as binding project policy.

No agent may:

- redesign Allbright arbitrarily;
- introduce duplicate subsystems;
- replace existing architecture without an evidence-based reason;
- claim performance that has not been measured;
- activate live execution merely because code compiles;
- introduce an unverified protocol, DEX, builder, token, **or pool address**;
- weaken risk controls to increase apparent profitability;
- contaminate the live execution path with research code;
- replace a verified implementation with a placeholder;
- silently alter configuration or security policy;
- treat theoretical profit as realized profit.

## 2. ELITE-GRADE DEFINITION

Allbright is not considered elite because it has many strategies, a visually sophisticated dashboard, fast benchmark numbers in isolation, theoretical arbitrage opportunities, flash-loan access, or a large codebase.

Allbright qualifies for elite-grade evaluation only when it demonstrates, **with evidence**: strategy depth, accurate executable-profit calculation, high-quality opportunity detection, fast and deterministic simulation, reliable transaction construction, competitive execution, security and contract verification, operational resilience, measured latency, historical replay and validation, strategy-level attribution, and continuous performance calibration.

Top-three-tier status is a target and qualification objective, not a claim that may be made before independent measurements support it.

## 3. COMPETITIVE DOCTRINE — THE FIVE DIMENSIONS

Allbright is not defined by latency alone. It competes across five primary dimensions, evaluated **together**:

| Dimension | Competitive objective | Primary evidence |
|---|---|---|
| **Opportunity Intelligence** | Detect a broader economically relevant opportunity universe | Independent Observatory (`allbright-observatory`) |
| **Economic Accuracy** | Correctly determine executable net profitability | Exact simulation + realized outcomes |
| **Latency** | Detect, simulate, decide and submit before opportunity decay | Measured latency telemetry |
| **Execution Quality** | Convert qualified opportunities into successful executions | Inclusion, failure, slippage and gas results |
| **Adaptive Intelligence** | Improve strategy selection, thresholds and allocation from evidence | Challenger + replay + calibration |

Conceptual model:

```
Competitive Advantage ≈
    Opportunity Coverage × Economic Accuracy × Execution Probability
    × Speed × Adaptability
```

This is an engineering model, not a financial guarantee or a single scalar benchmark. Corollaries:

- A system that is extremely fast but systematically misses profitable opportunities is not elite.
- A system that detects opportunities but systematically miscalculates net profitability is not elite.
- A system that identifies profitable opportunities but cannot execute reliably is not elite.

**Latency must not become tunnel vision.** Agents MUST NOT optimize latency at the expense of opportunity coverage, economic accuracy, simulation correctness, execution reliability, security, strategy diversity, or calibration. A slower but economically superior strategy is preferable to a faster but systematically blind one.

## 4. CORE PROJECT IDENTITY

Allbright is a **multi-chain** dual-vector arbitrage and MEV system.

**Vector A — Predictive.** CEX market data (Binance bookTicker, live), CEX/DEX divergence, liquidity information, volatility/flow/imbalance indicators, historical relationships, predictive opportunity scoring.

**Vector B — Reactive.** Per-chain blockchain state (Multicall3-batched pool reads), pending/state-change information where legitimately available, pool reserve/√P/liquidity changes, swap activity and state-dependent arbitrage, reactive backrun candidates.

The two vectors share infrastructure but MUST remain logically separable so their performance is measured independently. **Neither vector may define the complete opportunity universe.**

**Current honest constraint:** pool state is refreshed by polling free-tier RPCs at block cadence (`newHeads`-driven). Seconds-scale freshness bounds Vector B; intra-block reactive strategies MUST NOT be claimed operational until measured.

## 5. THREE REFERENCE ARCHITECTURES + ALLBRIGHT ORIGINAL LAYER

Allbright is a deliberate hybrid of three premium reference lineages plus original integration. Agents MUST preserve explicit attribution and MUST NOT represent reference concepts as original inventions.

| Allbright area | Reference project / technology | Primary lesson |
|---|---|---|
| Event-driven ingestion | Paradigm Artemis | Modular asynchronous collectors |
| Simulation architecture | Rusty-Sando | Rapid state-aware simulation concepts |
| EVM execution simulation | revm | EVM-compatible local execution |
| Historical MEV research | Flashbots Hindsight | Replay and opportunity reconstruction |
| Private MEV concepts | Flashbots MEV-Share | Private orderflow / bundle concepts |
| CEX signals | Binance market-data infrastructure | Streaming market information |
| AMM execution math | Uniswap/PancakeSwap V2/V3, Solidly | DEX-specific pricing and liquidity |
| Blockchain primitives | Alloy | Typed EVM ecosystem interfaces |
| Account abstraction | Pimlico / ERC-4337 | Sponsored ops (bounded hot-path role) |
| RPC efficiency | Multicall3 | Batched on-chain reads on free RPCs |

**Allbright-original layer:** multi-chain integration, dual-vector detection, the premium strategy portfolio, CEX–DEX predictive intelligence, adaptive strategy allocation, the Independent Opportunity Observatory, the Strategy Challenger, counterfactual economics, unified telemetry, risk governance, historical replay, strategy attribution, performance calibration, and Commander-style governance.

References are lineage, not compatibility guarantees. Before integration, verify: API/interface, supported network, licensing, transaction model, bundle semantics, latency, availability, per-chain compatibility. **Never assume Ethereum infrastructure works identically on other chains.** Defects MUST be classified by origin (§30) before repair — never blame a reference project without evidence.

## 6. SYSTEM ARCHITECTURE

Repository root `D:\Allbright`:

```
Allbright/
├── Cargo.toml                    # workspace + shared deps
├── .cargo/config.toml            # target-dir = D:\allbright-target
├── ENV                           # secrets + chain RPC + policy (NOT committed)
├── pools.json                    # live-verified pool registry (see §16)
├── pools.unverified.json         # quarantined candidates
├── ecosystem.config.js           # PM2 process topology
├── IMPLEMENTATION_PLAN.md
├── AGENTS.md                     # this file (constitution)
├── .devin/skills/…/SKILL.md      # implementation playbook
├── journal/                      # append-only BSONL event journals
├── tools/
│   ├── fetch_pools.py            # overlap-aware live metadata pipeline
│   └── mark_stable.py            # sAMM retagging probe
├── contracts/                    # Foundry project
│   ├── src/AllbrightExecutor.sol # flash-loan atomic executor
│   └── test/AllbrightExecutor.t.sol
└── crates/
    ├── allbright-core/           # types, events, bus, config, amm, multicall, chain, lineage
    ├── allbright-ingest/         # CEX + per-chain EVM collectors, recorder, audit fan-in
    ├── allbright-simulation/     # pool cache + route simulation
    ├── allbright-solver/         # pool graph, cyclic arb, divergence, ranking, Rejected events
    ├── allbright-executor/       # relay adapters, calldata, AA submission (Pimlico)
    ├── allbright-research/       # incremental journal scanner, stats API
    ├── allbright-dashboard/      # BSON telemetry WS + funnel UI (:3001)
    ├── allbright-observatory/    # IOO — threshold-free spread observation (:3106)
    └── allbright-challenger/     # strategy auditor — counterfactual economics (:3107)
```

**Process topology (PM2, `ecosystem.config.js`):** each crate is a separate `allbright-*` process bound to `127.0.0.1` — ingest `:3101`, simulation `:3102`, solver `:3103`, executor `:3104`, research `:3105`, dashboard `:3001`, observatory `:3106`, challenger `:3107`. Inter-process events are BSON frames over localhost WebSocket (`allbright-core::bus`). Ingest additionally runs an **audit fan-in**: it subscribes to the solver/executor/observatory/challenger buses and journals their governance events (`Rejected`/`Challenge`/`Observed`/`Universe`/`Opportunity`/`ExecResult`/`Failure`) so the append-only journal holds the full decision trail.

Agents MUST extend this architecture before proposing structural replacement.

## 7. MODULE RESPONSIBILITIES

### allbright-core
Owns: shared types (`Event` enum, `PoolState`, `CexTick`, `SimRequest`/`SimResult`, `OpportunitySignal`, `ExecutionRequest`/`ExecutionResult`, `RejectedCandidate`, `ChallengeVerdict`), the BSON-over-WS bus, config/ENV loading with the `secrecy` secrets boundary, the chain registry, canonical Multicall3 (`0xcA11bde0…`), the AMM math module (`amm.rs`: V2, V3 in-range, Solidly-stable), and the lineage registry (`lineage.rs`). Core MUST remain dependency-light and is the single source of AMM math.

### allbright-ingest
Owns: Binance CEX stream, per-chain EVM pool collectors (Multicall3 `aggregate3` batched reads, `newHeads`-driven refresh, runtime `token0`/`token1` re-verification), Velora quotes (idle while `VELORA_API_KEY` unset), the BSONL journal recorder, and the audit fan-in. Reference lineage: Artemis + Binance. Ingestion MUST NOT execute transactions.

### allbright-simulation
Owns: the persistent pool cache (CacheDB analog), route folding through core AMM math, per-request ephemeral evaluation, gas estimation, SimResult production. Reference lineage: Rusty-Sando + revm. Simulation MUST be deterministic wherever practical — current measured latency is single-digit µs per route (math backend); any claim beyond that requires the latency-boundary methodology in SKILL.md.

### allbright-solver
Owns: the pool graph (address-keyed), cyclic route discovery (2-hop cross-venue and 3-hop triangular), CEX-divergence detection, optimal input sizing (ternary search), the opportunity funnel (`seen → simulated → profitable`), ExecRequest emission, and `Rejected` emission at every funnel exit. The solver MUST optimize net executable profit, not gross price discrepancy — `min_profit_usd` is a hard gate.

### allbright-executor
Owns: `ExecutionRequest` intake, relay adapters, contract calldata encoding (`SwapLeg` **addresses**, never symbols), dry-run/shadow execution, and the ERC-4337 path (Pimlico sponsorship, sponsored-UserOp submission on executor-deployed chains, gasless profit sweeps, contract admin). Reference lineage: Flashbots concepts + per-chain builder ecosystems. Execution MUST remain isolated from research and dashboard code. **Live submission requires a deployed `AllbrightExecutor` contract address per chain (`ALLBRIGHT_EXECUTOR_<CHAIN>`) and every gate in §17.**

### allbright-research
Owns: incremental journal scanning (offset-tracked, bounded memory — never `read_to_end` on journals), aggregate stats, coverage KPIs, replay and backtest feeds. Research code MUST NOT import private signing keys or live execution capabilities.

### allbright-dashboard
Owns: telemetry, funnel visibility, latency surfaces, blindness/coverage surfaces, system health on `:3001`. The dashboard is an observability layer, not the execution authority.

### allbright-observatory (IOO)
Owns: the independent opportunity universe on `:3106` — cross-venue spread observation over the raw ingest feed, deliberately WITHOUT the solver's profit threshold or strategy assumptions. Emits `Observed` and `UniverseStats`. It never decides whether to trade; it is the denominator that "zero qualified opportunity" is measured against.

### allbright-challenger
Owns: independent counterfactual economics on `:3107`. Consumes every economically-material `Rejected` candidate and applies the full cost model — live `eth_gasPrice`, flash premium, adverse-movement buffer — emitting `Challenge` verdicts. `challenged=true` flags a threshold for recalibration review; it never lowers a threshold automatically and never executes.

### contracts/
Owns: `AllbrightExecutor.sol` — atomic multi-leg route execution, flash-loan adapters (Aave V3, DODO), the on-chain `minProfit` guard, `pause()` kill switch, `sweep()`/`sweepNative()`. The contract is the final profit authority; off-chain estimates are advisory only.

## 8. STRATEGY PORTFOLIO

Allbright MUST support an extensible strategy registry keyed on `OppKind`. The premium portfolio:

| ID | Strategy | Status |
|---|---|---|
| P01 | Cross-DEX Arbitrage | **Live** (2-hop, V2/V3) |
| P02 | Multi-Hop Cyclic Arbitrage | **Live** (triangular) |
| P03 | CEX–DEX Divergence | Partial (Binance signal ingested; executable-price model pending) |
| P04 | Stablecoin / Correlated-Asset Arbitrage | Partial (Solidly sAMM math live) |
| P05 | State-Change Backrun | Blocked — requires per-block/mempool feed |
| P06 | Long-Tail Liquidity Arbitrage | In progress — new-pool monitor + token-risk layer |
| P07 | Concentrated-Liquidity / Fee-Tier Optimization | Partial (in-range V3 math; tick-walk pending) |
| P08 | Protocol Liquidation Arbitrage | In progress — Aave V3 liquidation adapter |
| P09 | Short-Horizon Divergence Prediction | Not started |
| P10 | Statistical Convergence | Not started |
| P11 | Liquidity-Imbalance / Flow Prediction | Not started |
| P12 | Adaptive Strategy Portfolio Allocation | Not started — blocked on P01–P04 outcome data |

The status column is the honest truth as of the last measurement window — update it only with measured evidence. **P12 must never control the entire feedback loop: the Observatory and Challenger remain outside the strategy allocator's authority.**

## 9. STRATEGY PLUG-IN CONTRACT

Every strategy MUST pass through the common funnel:

`discover → validate → simulate → optimize → estimate_profit → estimate_risk → estimate_execution_probability → rank → prepare_execution → record_outcome`

A strategy MUST NOT bypass the shared profitability gate, pool verification, liquidity requirements, simulation requirement, execution policy, or telemetry. Every strategy MUST have: unique ID, version, configuration, required data sources, eligibility rules, simulation requirements, minimum net-profit threshold, risk class, execution class, performance metrics, failure statistics.

## 10. OPPORTUNITY LIFECYCLE & REJECTION TAXONOMY

Every material opportunity progresses through:

```
OBSERVED → CANDIDATE → QUALIFIED → SIMULATED → ECONOMICALLY_POSITIVE
        → RISK_APPROVED → EXECUTED → CONFIRMED → PROFITABLE / UNPROFITABLE
```

Rejections MUST retain explicit machine-readable reasons (`RejectReason`/`OppState` in `events.rs`): `BELOW_STRATEGY_THRESHOLD`, `INSUFFICIENT_LIQUIDITY`, `EXCESS_SLIPPAGE`, `NEGATIVE_NET_PROFIT`, `HIGH_RISK`, `SIMULATION_FAILURE`, `EXECUTION_UNAVAILABLE`, `STALE_STATE`, `LATENCY_EXPIRED`. Every funnel exit emits `Event::Rejected` carrying route, gross edge, threshold, gas — enough evidence for counterfactual reconstruction later.

## 11. "NO OPPORTUNITY" GOVERNANCE

A generic `NO_OPPORTUNITY` status is forbidden. Diagnostics MUST distinguish:

- `NO_MARKET_OPPORTUNITY` — observatory saw nothing economically interesting
- `NO_DETECTED_OPPORTUNITY` — market had it; detection missed it
- `NO_QUALIFIED_OPPORTUNITY` — detected, rejected by threshold/liquidity/risk
- `FILTERED_BY_POLICY`, `SIMULATION_FAILURE`, `EXECUTION_UNAVAILABLE`, `LATENCY_EXPIRED`, `DATA_BLINDNESS`

**Zero qualified opportunities does not mean zero market opportunities.** A "no trades" report MUST decompose into these classes via research stats (`observed_universe`, per-reason `rejections`, `challenged`, `rescued_usd`).

## 12. ANTI-SELF-APPROVAL RULE (CONSTITUTIONAL)

No Allbright strategy, allocator, simulator, threshold engine, or performance-reporting component may be the **sole authority** for validating its own opportunity universe, profitability assumptions, thresholds, or performance claims. The forbidden loop:

```
ASSUMPTION → FILTER → NO OPPORTUNITY → NO TRADE → NO PROFIT
         → "ASSUMPTION APPEARS CORRECT" ↺
```

Thresholds are hypotheses to calibrate, not truths to worship: a 0.9% spread may be unprofitable or more profitable than a 1.1% spread depending on liquidity, fees, size, gas, slippage and execution probability.

## 13. NET PROFIT STANDARD

No strategy may rank an opportunity using gross arbitrage spread alone. Minimum model:

```
Gross executable trading profit
- DEX fees - flash-loan premium - gas - execution/builder costs
- expected slippage - expected adverse movement - expected failure cost
= expected net profit
```

Where probability estimates exist: `EV = P(success) × net_if_success − P(failure) × quantified_failure_cost`. Every additional economic term MUST be documented.

## 14. PERFORMANCE POLICY & TARGETS

Performance claims MUST be classified: **TARGET** (desired), **MEASURED** (documented benchmark), **VERIFIED** (repeated, reproducible), **PRODUCTION** (actual workload). No agent may convert a target into a measured or production claim.

| Component | Target | Last measured |
|---|---|---|
| Route simulation (math backend) | <120 µs | ~1–6 µs/route, production window (MEASURED) |
| Pool state freshness (free RPC) | per-block | block-cadence `newHeads` refresh (PRODUCTION) |
| CEX stream (Binance bookTicker) | <50 ms end-to-end | connected; E2E not yet instrumented |
| Solver candidate pass | <200 µs | µs-scale; benchmark file pending |
| Dashboard refresh | ≤33 ms | not yet measured |

Every performance report MUST include median, p95, p99, workload, hardware, version, dataset, concurrency, methodology. **Never report only the best observed result.**

## 15. SIMULATION RULES

The simulation layer uses the persistent pool cache updated by the ingest feed; per-request evaluation is ephemeral. A revm/CacheDB execution backend is the roadmap for transaction playback; until it lands, simulation authority is the core AMM math — and the on-chain `minProfit` guard in `AllbrightExecutor.sol` is the final arbiter. Simulation must validate: balances, reserves, √P/liquidity, fees, decimals, swap direction, gas, flash-loan repayment, final profitability.

## 16. METADATA & DEX MATH POLICY

**Guessing addresses is forbidden.** Pool, factory, token and pair data MUST come from live sources (`tools/fetch_pools.py`: GeckoTerminal → overlap-aware pair selection → on-chain verification) and MUST be re-verified at runtime (`token0()`/`token1()`, reserves or `slot0`+`liquidity`, `stable()` where the ABI exists). Failures drop to `pools.unverified.json`, never silently kept.

AMM math MUST use verified protocol logic: V2 constant-product with fee-on-input; V3 in-range √P/L math (tick-walk pending — MUST NOT be approximated as in-range-only for final execution approval); Solidly stable `k = x³y + xy³` Newton-solved — **never** price an sAMM as constant-product (pools tagged `stable` only after an on-chain `stable()` probe). Verify: token ordering, decimals, reserve units, fee tiers, sqrt-price representation, tick behavior, rounding.

## 17. EXECUTION SAFETY

Live execution requires all mandatory gates to pass:

```
Opportunity → Data Freshness → Contract/Token Validation → Route Validation
→ Exact Simulation → Net Profit Validation → Risk Validation
→ Execution Policy → Transaction Construction → Approved Submission
→ Outcome Verification
```

Failure of any mandatory gate means: **DO NOT EXECUTE.**

Current gate state: pipeline is armed (`AUTO_EXECUTE=true`, `min_profit_usd=$5`). `AllbrightExecutor` deployed on **BSC** at `0x7ce11f29f75371fe21f563713974cde96bbb409f` (owner = Kernel smart account `0x663050Be401F590E8E2c3F01f05536948275015B`). BSC submission: sponsored UserOp → smart account → `execute()`, flash-funded via Aave V3 (pool verified on-chain at submit time), `tokenIn` restricted to runtime-verified stablecoins, executor code-checked on-chain, `minProfit` enforced on-chain. **Caveat:** bundler UserOps are public-mempool — not private orderflow; the contract's profit guard bounds worst case to a revert at sponsor cost. Other chains have no executor configured (`ALLBRIGHT_EXECUTOR_<CHAIN>` unset = not deployable there).

## 18. GAS POLICY

Allbright minimizes unproductive on-chain gas loss. Agents must distinguish: simulation rejection; relay rejection; builder rejection; transaction not included; transaction included and reverted; transaction included successfully. These are different economic outcomes. Private submission is NOT a universal zero-gas-loss guarantee; reverted-tx gas MUST be modeled.

## 19. PRIVATE EXECUTION

Builder/relay adapters MUST be capability-tested individually per chain. Never assume Ethereum Flashbots semantics, identical bundle APIs, identical privacy guarantees, or guaranteed inclusion. Each adapter documents: endpoint, authentication, transaction type, bundle semantics, simulation support, response format, latency, failure modes. Public-mempool fallback is acceptable only where the atomic contract + `minProfit` guard bounds the downside — document that choice per chain.

## 20. WALLET AND KEY SECURITY

Private keys are security-critical. Agents MUST NOT: print private keys, commit secrets, embed keys in source, send keys to dashboard code, expose keys through telemetry, or use test credentials as production credentials. `ENV` holds `PRIVATE_KEY`, `PIMLICO_API_KEY`, `PROFIT_WALLET` and policy keys (`AUTO_EXECUTE`, `MIN_PROFIT_USD`, `NATIVE_USD`, `SCAN_INTERVAL_SECS`) — it MUST stay out of version control. Secrets materialize only at signing boundaries (`secrecy::SecretString`). The plaintext `PRIVATE_KEY` in `ENV` MUST be treated as exposed and rotated before real funds depend on it.

## 21. ACCOUNT ABSTRACTION

Pimlico / ERC-4337 is integrated for: sponsored-UserOp submission on executor-deployed chains (currently the only caller `AllbrightExecutor` accepts), gasless profit sweeps → `PROFIT_WALLET`, and contract admin. The Kernel smart account (`0x6630…015b` on BSC) owns the executor — not the raw EOA. Bundler latency is seconds-scale; any expansion of AA's role in latency-critical decisions requires benchmark evidence.

## 22. RESEARCH ISOLATION

Research code may read historical journals, calculate statistics, replay opportunities, compare designs. It MUST NOT access live private keys, submit transactions, modify execution configuration, silently change live thresholds, or mutate production state. Journal scanning MUST be incremental (per-file byte offsets) — `read_to_end` on growing journals caused the 1.3 GB RSS crash-loop; that failure mode is prohibited. Shadow strategies record hypothetical trades and MUST NOT submit live transactions.

## 23. DASHBOARD POLICY

The dashboard is the observability and command-intent surface on `:3001`. It MUST distinguish **market opportunity** (observatory universe), **Allbright qualification** (solver funnel), and **execution outcome** — never collapse them into one number. Telemetry is typed `Event`s over BSON frames. The dashboard MUST NOT become an execution bottleneck.

## 24. ENGINEERING RULES

Agents shall:

- **Audit before modification** — inspect workspace, modules, dependencies, tests, benchmarks, security boundaries, source lineage, telemetry, PM2/port state. Never rewrite an unfamiliar subsystem.
- **No duplication** — reuse, improve, or refactor existing implementations (canonical: AMM math lives only in `allbright-core::amm`).
- **No placeholders** — no `TODO`/`FIXME`/mock/dummy results in production-critical paths (the `DexKind::Stable` stub that produced phantom profit is the canonical violation).
- **Change control** — significant changes state: current state, problem, proposal, affected modules, expected benefit, performance/security impact, rollback path, validation plan.

Workflow: **INSPECT → MAP → VERIFY → PLAN → IMPLEMENT → COMPILE → TEST → BENCHMARK → REVIEW → INTEGRATE → REVALIDATE.**

## 25. TESTING STANDARD

Tests must exist at multiple levels: **unit** (pricing, fees, decimals, ordering, Newton-solver convergence), **integration** (ingestion → solver → simulation → executor → telemetry), **deterministic replay** (journaled events → expected state → outcome), **fork** (`forge test` against live chain state — BSC V2 cycle green), **failure** (stale data, RPC failure, invalid token, insufficient liquidity, simulation revert, builder rejection, malformed response, timeout, duplicate event, port conflict, reorg). Historical replay and shadow-mode evaluation are required for material strategy changes.

## 26. STRATEGY QUALIFICATION

A new strategy is not production-ready merely because it finds profitable historical examples. Minimum qualification:

```
Research → Historical Replay → Out-of-Sample Test → Paper/Shadow Mode
→ Simulation Validation → Execution Reliability Test → Controlled Production
```

Do not approve a strategy because it performs well only on the data used to create it. Performance reported by strategy, token, pool, route, regime, period, size, and failure reason.

## 27. COMPETITIVE SCORECARD — FIVE DIMENSIONS

The continuously updated scorecard mirrors the five dimensions of §3:

- **Opportunity Intelligence** — independently observed universe, detected, evaluated, threshold-rejected, missed; strategy blindness rate; detection coverage.
- **Economic Accuracy** — predicted vs realized net profit, prediction error, false-positive rate, false-negative rate, economic rescue rate (threshold-rejected candidates later proven viable by counterfactual).
- **Latency** — detection, state-to-simulation, simulation, solver, signing, submission, inclusion; opportunity decay during processing. Separate boundaries — never one merged "latency" number.
- **Execution Quality** — success rate, inclusion rate, revert rate, slippage, gas consumed, unproductive gas, relay performance, state staleness at submission.
- **Adaptive Intelligence** — strategy hit rate and profitability, threshold calibration, Challenger discoveries, out-of-sample improvement, allocation efficiency.

Anti-blindness KPIs (research/dashboard surfaces): Opportunity Coverage, Threshold Rejection Rate, Economic Rescue Rate, Strategy Blindness Rate, Simulation Validation Rate, Execution Conversion, Realized Profit Accuracy, False Opportunity Rate, Missed Profit Estimate, Challenger Detection Rate.

### 27.1 Competitive Benchmark Register — public snapshot

A maintained register of six public benchmark platforms used for gap analysis. **This is a snapshot, not a verified "top-tier" ranking** — it mixes distinct platform categories (relay/builder infrastructure, solver networks, intent protocols, aggregators) and MUST NOT be conflated with Allbright's technical lineage (§5). Lineage answers "what our modules descend from"; the register answers "what the market's strongest public players do better than us today".

| Benchmark | Category | Primary dimension it informs (§3) |
|---|---|---|
| Flashbots | Relay / bundle infrastructure | Execution Quality |
| bloXroute BackRunMe | Backrun infrastructure | Latency, Execution Quality |
| CoW Protocol | Batch-auction solver competition | Economic Accuracy, Opportunity Intelligence |
| UniswapX | Intent-based execution | Economic Accuracy |
| 1inch Pathfinder | Route optimization / aggregation | Opportunity Intelligence |
| Titan Builder | Block building / orderflow | Execution Quality |

**Evidence & cadence requirements:**

- **Event-triggered updates** — refresh an entry when a public source publishes a material change (new APIs, inclusion economics, fee models, network coverage).
- **Monthly source review** — re-check primary citations for staleness; flag drifted claims.
- **Quarterly Allbright gap review** — map register findings onto the five-dimension scorecard and emit testable follow-ups.
- Every register claim carries: citation, confidence, scope, comparison limits, and a testable follow-up. No unsupported rankings; no headline-latency-only comparisons.
- Register findings feed §27 scoring but never substitute for Allbright's own measured evidence.
- This is agent-driven monitoring guidance — **not** an implemented real-time feed.

## 28. AI AGENT BEHAVIOR & DEFINITION OF DONE

Agents behave as senior engineering specialists: question unsupported assumptions, verify protocol behavior, benchmark claims, preserve boundaries, identify hidden economic costs, prefer measurable improvements, document uncertainty. Never optimize a local metric at the expense of the complete system — a µs-faster simulation that produces less accurate profitability decisions is not an improvement; a "profitable" edge that is a math artifact is a bug, not a win.

A task is complete only when the change answers, with evidence:

- Did we improve opportunity coverage?
- Did we preserve or improve economic accuracy?
- Did latency improve without harming other dimensions?
- Did execution quality improve?
- Did the change improve adaptive intelligence — did the Observatory detect anything the strategy missed, did the Challenger find weaknesses?
- Is the result measured and reproducible — not claimed?

For live-execution features, §17 gates apply additionally.

## 29. COMMANDER GOVERNANCE

Commander controls intent, not technical machinery. Commander sees: opportunity environment, strategy performance, profitability, missed opportunities, execution quality, risk, system health, adaptive performance, major recommendations. Commander does not directly manipulate: latency parameters, signing internals, strategy code, security gates, wallet keys, simulation correctness controls.

## 30. SOURCE-ORIGIN DIAGNOSTIC PRINCIPLE

Sophisticated components can still fail at the system level. Every significant failure MUST enter the Source-Origin Diagnostic Pipeline:

```
WHAT failed? → WHERE (component) → WHICH lineage owns that component
    → diagnosed ORIGIN (artemis/rusty_sando/flashbots/allbright/environment)
    → root cause → corrective action → retest → verified resolved?
```

Machine-readable implementation: `allbright-core/src/lineage.rs` — `LINEAGE_REGISTRY` (component → lineage → module), `FailureClass` taxonomy, `FailureEvent{component, lineage, class, origin, stage, chain, detail}` emitted as `Event::Failure`, journaled by ingest, aggregated by research, surfaced on the dashboard.

| Origin | Meaning | Examples |
|---|---|---|
| `artemis` | collector/event-flow design assumption | stream lifecycle, async flow |
| `rusty_sando` | simulation/state-lineage defect | cache model, replay, sim bottleneck |
| `flashbots` | execution-lineage defect | bundle/relay semantics assumptions |
| `allbright` | **integration defect** — the most common case | strategy wiring, metadata tagging, adaptation errors |
| `environment` | external infrastructure | RPC drop, WSS flap, chain condition, PM2/ports |

**Lineage is not blame.** A Rusty-Sando-derived module failing on a mis-tagged stable pool is an `allbright` integration defect inside a `rusty_sando`-lineage component (canonical: the OP Velodrome sAMM phantom-profit incident). Agents MUST NOT conclude "the strategy is bad" or "the reference design is broken" without walking the pipeline.

**Authorization boundary:** self-repair applies to diagnosis, classification, and retry-safe corrections only. Auto-modifying execution, wallet, or security controls is forbidden — those require explicit operator authorization (§17, §20).

## 31. OPPORTUNITY UNIVERSE EXPANSION DOCTRINE

A superior arbitrage engine operating on a narrow universe can be inferior to a slightly slower engine operating on a much broader, higher-quality universe. Allbright competes on **Search Quality AND Search Space** — the system must continuously ask whether its universe itself is too small, not merely trade the configured one faster.

### 31.1 The universe is multi-dimensional

```
CHAINS → NETWORKS/VENUES → DEXs → PROTOCOLS → POOLS → TOKENS
      → PAIRS → ROUTES → TRADE SIZES → MARKET STATES → STRATEGIES
```

The opportunity universe is NOT "the list of pairs in `pools.json`." It is the set of markets, venues, assets, routes and states Allbright can potentially observe, evaluate and execute safely. Expansion occurs across every dimension where technically and economically justified.

### 31.2 Three universe layers — never conflate them

| Layer | Meaning |
|---|---|
| **Execution Universe** | Chains/venues Allbright is authorized and technically prepared to execute on (today: BSC via the deployed executor) |
| **Observation Universe** | Markets monitored for intelligence (all 10 chains) |
| **Research Universe** | Markets/chains/protocols being evaluated for future integration |

This separation lets intelligence expand ahead of execution risk. BSC-first execution does not mean BSC-blind intelligence.

### 31.3 Expansion dimensions

- **Chains** — evaluated on: liquidity, DEX/token diversity, activity, arb frequency, bridge/settlement characteristics, gas economics, block latency, MEV competition, RPC quality, private-execution availability, ecosystem maturity, integration complexity, security. A chain earns production when risk-adjusted opportunity density justifies it — never because it is popular.
- **DEXs** — AMMs, concentrated-liquidity, stable-swap, weighted pools, hybrids, emerging venues. Per-DEX registry records: factory/router, pool discovery mechanism, pool types, fee structure, liquidity model, swap math, state-access requirements, execution semantics, simulation compatibility, reliability, security status. **A known contract address never makes a DEX production-eligible.**
- **Pools** — dynamic discovery over manual curation: new pools, liquidity/volume changes, fee tiers, reserve imbalance, pool age, historical arb frequency, token risk. New pools traverse `DISCOVERED → OBSERVED → VALIDATED → SIMULATED → QUALIFIED → EXECUTION_ELIGIBLE` — never straight into the live universe.
- **Tokens** — liquidity, activity, pool depth, age, contract verification, transfer/fee-on-transfer/rebase behavior, blacklist/pause controls, upgradeability, ownership risk, oracle dependencies, price-source quality. Goal: every token with evidence of economically actionable, technically executable opportunity — not every token.
- **Pairs** — beyond majors: major/stable, stable/stable, major/long-tail, correlated, wrapped, synthetic, cross-venue. Pair ranking is **economic**, not percentage-spread — a smaller spread with superior liquidity/fees/execution-probability/persistence beats a larger one.
- **Routes** — graph search (`A→B`, `A→C→B`, `A→C→D→B`, `A→stable→B`, `A→DEX1→C→DEX2→B`) pruned by gas, liquidity, state consistency, depth, latency, net profit — controlled pruning, never arbitrary narrowness.
- **Market states** — normal, high/low volatility, liquidity shocks, CEX–DEX divergence, pool imbalance, large swaps, new-pool creation, liquidity removal, protocol events, unusual gas. The same pair is a different opportunity in a different state.

### 31.4 Reference-informed, not reference-limited

| Lineage | Expansion lesson |
|---|---|
| Artemis | Opportunity discovery should be modular/extensible, not hard-coded to a small market set |
| Rusty-Sando/revm | A bigger universe is only valuable if it can be evaluated efficiently |
| Flashbots | The opportunity universe and the execution universe are different problems — model separately |
| Binance | Market intelligence must not depend on a manually curated symbol list |
| DEX ecosystems | Pool math and liquidity structure create opportunities simple price monitoring misses |
| Hindsight/research | Historical evidence reveals opportunity classes live logic overlooks |

For every reference-derived capability ask: what problem did it solve, what assumptions did it make, what does it miss, what does Allbright add? **Reverse-engineer the capability, not the limitation.**

### 31.5 Universe registry, density, and priority

Maintain an authoritative, versioned, auditable registry: chain → DEX → factory/router → pool → token → pair → fee tier → route, each with data source, execution method, risk status, simulation support, execution status, last observation, opportunity density, economic performance.

Measure **opportunity density per universe segment** (opportunities/hour, economically-positive rate, mean/median net profit, lifetime, execution probability, competition, gas burden) — expand where evidence says expansion is valuable.

```
Expansion Priority ≈ Opportunity Density × Expected Net Value
                   × Execution Probability × Strategic Uniqueness
                   ÷ Integration Cost ÷ Risk
```

A prioritization framework, not a return guarantee. A low-volume chain with exceptional opportunity quality can outrank a high-volume chain with extreme competition.

### 31.6 The Challenger audits the universe too

`allbright-challenger`'s mandate extends beyond strategy decisions: which pools/DEXs/tokens/pairs/routes/fee-tiers are systematically absent? Which opportunities appear in historical data but never in Allbright? This is **Universe Blindness Detection**, not only strategy blindness detection.

### 31.7 Expansion is not noise — governance

Maximum coverage ≠ maximum value. No indiscriminate addition of illiquid tokens, malicious contracts, unusable pools, stale markets, duplicate liquidity, uneconomic chains, or unreliable data sources. Every new universe component traverses:

```
DISCOVERY → INTELLIGENCE REVIEW → SECURITY REVIEW → DATA VALIDATION
→ SIMULATION SUPPORT → SHADOW OBSERVATION → ECONOMIC VALIDATION
→ EXECUTION QUALIFICATION → PRODUCTION
```

Nothing bypasses this sequence merely because it appears profitable.

### 31.8 Strategic objective

Continuously answer: *"Where is the next profitable opportunity that our current universe cannot see?"* — as important as *"How fast can we trade what we already see?"* The first creates opportunity expansion; the second creates latency advantage. Elite requires both.

**Do not build the world's fastest search engine for a small map.**

## 32. MULTI-RPC INTELLIGENCE & VISIBILITY DOCTRINE

RPC infrastructure is a competitive architecture problem, not a networking
footnote. The failure chain is constitutional:

```
Limited RPC sources → limited request capacity → limited data acquisition
→ potential data blindness → reduced opportunity visibility
```

RPC limitations must never become an invisible cap on Allbright's
opportunity universe. **Never confuse "I cannot see it" with "it does
not exist"** — this rule sits at the same level as the anti-self-approval
rule and the Independent Observatory.

### 32.1 Endpoint pool — capacity target, not coverage claim

The architecture must support an RPC Intelligence Registry capable of
managing **200+ endpoints** across chains and providers. The number is a
design capacity target, not a claim that 200 usable endpoints exist for
free — and not an instruction to query 200 endpoints in parallel. The
objective is *200+ intelligently managed sources of blockchain
visibility*:

- official chain endpoints, public RPCs, free-tier and paid providers
- WebSocket endpoints, archive endpoints, trace/debug endpoints
- provider-enhanced RPCs, geographically distributed endpoints
- builder/private-transaction endpoints where applicable

Every endpoint is independently validated before use. The registry is
dynamic — never a static list embedded in code.

### 32.2 Registry schema (minimum)

`provider_id, endpoint_id, chain_id, network, transport (HTTP/WSS),
region, provider_type, auth, free/paid, req/s, burst, daily/monthly
limits, method limits, remaining budget, latency, p95/p99, error rate,
timeout rate, staleness, block lag, archive/trace/log/WS/private-tx
capability, health status, last_verified`.

Endpoint capability is **per-endpoint, not per-chain** — e.g. an HTTP
endpoint that lacks WSS (`arb1.arbitrum.io` — verified 400 on upgrade)
must be paired with a separate `<CHAIN>_WSS_URL` transport, not assumed
to subscribe. This is why `Config` carries `wss_urls` separately from
`rpc_urls`.

### 32.3 Request classification and budget-aware routing

Every RPC request is classified before dispatch:

```
CRITICAL_HOT_PATH / HIGH_VALUE_OPPORTUNITY / REAL_TIME_OBSERVATION
STATE_REFRESH / SIMULATION_SUPPORT / DISCOVERY / HISTORICAL_RESEARCH
DASHBOARD / BACKGROUND_HEALTH
```

Routing considers method limits, remaining quota, health, cost, latency
requirement and burst allowance — the best endpoint is the one giving the
best information service for the request, not necessarily the fastest.
Low-priority traffic must never consume capacity reserved for the live
opportunity engine. Optimize **expected information value ÷ RPC cost**.

### 32.4 Visibility states — never collapse to "no opportunity"

Every data domain is classified:

`VISIBLE / PARTIALLY_VISIBLE / DEGRADED / INVISIBLE / UNKNOWN`

An endpoint count never defines the opportunity universe: 5 endpoints ≠
5-source-only market; 200 endpoints ≠ complete visibility. Visibility is
an evidence and coverage problem, not an endpoint-count problem.

### 32.5 RPC blindness classes

First-class telemetry, never reported as `NO_OPPORTUNITY`:

`RPC_BLINDNESS, RPC_QUOTA_EXHAUSTION, RPC_RATE_LIMIT, RPC_STALE_STATE,
RPC_METHOD_UNAVAILABLE, RPC_PROVIDER_FAILURE, RPC_NETWORK_PARTITION,
RPC_DATA_INCONSISTENCY, RPC_LATENCY_EXPIRED`

A provider failing degrades coverage gracefully — observation continues
through other providers. No single RPC provider may create systemic
market blindness.

### 32.6 Cross-RPC validation and quorum

Critical state may be cross-checked across independent providers with
configurable policies (2/3, 3/5, weighted agreement, or latency-first
single-source). Divergent responses (different head heights, conflicting
state) are detected, not silently resolved. Hot-path execution never
blindly waits for every provider — confidence vs latency is a deliberate
per-request-class trade-off.

### 32.7 Health scoring, failover, reservation

Each endpoint carries a dynamic health score (latency p50/p95/p99,
error/timeout rates, block freshness, consistency, quota pressure, WS
stability). Routing is intelligent, not sequential-primary failover — a
degraded "primary" loses to a healthy secondary. Budget is reserved per
class (hot path, validation, discovery, research, health) so background
work cannot starve live detection.

### 32.8 Endpoint lifecycle

`DISCOVER → VERIFY → CAPABILITY-TEST → LATENCY-TEST → QUOTA-TEST →
HEALTH-CLASSIFY → SHADOW → PRODUCTION → MONITOR → REASSESS → RETIRE`

No endpoint becomes production infrastructure merely because its URL is
reachable. Provider limits change — hard-coded quota assumptions are
prohibited; agents verify against current provider documentation.

### 32.9 Commander visibility reporting

The Commander must see: sources available/healthy/degraded, capacity
remaining, reserved capacity, market visibility state, RPC-induced
blindness. It must never hear a bare "no opportunity" — the system
distinguishes *"no opportunity under current observable conditions"*
from *"visibility degraded because RPC capacity is insufficient."*

### 32.10 Agent reasoning requirement

Agents must reason about infrastructure: *what information is Allbright
currently unable to observe, which RPC capability resolves that
blindness, what endpoint provides it, what quota it has, and how the
capacity should be allocated* — not merely "add another RPC."

```
MORE RPC SOURCES + BETTER RPC INTELLIGENCE + SMARTER BUDGET ALLOCATION
+ CROSS-SOURCE VALIDATION + CONTINUOUS DISCOVERY
= GREATER OPPORTUNITY VISIBILITY
```

The endpoint pool is infrastructure. **Visibility is the strategic
objective.**

## 33. DEPLOYMENT READINESS = CAPACITY CERTIFICATION

A deployment gate must challenge whether the implemented system can
**deliver its designed KPI capacity** — not merely whether the
application starts or deploys.

The readiness question is never *"is AllBright deployed and running?"*
It is:

> *"Does the deployed AllBright system possess the infrastructure, data,
> compute, execution, security and governance capacity required to
> deliver every material KPI in its approved design?"*

### 33.1 The capacity–KPI challenge

For every material KPI the readiness audit MUST produce:

```
Design KPI → Required capacity → Actual capacity → Stress-test result
→ Evidence → Gap → Deployment decision
```

**Critical rule:** a KPI is not production-ready because its software
calculation exists. Its upstream infrastructure must have demonstrated
sufficient capacity to generate trustworthy KPI values at the designed
operating scale.

### 33.2 Readiness domains (mandatory coverage)

Every go/no-go review MUST address, with evidence:

- **Architecture match** — deployed components = approved design; all
  required services connected.
- **KPI coverage & accuracy** — every designed KPI computed from live
  data, traceable to source/formula/timestamp, or formally excluded
  with evidence.
- **Opportunity coverage** — intended chains/venues/pools/routes
  observed; no unexplained visibility gaps; RPC capacity not silently
  constraining discovery.
- **RPC capacity** — visibility survives individual endpoint failure/
  throttling; request volume fits measured quota.
- **State freshness** — measured freshness inside design tolerance.
- **Simulation throughput & accuracy** — meets design load; error
  within approved tolerance vs verified outcomes.
- **Strategy portfolio** — each production-approved strategy
  independently validated, not merely coded.
- **Independent challenge** — observatory/challenger path operational;
  counterfactual testing can refute a rejection.
- **Execution** — end-to-end path demonstrated; risk gates provably
  block unacceptable transactions; net profit computed pre-execution.
- **Latency** — median/p95/p99 measured across the *complete*
  production path; no hidden instrumentation gaps.
- **Contracts** — deployed bytecode/address verified identical to the
  approved build.
- **Failure & degradation** — RPC/feed/sim/executor failure demonstrably
  degrades gracefully; capacity reduction is safe, never produces false
  confidence.
- **Auditability** — any decision reconstructible; journal replay works.
- **Design-vs-reality gap** — quantified percentage of designed
  capability actually production-ready, explicitly accepted.
- **Independent readiness challenge** — the system's own readiness claim
  can be challenged by an independent test.

### 33.3 Multi-chain capacity certification

Allbright must not be certified merely for its current BSC execution
configuration. It is certified against its designed ability to operate
as a **configurable multi-chain × multi-DEX × multi-pool ×
multi-token/pair platform**.

Deployment capacity is defined as:

```
Chains × DEXs × Pools × Tokens × Pairs × Routes
× RPCs × Strategies × Market States
```

— never `BSC × selected DEXs × selected pairs`. Pool/token/pair counts
are discovered and measured, never hard-coded into a capacity claim.

Mandatory multi-chain readiness domains (evidence required):

- **Chain architecture** — supports the designed chain target (up to
  ~50) without redesign; chains add/remove via config (registry +
  `ENV` + `pools.json`), not code changes; per-chain enable/disable;
  a failed chain is isolated and cannot compromise the others.
- **Per-chain completeness** — for every chain, the audit reports
  separately: RPC pool diversity (no critical chain on one endpoint),
  state freshness, DEX coverage, pool count, simulation support,
  execution path status, relay/private-tx capability (verified per
  chain, never assumed — Ethereum semantics are not portable).
- **Observation/qualification/execution separation per chain** — a
  chain may be observation-live and execution-blocked; the matrix
  must show both.
- **RPC capability matrix** — HTTP/WS/archive/log/trace/pending/private
  per endpoint per chain; method limitations (e.g. public BSC
  `eth_getLogs` limits, HTTP-only endpoints) are recorded, not
  discovered by failure.
- **KPI hierarchy** — every KPI drills down `chain → DEX → pool →
  pair → strategy`; 72-KPI coverage mapped to component level.
- **Capacity stress** — concurrent multi-chain processing, compute
  governor under a large universe, KPI pipeline at design volume.

### 33.4 The readiness equation

```
Deployment Readiness = Configured Universe Capacity
                     × Data Visibility
                     × Simulation Capacity
                     × Execution Capacity
                     × Risk Capacity
                     × KPI Observability
```

Multiplication, not addition: a system configured for 50 chains that
reliably observes 3 is not 50-chain ready. 50 chains with 1 RPC each is
not sufficient. Thousands of pools with inadequate state retrieval is
not sufficient. Millions of pairs with weak route computation is not
sufficient. 72 KPIs displayed on incomplete data is not sufficient.

### 33.5 Commander readiness statement

The readiness surface must report the certified shape of the deployed
system, e.g.:

```
DESIGN CAPACITY:  N chains
ACTIVE / QUALIFIED chains
DEX venues / observed pools / qualified pools
tokens / pairs observed
RPC endpoints / healthy capacity %
opportunity visibility state
simulation capacity/sec, execution capacity/sec
KPI coverage n/72
DESIGN-CAPACITY READINESS: xx%
DEPLOYMENT GATE: GO / CONDITIONAL / NO-GO
```

### 33.6 Master Readiness Score — 1,100 points

Deployment readiness is a formal KPI system, not a checklist. Pillars
and weights:

| # | Pillar | Weight | Measures |
|---|---|---|---|
| 1 | Architecture & design conformance | 80 | deployed system = approved architecture |
| 2 | Multi-chain capacity | 100 | configure/operate up to 50 chains |
| 3 | Multi-DEX/pool/token/pair coverage | 100 | breadth + quality of search universe |
| 4 | RPC & data visibility | 120 | diversity, quota, state visibility, continuity |
| 5 | Opportunity intelligence | 100 | observatory, discovery, coverage |
| 6 | Simulation & economic truth | 100 | accuracy + capacity of economic sim |
| 7 | Strategy & adaptive intelligence | 80 | portfolio, challenger, thresholds |
| 8 | Execution & transaction reliability | 100 | qualified → safe execution |
| 9 | Performance & latency | 70 | measured throughput/latency/concurrency |
| 10 | Security, risk & governance | 100 | controls, gates, governance integrity |
| 11 | Observability & 72-KPI integrity | 50 | Commander sees trustworthy data |
| 12 | System wiring integrity | 100 | every component connected, correct + verified (§35) |
| | **Total** | **1,100** | |

**Capacity is scored, not configuration** — and wiring is scored, not
existence. Every pillar grades the ladder `Designed → Configured →
Observed → Validated → Production`, never the config field's existence.
50 configurable chains with 20 tested and 18 effective scores *partial*,
not 100/100. A wallet that renders an address but cannot sign scores
UNVERIFIED, not operational.

### 33.7 Score thresholds and critical gates

| Score | Recommendation | Meaning |
|---|---|---|
| 1045–1100 | GO | elite production readiness |
| 990–1044 | GO | ready, minor gaps |
| 880–989 | CONDITIONAL GO | deployable only with explicitly accepted limitations |
| 770–879 | NO-GO / REMEDIATION | material capacity or reliability gaps |
| <770 | NO-GO | not deployment-ready |

**Non-compensatory gates** — the average can never hide a critical
failure. Any of these forces NO-GO regardless of total score:

- Execution path submits unverified calldata or bypasses `minProfit`.
- A secret is exposed in repo/logs/telemetry.
- Metadata (pool/factory/token) enters execution unverified.
- Challenger/observatory independence broken (self-approval).
- Risk gates demonstrably fail a forced-failure test.
- A critical wiring path is BROKEN/MISSING/MISWIRED: wallet→signer→
  execution, flash-loan provider chain, critical RPC layer, economic
  simulation, or KPI data lineage (§35.8).
- Self-healing gate fails: no detection, no bounded remediation, no
  recovery verification, infinite retry, or audit gaps (§34.16).
- The readiness report itself contains unmeasured claims presented as
  measured.

### 33.8 Commander override

The Master Score **recommends** `GO / CONDITIONAL GO / NO-GO`. The
Commander retains final authority to override — but an override is a
governance act, not an edit: it requires an explicit reason, an expiry
(condition or window), acceptance of the specific failed gates, and a
journaled audit record. Silent overrides are prohibited; an overridden
gate still shows as failed on the readiness surface.

### 33.9 Gate rule

**GO only when critical gates pass.** A readiness claim without measured
evidence is a TARGET claim, and self-certification by the component
being certified is not evidence.

## 34. SELF-DIAGNOSIS & SELF-HEALING DEPLOYMENT CERTIFICATION

Self-diagnosis and self-healing are integral Allbright production
capabilities. They must never be treated as: decorative dashboard
features; documentation claims; inactive agents; simulated success
messages; manually triggered demonstrations; placeholder remediation
functions; or readiness-score padding.

A self-healing component receives deployment-readiness credit only when
Allbright demonstrates the complete chain:

```
FAULT → DETECTION → DIAGNOSIS → SAFE DECISION → REMEDIATION
      → VERIFICATION → RECOVERY → READINESS RECHECK
```

If any link is missing, the self-healing capability is not certified.

### 34.1 Deployment readiness must test the healer

The readiness challenge actively introduces controlled failures into the
deployed system — it does not merely inspect whether health checks
exist, healing agents run, remediation functions compile, dashboards
display "healthy", or configuration contains healing rules.

Required test sequence:

1. Establish known-good baseline.
2. Inject controlled fault.
3. Confirm the fault is actually present.
4. Verify self-diagnosis detects it.
5. Verify diagnosis identifies the correct affected component.
6. Verify the proposed remediation is appropriate.
7. Verify the safety gate permits or rejects the action correctly.
8. Execute remediation where autonomous action is authorized.
9. Verify the fault is removed.
10. Verify dependent services recover.
11. Verify KPIs return to acceptable range.
12. Verify no secondary damage was introduced.
13. Record the complete evidence chain.
14. Re-run the affected deployment-readiness tests.
15. Only then restore readiness status.

### 34.2 Diagnosis coverage — fault families

The deployment challenge must test failures across the actual Allbright
architecture. Minimum fault families:

- **Infrastructure** — RPC unavailable, RPC timeout, RPC throttling,
  RPC quota exhaustion, WebSocket failure, stale blockchain state,
  provider disagreement, network degradation, CPU saturation, memory
  pressure, storage exhaustion.
- **Market intelligence** — CEX feed interruption, DEX feed
  interruption, missing pool, stale pool state, token metadata failure,
  pair/route discovery failure, opportunity-data inconsistency.
- **Simulation** — simulation timeout, stale simulation state,
  simulation-service failure, invalid state, model mismatch,
  unexplained simulation rejection.
- **Strategy** — strategy-service failure, threshold anomaly, abnormal
  rejection rates, performance degradation, Challenger disagreement.
- **Execution** — signer failure, transaction construction failure,
  RPC submission failure, private-route/builder failure, excessive gas,
  execution latency degradation, repeated transaction rejection.
- **Security / governance** — risk-gate failure, contract-verification
  mismatch, unauthorized configuration change, abnormal wallet state,
  policy violation, governance-control failure.
- **Dashboard / observability** — telemetry interruption, KPI pipeline
  failure, dashboard/backend disconnect, missing event stream,
  inconsistent KPI values.

### 34.3 Root cause, not symptoms

Self-diagnosis must distinguish:

```
Symptom → affected component → probable cause → confirmed cause → remediation
```

Example: "opportunity count fell to zero" is not a diagnosis. The
system must determine whether RPC timeout → state acquisition degraded
→ opportunity visibility fell → the opportunity universe became
partially invisible. "No opportunity" must never be inferred from an
unhealthy data layer (§32.4). The system preserves the distinction
between: no market opportunity; no detected opportunity; no qualified
opportunity; RPC blindness; stale state; data failure; simulation
failure; execution unavailable.

### 34.4 Self-healing authority levels

| Level | Name | Authority |
|---|---|---|
| H0 | Observe | Detect and report only. |
| H1 | Recommend | Diagnose and propose remediation. |
| H2 | Auto-heal low-risk | Predefined reversible actions: switch RPC, restart failed worker, refresh stale cache, reconnect WebSocket, rebalance data requests, disable unhealthy provider, restart non-critical service. |
| H3 | Controlled autonomous recovery | Multi-step recovery for approved scenarios; requires safety checks, blast-radius limits, rate limits, rollback capability, post-action verification. |
| H4 | Human escalation | Required when diagnosis confidence is low, remediation is irreversible, capital/wallet exposure is involved, contract state could be affected, security controls are implicated, repeated remediation fails, or no safe recovery state can be established. |

Commander remains the final governance authority. **Current
implementation state: `allbright-sentinel` operates at H2 only.**

### 34.5 Self-heal must never bypass safety

Self-healing must not bypass security gates, risk controls, contract
verification, wallet controls, deployment policy, governance rules,
execution safety or audit logging. The healer is a privileged
production subsystem: every remediation records action ID, triggering
fault, diagnosis, confidence, authority level, affected component,
expected outcome, actual outcome, rollback method, verification result
and timestamp — journaled as `Event::Heal`.

### 34.6 Anti-false-healing rule

Allbright SHALL NOT report `HEALED` merely because a remediation
command completed. The state sequence is:

```
FAULT_DETECTED → DIAGNOSING → ROOT_CAUSE_IDENTIFIED
  → REMEDIATION_AUTHORIZED → REMEDIATION_EXECUTED → VERIFYING
  → RECOVERED → HEALED
```

Only successful post-remediation verification permits `HEALED`.

### 34.7 What self-heal may NEVER do

- Lower `min_profit_usd`, relax risk gates, or weaken verification to
  make a failure "go away" — a Challenger verdict is a review signal,
  not a mutation.
- Admit a discovered/quarantined pool to the execution universe.
- Modify config, secrets, contracts or policy autonomously.
- Restart-loop without a budget — persistent failure escalates to a
  classified failure + Commander-visible degradation, not infinite
  retry.
- Hide a failure: remediation supplements the failure record, it never
  replaces it.

### 34.8 Self-healing readiness score — measured KPIs

Self-healing receives readiness credit only from measured tests:

| KPI | Requirement |
|---|---|
| Fault detection rate | Measured |
| Diagnosis accuracy | Measured |
| Root-cause identification rate | Measured |
| Safe remediation rate | Measured |
| Successful recovery rate | Measured |
| False-heal rate | Measured |
| Failed-heal rate | Measured |
| Mean time to detect / remediate / recover | Measured |
| Rollback success rate | Measured |
| Escalation accuracy | Measured |
| Secondary-damage rate | Measured |
| Recovery verification rate | **100% required** |
| Audit-trail completeness | **100% required** |

### 34.9 Mandatory fault-injection certification

Before production GO, a controlled fault-injection suite must execute —
safe, reversible, isolated from real capital. At minimum: kill one RPC
provider; exhaust/simulate one RPC quota; disconnect one market-data
source; create stale blockchain state; stop one ingestion worker; force
simulation degradation; force strategy-service degradation; force
execution-provider failure; create telemetry/KPI interruption; create a
configuration inconsistency; a controlled service restart; recovery
after simultaneous non-critical failures.

### 34.10 Readiness re-test rule

A healing action is incomplete until the system proves the original
business capability returned. Example:

```
RPC FAILURE → DIAGNOSIS → RPC FAILOVER → HEALTH RESTORED
  → VISIBILITY TEST → OPPORTUNITY TEST → SIMULATION TEST → KPI TEST
```

### 34.11 Multi-chain scope protection

Self-healing operates at the smallest affected scope:
`Chain → DEX → Pool → Token/Pair → RPC → Strategy → Execution`.
A failure on chain A must not unnecessarily disable chain B, unrelated
DEXs, pools, strategies or the entire opportunity universe. Prefer:

```
isolate → reroute → degrade locally → recover
```

over global shutdown, unless a genuine systemic safety condition exists.

### 34.12 Visibility vs market absence

If RPC failure causes opportunity visibility to fall, the system state
is `Opportunity Visibility = DEGRADED` — never `No Opportunities`.
Self-diagnosis must attribute reduced opportunity count to its actual
origin: market conditions, data loss, RPC blindness, pool-state
blindness, strategy filtering, simulation failure or execution
unavailability.

### 34.13 KPI integrity wiring

If a critical upstream component fails: affected KPIs become
degraded/stale; the system identifies affected lineage; self-healing
attempts recovery; KPI streams are revalidated; historical gaps are
marked; readiness is recalculated. The dashboard must never display
apparently normal KPI values when their underlying data source is known
unhealthy.

### 34.14 Evidence ledger

Every diagnosis and remediation produces machine-readable evidence,
journaled via `Event::Heal` + `Event::Failure`:

`incident_id, fault_type, affected_component, chain, dex, pool,
token/pair, rpc_provider, detection_timestamp, diagnosis_timestamp,
diagnosis_confidence, root_cause, remediation_action,
authorization_level, execution_timestamp, verification_timestamp,
recovery_status, kpi_impact, readiness_impact, rollback_status,
final_disposition`

This ledger is part of deployment certification and post-deployment
audit.

### 34.15 The sentinel service

`allbright-sentinel` (:3108) is the remediation authority:

- **Watches** — TCP liveness on every service port every scan tick, plus
  the classified failure stream on the ingest bus.
- **Policy table** — port dead ≥2 consecutive ticks → `pm2 restart
  <service>`; recorded as `Heal` {target, action, reason, success}.
- **Budget** — max 3 restarts per service per 10 minutes; exhausted
  budget emits `Failure(OrphanedProcess)` and stops trying — a
  crash-looping service must surface, not churn.
- **Verify** — post-restart port-liveness check before the action is
  recorded as successful; a failed heal emits a classified failure.
- **Lineage-honest** — heal actions carry the diagnosed origin
  (Environment/Allbright/…), never "fixed" without verification.

### 34.16 Commander override & mandatory gate

Self-healing certification is a **non-compensatory gate** under §33.7:
detection, root-cause diagnosis, safe remediation, recovery
verification, readiness re-test, infinite-retry protection, audit-trail
completeness, and zero security/risk bypass are mandatory. A failed
self-healing gate produces `SYSTEM RECOMMENDATION: NO-GO` regardless of
the weighted score.

The automated engine produces `GO / CONDITIONAL GO / NO-GO`; it does
not replace Commander authority. A Commander override must be explicit
and recorded (`COMMANDER_OVERRIDE=TRUE`, original recommendation,
reason, accepted risk, scope, duration, review condition) — and must
NOT falsify the underlying score. The system continues to display
`System Recommendation ≠ Commander Decision`.

**Allbright is not deployment-ready because it says it can heal itself —
it is ready only after demonstrating Detect → Diagnose → Decide →
Safely Act → Recover → Verify → Requalify under controlled failure. A
decorative self-healing system receives zero readiness credit.**

## 35. SYSTEM WIRING INTELLIGENCE & DEPLOYMENT CERTIFICATION

System wiring is a first-class production capability and a mandatory
readiness dimension. A component is not operational merely because its
source file exists, crate compiles, provider is configured, RPC URL
exists, wallet address exists, dashboard renders, endpoint responds or
contract address is set. **The component must be correctly wired to the
component it is intended to serve.**

```
Present ≠ Connected ≠ Correctly Wired ≠ Operationally Verified
```

The deployment system must verify all four states.

### 35.1 System Wiring Graph

Allbright maintains a machine-readable wiring graph:

```
Component → Function → Dependency → Endpoint → Provider
          → Credential/Configuration → Data Flow → Verification
```

Covering: source files, crates, modules, services, APIs, RPC and
WebSocket providers, chains, DEX adapters, pools, token/pair
registries, route engine, flash-loan providers, contracts, wallets,
signers, builders/relays, simulation, strategies, risk engine,
self-diagnosis, self-healing, telemetry, KPI pipelines, dashboard,
Commander/governance, configuration and research services.

### 35.2 Automatic wiring discovery

Commander is never required to manually enumerate wiring. The expected
graph is derived from: approved architecture, module/agent registries,
dependency manifests, source imports, configuration schemas,
environment variables, endpoint/provider/contract/wallet/RPC/strategy
registries, dashboard data requirements, KPI lineage and execution
requirements. The result is the authoritative wiring inventory.

### 35.3 Wiring state model

| State | Meaning |
|---|---|
| DISCOVERED | required connection identified |
| CONFIGURED | required parameters exist |
| CONNECTED | endpoint/provider responds |
| VERIFIED | correct target and capability confirmed |
| OPERATIONAL | real business function exercised end-to-end |
| DEGRADED | works with reduced capacity |
| BROKEN | expected connection fails |
| MISSING | required connection does not exist |
| MISWIRED | connects to the wrong component/capability |
| UNVERIFIED | correctness cannot be established |
| BLOCKED | security/governance gate prevents operation |

Only `OPERATIONAL` qualifies as fully wired. The system must never
silently promote `MISSING → HEALTHY` or `UNVERIFIED → OPERATIONAL` —
unknown status stays visible.

### 35.4 The dumb-component test

A component is classified `DUMB`/`DORMANT` when it exists but has no
required connection, targets the wrong endpoint, receives or emits no
expected data, cannot execute its intended function, has an unavailable
provider or invalid credentials/config, has unconsumed output, or feeds
a metric with no verified lineage. Example: a dashboard showing a
wallet address is not wallet integration — it is operational only when
`Wallet → chain → RPC → balance/state → signing → tx → confirmation`
is exercised.

### 35.5 Mandatory dependency chains

- **Flash-loan provider**: registry → chain → contract → liquidity →
  interface → tx builder → simulator → risk gate → signer → execution
  route → post-execution verification. Presence in configuration earns
  zero credit. Distinguish discovered / configured / reachable /
  contract-verified / liquidity available / simulatable / executable /
  production-qualified.
- **RPC endpoint**: chain identity, network ID, latest block + freshness,
  required methods, WS/log/state/trace-archive capability, quota/rate
  limits, latency, error rate, provider health, data consistency — mapped
  to the workload it serves. A URL that cannot perform the required
  operation is `MISWIRED`/`UNSUITABLE`, not healthy.
- **Per chain (≤50)**: chain → RPC pool → gas config → DEX registry →
  factory/router → pool discovery → token/pair registry → simulation
  model → strategy compatibility → execution path → wallet/signer →
  risk rules → telemetry → dashboard KPIs.
- **Per DEX**: correct chain, factory, router/quoter, discovery, state
  retrieval, token metadata, fee/liquidity model, simulation, route
  engine, strategy compatibility, telemetry. Per pool: `exists → state
  accessible → liquidity measurable → pricing usable → simulation
  possible → economic evaluation possible`.
- **Dashboard metric**: UI → frontend state → backend/API → telemetry →
  producing service → market/blockchain data → timestamp. A metric
  without complete lineage is `DECORATIVE` and fails certification.
- **KPI**: source, calculation, update mechanism, timestamp, health,
  lineage, validation test — `exists → receives data → calculates →
  reflects production → independently validated`.
- **Wallet**: chain → address → balance/state → signing → nonce → gas →
  tx construction → risk approval → execution → confirmation →
  accounting.
- **Agent**: purpose, inputs, output consumers, execution authority,
  dependencies, health signal, failure behavior, telemetry, recovery
  path. A running process without this chain is `DORMANT`, not an
  operational agent.

### 35.6 Impact analysis

For every component the system answers "what depends on this?" and
"what does this depend on?" — the full downstream blast radius. `RPC
failure → state ingestion → pool state → route engine → simulation →
qualification → execution`, not merely "RPC unhealthy".

### 35.7 Wiring fault injection & self-heal

Readiness deliberately breaks selected connections (remove an RPC
endpoint, disable WS, break a flash-loan provider, disconnect wallet
data, stop pool discovery, disconnect telemetry, stop an agent,
disconnect a DEX adapter) and requires `detect → diagnose → isolate →
repair/failover → verify → requalify`. The self-healer operates on the
wiring graph: reconnect, switch endpoint, fail over provider, restart
component, refresh configuration, isolate, rebuild dependency,
downgrade capability, or escalate — then verifies the business function,
not just the connection.

### 35.8 Wiring Integrity Score — non-compensatory

System Wiring Integrity is pillar 12 of the Master Score (§33.6),
weighted 100: components discovered (10), dependencies mapped (10),
endpoint correctness (10), RPC/provider wiring (10), multi-chain
wiring (10), DEX/pool/token/pair wiring (10), flash-loan wiring (10),
wallet/execution wiring (10), dashboard/KPI lineage (5), agent wiring
(5), self-heal wiring (5), fault-injection verification (5).

**Critical wiring failures override the numerical score.** A 1045+
score with an unwired or unverified wallet→signer→execution path,
flash-loan chain, critical RPC layer, economic simulation or KPI
lineage produces `SYSTEM RECOMMENDATION: NO-GO`. After a wiring repair,
the sequence is mandatory: `Repair → Self-Diagnosis → Self-Heal
verification → End-to-End function test → KPI revalidation → Readiness
recalculation`.

### 35.9 Commander wiring view

Commander receives an auto-generated summary — components discovered,
required connections, verified, operational, degraded, missing,
miswired, unverified, critical failures, self-healed connections,
Wiring Integrity Score — with drill-down `Chain → DEX → Pool → Pair →
RPC → Provider → Strategy → Execution`. Deployment cannot be certified
from application startup alone; the engine evaluates Design Capacity +
Wiring Integrity + Data Visibility + Self-Diagnosis + Self-Healing +
Economic Function + Execution + Security + KPI Integrity.

**A component that exists but is not correctly wired is not part of the
operating system — it is dormant capacity.**

## 36. 200+ LIVE RPC INFRASTRUCTURE REQUIREMENT

**Mandatory capacity target:** Allbright SHALL maintain a discovery and
qualification universe of **more than 200 qualified, live-capable RPC
endpoints** across enabled chains — not 200 URLs stored in config. The
pipeline is a funnel, not a list:

```
2,000+ DISCOVERED CANDIDATES
  → DNS/TLS/HTTP → JSON-RPC → CHAIN-ID VALIDATION
  → METHOD CAPABILITY → LATENCY → BLOCK/STATE FRESHNESS
  → CONTROLLED CAPACITY → QUOTA/RATE-LIMIT → VISIBILITY TEST
  → 200+ QUALIFIED → PRODUCTION POOL (continuous rotation)
```

Candidate sources are continuously re-discovered, never frozen:
official chain RPCs (priority 1), established keyless infrastructure
(Pocket `https://{chain-slug}.api.pocket.network` — 60+ networks,
BlockMachine, NodeFlare, SolidRPC), directory sources (QuickRPC ~2.5k
networks, ChainIndex/ChainList), and community/decentralized providers.
**No directory entry is trusted without independent live qualification.**

### 36.1 Independence, not count

200 endpoints backed by one provider ≠ 200 independent resources. Track
`provider, infrastructure_owner, region, transport,
upstream_dependency` and maximize provider + geographic +
infrastructure diversity + chain coverage.

### 36.2 Per-chain redundancy floor

Each critical chain requires ≥5 qualified RPCs across ≥3 independent
providers with ≥2 infrastructure paths and ≥1 emergency backup, where
infrastructure exists. A chain with one working RPC is classified
`RPC_REDUNDANCY_INSUFFICIENT` — even if that RPC is fast.

### 36.3 Qualification ladder

`CANDIDATE → LIVE → CHAIN VERIFIED → METHOD VERIFIED → STATE VERIFIED →
LATENCY VERIFIED → CAPACITY VERIFIED → VISIBILITY VERIFIED → QUALIFIED`.
Only `QUALIFIED` endpoints enter the production pool. Minimum live
tests: `eth_chainId`, `net_version`, `eth_blockNumber`,
`eth_getBlockByNumber`, `eth_call`, `eth_getBalance`, `eth_getLogs`,
plus `eth_sendRawTransaction`/trace/debug/WSS/private submission where
the workload requires them.

### 36.4 Capacity rule — 90/10

`SAFE_RPS = VERIFIED_RPS × 0.90`. The 10% reserve is for bursts,
retries, failover, self-diagnosis/self-heal and emergency execution —
never ordinary workload.

### 36.5 Routing

Dynamic selection `request → chain → method → priority → capability →
health → latency → capacity → quota → best RPC`. Blind round-robin
across 200+ endpoints is forbidden.

### 36.6 Health score

Continuously updated per endpoint: latency, availability, error rate,
rate-limit frequency, block/state freshness, method coverage, capacity,
quota, visibility, provider reliability, failover performance.

### 36.7 Self-diagnosis & self-heal (integrated, not standalone)

RPC faults run through the sentinel's existing loop (§34) — no parallel
subsystem: `FAULT → DIAGNOSIS → DECISION → REMEDIATION → VERIFICATION →
RECOVERY → READINESS RECHECK`. On abnormal opportunity decline the
diagnosis order is `RPC health → block freshness → state freshness →
pool visibility → pair visibility → opportunity visibility`; the system
must always distinguish `MARKET EMPTY` from `SYSTEM BLIND`. On RPC
failure: `detect → diagnose → quarantine → select backup → redistribute
→ verify block → verify state → verify market visibility → verify
opportunity visibility`. A successful HTTP response after switching
providers does NOT prove recovery — business-function verification does.

### 36.8 Deployment gate

RPC readiness is NO-GO when: the qualified pool is below target;
critical chains lack redundancy; required methods unavailable; capacity
insufficient or the 10% reserve consumed by ordinary load; RPC-induced
market blindness exists; failover untested; or self-heal unverified. A
machine-readable RPC qualification report is produced before
deployment.

**Final rule:** do not build a list of 200 URLs — build an RPC
intelligence layer that discovers, qualifies, measures, routes,
replaces and heals a pool larger than 200 live endpoints.

## 37. RPC OPPORTUNITY VISIBILITY DOCTRINE

This doctrine is part of the System Wiring (§35), Self-Diagnosis/Self-Healing (§34)
and RPC Capacity (§32, §36) architecture — it is NOT a separate RPC subsystem.

### 37.1 Core requirement

Every AllBright RPC endpoint SHALL be evaluated not merely for connectivity, but
for its ability to provide the blockchain information required to:

- discover arbitrage opportunities;
- detect liquidity changes;
- observe DEX state;
- identify token/pair relationships;
- obtain pool reserves/state;
- observe relevant events;
- construct accurate routes;
- simulate candidate opportunities;
- validate profitability;
- prepare execution;
- confirm execution.

**Fundamental rule:**

```
RPC ALIVE
    ≠
RPC OPERATIONAL
    ≠
RPC OPPORTUNITY-VISIBLE
```

An RPC that responds to basic JSON-RPC calls but cannot provide sufficient market
information SHALL NOT be considered fully operational for opportunity intelligence.

### 37.2 Opportunity Visibility Contract (OVC)

Every RPC must pass the OVC before it can be classified production-capable:

```
RPC → CHAIN → BLOCK → CONTRACT → DEX → POOL → TOKEN → PAIR
    → LIQUIDITY/STATE → EVENTS → ROUTE → PRICE → OPPORTUNITY
    → SIMULATION → ECONOMIC VALIDATION
```

A failure anywhere in this chain must be visible to the diagnostic system.

### 37.3 Visibility classes

Every chain/RPC combination SHALL be classified as one of:

| Class | Meaning |
|---|---|
| `V0` — DEAD | RPC unavailable |
| `V1` — CONNECTED | basic JSON-RPC connectivity works |
| `V2` — CHAIN VISIBLE | chain identity and current block available |
| `V3` — STATE VISIBLE | contracts, balances, calls and current state accessible |
| `V4` — MARKET VISIBLE | DEXs, pools, pairs, liquidity and market state accessible |
| `V5` — OPPORTUNITY VISIBLE | AllBright can discover and evaluate candidates |
| `V6` — EXECUTION VISIBLE | AllBright can obtain information needed to construct, simulate and prepare execution |

Only V5/V6 qualifies an RPC for the corresponding production workload.

### 37.4 Opportunity visibility test

For representative samples on every enabled chain, test:

`CHAIN → DEX → POOL → TOKEN → PAIR → RESERVES → PRICE → EVENT → ROUTE → PROFITABILITY → SIMULATION`

Determining: can the pool be discovered, contract state read, reserves/liquidity
read, token metadata obtained, events/logs retrieved, historical state obtained
when required, pools compared, routes constructed, candidates validated, and
simulation state supplied.

### 37.5 Measured market coverage

```
RPC Opportunity Visibility = Visible Qualified Pools    / Expected Qualified Pools
Pair Visibility            = Visible Qualified Pairs    / Expected Qualified Pairs
DEX Visibility             = Visible Qualified DEXs     / Expected Qualified DEXs
Opportunity Visibility     = Observable Opportunity Universe / Expected Universe
```

These are not RPC uptime metrics.

### 37.6 Blindness detection

AllBright must distinguish `NO_MARKET_OPPORTUNITY` from `NO_DETECTED_OPPORTUNITY`
and from `SYSTEM_BLINDNESS`. Possible blindness causes include: RPC unavailable,
timeout, rate limiting, quota exhaustion, stale block/state, missing or restricted
`eth_getLogs`, missing historical/trace/debug capability, WebSocket failure,
provider divergence, incomplete pool/token/pair discovery, DEX contract
invisibility, routing failure, chain misconfiguration, wrong chain ID, wrong
contract address, provider synchronization failure. The opportunity engine
SHALL NOT interpret these as evidence that the market contains no arbitrage.

### 37.7 RPC capability matrix

Every RPC must declare and verify: `chain_id`, `latest_block`,
`block_timestamp`, `eth_call`, `eth_getBalance`, `eth_getCode`,
`eth_getStorageAt`, `eth_getLogs`, `historical_state`, `archive_state`, `trace`,
`debug`, `WebSocket`, `transaction_submission`, `private_submission` — each
classified `AVAILABLE` / `PARTIAL` / `UNAVAILABLE` / `UNKNOWN`. No capability may
be marked AVAILABLE merely because the provider advertises it.

### 37.8 Multi-RPC visibility quorum

For critical market state, cross-validate across multiple RPCs:

```
RPC-A + RPC-B + RPC-C → CROSS-VALIDATION → TRUSTED STATE
```

Material disagreement MUST raise `PROVIDER_DIVERGENCE`. The system must not
silently choose one inconsistent provider and report its state as market truth.

### 37.9 Request classification & visibility-aware routing

Every request is classified: `BLOCK`, `STATE`, `POOL`, `PAIR`, `TOKEN`, `EVENT`,
`DISCOVERY`, `ROUTE`, `SIMULATION`, `EXECUTION`, `DIAGNOSTIC`, `DASHBOARD`. The
router selects an RPC by required capability:

`REQUEST → REQUIRED CAPABILITY → CHAIN → MARKET DATA TYPE → RPC CAPABILITY → RPC HEALTH → RPC VISIBILITY SCORE → LATENCY → CAPACITY → QUOTA → BEST QUALIFIED ENDPOINT`

- `eth_getLogs` must not route to endpoints known to restrict log queries.
- Historical simulation must not use a non-archive endpoint.
- Execution submission must not use a public read-only endpoint.
- The fastest RPC is not necessarily the correct RPC; a slower endpoint with
  complete market state may be superior to a fast but blind one.

### 37.10 Opportunity Visibility Score (OVS)

Each RPC carries a dynamic score from: chain visibility, block freshness, state
freshness, DEX coverage, pool coverage, pair coverage, event visibility,
historical-state capability, response consistency, opportunity observation
success, simulation input completeness. Weightings are configurable and
empirically calibrated.

### 37.11 Self-diagnosis integration

```
OPPORTUNITY DROP → MARKET ACTIVITY CHECK → RPC HEALTH → BLOCK FRESHNESS
 → STATE FRESHNESS → DEX VISIBILITY → POOL VISIBILITY → PAIR VISIBILITY
 → EVENT VISIBILITY → ROUTE VISIBILITY → SIMULATION INPUT INTEGRITY
 → OPPORTUNITY ENGINE
```

This prevents incorrectly concluding arbitrage opportunities have disappeared.

### 37.12 Required diagnostic codes

`RPC_OPPORTUNITY_BLIND`, `RPC_POOL_VISIBILITY_LOSS`, `RPC_PAIR_VISIBILITY_LOSS`,
`RPC_DEX_VISIBILITY_LOSS`, `RPC_EVENT_VISIBILITY_LOSS`,
`RPC_STATE_VISIBILITY_LOSS`, `RPC_HISTORICAL_VISIBILITY_LOSS`,
`RPC_SIMULATION_DATA_BLIND`, `RPC_PROVIDER_DIVERGENCE`,
`RPC_STALE_MARKET_STATE`, `RPC_QUOTA_VISIBILITY_LOSS`,
`RPC_METHOD_VISIBILITY_FAILURE`, `RPC_CHAIN_VISIBILITY_FAILURE` — appearing in
system diagnostics, telemetry and deployment readiness.

### 37.13 Self-healing integration & success criteria

RPC visibility problems are remediated by the existing self-healing system:

`RPC_OPPORTUNITY_BLIND → identify chain → identify DEX/pool/pair → identify failed capability → select qualified alternative → redistribute → refresh state → retest visibility → retest opportunity discovery → retest simulation → restore KPI lineage`

Switching RPCs is NOT sufficient. Healing succeeds only when connectivity, block
freshness, state freshness, DEX/pool/pair/event visibility, opportunity
visibility and simulation input integrity are all restored; the original failure
closes only after verification.

### 37.14 RPC failure priority

Under constrained capacity: P1 execution/final validation → P2 active opportunity
validation → P3 pool/pair state for live opportunities → P4 opportunity discovery
→ P5 simulation/research → P6 dashboard/background. Dashboard traffic SHALL never
consume capacity needed to maintain opportunity visibility.

### 37.15 Deployment readiness gate

RPC readiness is NO-GO if: required market data cannot be observed; critical
pools/pairs invisible; DEX state unverifiable; required events unobservable;
simulation state unobtainable; RPC disagreement unresolvable; capacity
insufficient; 10% reserve unavailable; failover untested; self-healing cannot
restore opportunity visibility; or opportunity KPIs cannot be traced to reliable
blockchain data.

### 37.16 Commander override

The Commander may override GO/NO-GO, but `SYSTEM RECOMMENDATION = NO-GO /
COMMANDER DECISION = OVERRIDE` must remain visible. Commander authority cannot
erase: RPC blindness, failed visibility tests, self-healing failures, provider
divergence, insufficient capacity, missing market data or failed deployment
gates. Commander authority is governance authority, not data falsification.

### 37.17 Final principle

An RPC is operational only when it can reliably make the relevant blockchain
market visible to the opportunity engine. The test is not "can AllBright reach
the RPC?" — it is "can AllBright see, understand, validate and simulate the
arbitrage opportunity universe through the RPC?"

## 38. DASHBOARD IMPLEMENTATION SPECIFICATION (MANDATORY)

This section is binding implementation authority for `allbright-dashboard`,
not optional UI guidance. It extends the Master Build; it does not replace it.

### 38.1 Implementation authority rules

Any agent modifying the dashboard MUST:

- audit the current Master Build before implementation;
- reuse existing KPI, strategy, deployment, infrastructure, wallet,
  governance and telemetry definitions;
- never create duplicate KPI engines, strategy registries, data services
  or monitoring systems;
- never use mock, fabricated, placeholder or hard-coded operational values;
- preserve existing working functionality unless the Master Build
  explicitly requires a change;
- verify complete data lineage for every displayed operational metric.

### 38.2 Page structure

Sidebar: Expand/Collapse (icon+label expanded; icon+active-indicator+tooltip
collapsed); AllBright logo upper-left; Kill Switch pinned at bottom.
Sidebar page names and order are fixed — **Profit Metrics** is the
default landing page:

```
PROFIT METRICS → BLOCKCHAIN STREAM → STRATEGY → DEPLOYMENT
→ INFRASTRUCTURE → REPORTS → COMPETITIVE ANALYSIS → WALLET
```

Global header: logo, `ALLBRIGHT`, page title (left); Profit Pulse, currency
selector, period indicator, refresh selector, system status, day/night
theme (right).

**Profit Pulse** must clearly distinguish realized profit, wallet balance
and unrealized value; it must never present wallet balance as profit
without lineage. Periods: Today / 7D / 30D / Lifetime. Currencies: USD,
ETH, top-10 supported. Tooltip explains exactly what it represents.

### 38.3 Universal table engine

Every structured-data page uses the common table engine: ascending/
descending sort, functional column groups, filtering, search, pagination,
page-size selection (10/25/50/100), category & subcategory expand/collapse,
Expand All / Collapse All, column visibility, sticky headers, horizontal
scroll, freshness + stale indicators, tooltips, drill-down. Large datasets
use backend/server-side filtering, sorting and pagination. Sorting uses
functional headings, not database fields.

### 38.4 Time-dimension standard

Common time component: minutes (1/5/15/30/60m), hours (1/2/4/8/12/24h),
days (1/2/7/14/30D), months (1/3/6/12M), plus Today, All Time,
Since Deployment, Custom. Each page exposes only meaningful ranges and
remembers its own last-used range — changing Profit's range must not
perturb Infrastructure or Reports.

### 38.5 Profit page — Page 1 and primary analytical page

The Profit page answers: *how much was made, during what period, on which
chain, through which strategy variant, at what economic quality.*

**Control bar** (immediately above the visualization/table):

- **Time Range** — full common set incl. Since Deployment and Custom; the
  selected range is sent to the backend query layer — the frontend must
  NOT merely hide already-loaded rows.
- **Chain selector** — ALL CHAINS plus every production chain from the
  authoritative chain registry; no invented/hard-coded chains.
- **Strategy Variant selector** — ALL STRATEGY VARIANTS plus each of the
  12 Master Build variants, read BY NAME from the strategy registry
  (§7); agents MUST NOT invent replacement names or duplicate strategy
  definitions.
- **Presentation selector** — TABLE / CHART / BAR. Exactly one
  visualization at a time; all three are presentation modes of ONE
  authoritative P&L dataset, never separate data implementations.

**Summary metrics** (computed from selected Time+Chain+Variant context):
Gross Profit, Total Costs, Net Profit, Profit Margin, ROI, Successful
Trades, Failed/Aborted Trades, Execution Win Rate, Avg Profit/Trade,
Lifetime Profit.

**P&L table** functional groups — Identity (Timestamp, Chain, Strategy
Variant, Strategy, Opportunity ID, Transaction ID) | Revenue (Gross
Trading, Flash Loan, Other Realized) | Costs (Gas, Flash Loan Fee,
Slippage, RPC/Infra, Builder/Relay, Other) | Economic Result (Expected,
Realized, Net, ROI, Margin, Variance) | Execution (Submitted, Confirmed,
Reverted, Failed, Win Rate) | Performance (Detection/Decision/Simulation/
Execution latency) | Data Quality (Market Data Status, RPC Visibility,
State Freshness, Profit Confidence). Final TOTAL row reconciles all
aggregate columns from the selected dataset — never hard-coded.

**Synchronization**: Time, Chain, Strategy Variant and Currency changes
apply to table and visualization simultaneously; the chart/bar must never
display data outside the visible table context.

### 38.6 Strategy page

Variant selector (All + 12 registry variants), time selector, comparison
table with groups: Opportunity | Economic | Execution | Latency | Risk |
Adaptation | Profitability — covering detected/qualified/executed counts,
win rate, gross/net/expected/realized profit, variance, gas/flash-loan/
slippage costs, missed opportunities, coverage, latencies, health and
adaptation state.

### 38.7 Deployment page

Time-filtered register: timestamp, deployment ID, version, git commit,
build ID, environment, mode, chain scope, config/strategy/RPC-config/
wallet-config versions, readiness score, critical gate result, Commander
decision, system recommendation, deployment and verification results.
Pipeline stages — Pre-Flight → Config Validation → Wiring Validation →
RPC/Visibility Validation → Simulation → Risk/Security Gates → Readiness
Certification → Commander Decision → Deployment → Post-Deployment
Verification → Monitoring — each recording status, start, end, duration,
evidence, errors, warnings.

### 38.8 Infrastructure page

Time-filtered trends for: RPC (total/live/qualified/production/degraded/
quarantined counts, provider diversity, per-chain counts, current RPS,
verified capacity, safe-90% capacity, 10% reserve, headroom, p50/p95/p99
latency, error/timeout rate, rate-limit events, block lag, state freshness,
opportunity visibility, blindness); Visibility (chain/DEX/pool/pair/event/
opportunity/simulation-data); Self-Diagnosis (active/critical/RPC
diagnoses, visibility failures, provider divergence, stale-state
incidents); Self-Healing (active recoveries, success/failure, recovery
time, quarantined/restored endpoints).

### 38.9 Reports page

Time ranges 1H/6H/24H/7D/30D/90D/Custom/Since Deployment across
categories: Profit (gross/net/costs/margin/ROI by chain and variant),
Opportunity (detected/qualified/executed/missed/filtered/RPC-blind/
expired/simulation-failed), Strategy (performance/contribution/win-rate/
adaptation), Execution (transactions/confirmations/reverts/failure-rate/
latency), Infrastructure (availability/capacity/90%-utilization/reserve/
visibility/self-healing), Risk/Security (alerts/blocks/circuit-breakers/
security and governance events).

### 38.10 Wallet page — non-custodial

MetaMask provider detection where supported: list accounts, user selects,
read address/chain/balance, populate wallet table. NEVER request or store
seed phrases or private keys. Table: Wallet, Type, Account, Address,
Chain, Native Balance, USD Value, Last Activity, Connection Status,
Auto-Transfer Status, Threshold, Last Transfer; final TOTAL BALANCE row
reconciles visible balances in the selected currency. Time selector
applies to transfer history/activity/balance history.

**Auto-transfer controls:** ON/OFF, minimum threshold, current balance,
eligible amount, last transfer, status. Manual transfer stays separate.
Opening the Wallet page must NEVER initiate a transfer.

### 38.11 Global kill switch & system status

Kill Switch (sidebar bottom): RUNNING / STOPPING / HALTED / RECOVERING;
activation ⇒ EXECUTION HALTED via the Master Build halt mechanism — stops
NEW execution activity, does not imply cancellation of already-submitted
transactions; monitoring/diagnostics stay active.

Header status: HEALTHY / DEGRADED / SAFE-HALT with tooltip summarizing
opportunity visibility, RPC health, execution state, wallet state,
self-diagnosis and self-healing.

### 38.12 Data lineage & no-mock-data

Every metric carries lineage `Dashboard → Frontend State → API/Event
Stream → Backend Service → Telemetry/Database → Producing Component →
Blockchain/Market Data`. Unverified data is marked `UNVERIFIED` —
never silently authoritative. No fake profit/balances/RPC health/
deployment status/strategy performance or static values presented as
live; unavailable data shows `DATA UNAVAILABLE`/`NOT VERIFIED` with reason.

RPC display must distinguish `RPC AVAILABLE` / `RPC HEALTHY` /
`RPC MARKET-VISIBLE` / `RPC OPPORTUNITY-VISIBLE` (§37.3).

Diagnoses display: Issue ID, Timestamp, Severity, Affected Component,
Chain, Root Cause, Confidence, Current State, Remediation State — covering
RPC blindness, pool/pair visibility loss, stale state, provider
divergence, quota exhaustion, missing market data, simulation-data
failure, wallet connectivity failure, deployment wiring failure.

Healing records display: Incident ID, Detection Time, Diagnosis,
Remediation, Endpoint/Provider Changed, Affected Chain, Recovery Time,
Verification Result, Final State — state machine `DETECTED → DIAGNOSING →
REMEDIATING → VERIFYING → HEALED / FAILED / ESCALATED`. A remediation
command completing is NOT proof of healing; verification is required.

### 38.13 Branding & footer

- **Logo:** the word `AllBright` only — minimalist, light, elegant,
  premium; blue-and-white palette; no shields, coins, charts, lightning
  bolts or conventional crypto imagery; recognizable at small size. The
  logo appears top-left of the header and centered in the footer.
- **Motto** (displayed separately from the logo, never inside it):
  `Autonomous Wealth Engine. 24/7. Like No One.`
- **Footer identity:** `AllBright DeFi Software Engineering Ltd. © 2026`
  — no obsolete product or company names may appear.

### 38.14 Acceptance tests (mandatory)

Profit: is Page 1; real data; all minute/hour/day/month/custom ranges;
chain selector with all-chain reconciliation; all 12 variants from the
registry; TABLE/CHART/BAR each work; one visualization at a time; table
and viz share identical filters; totals reconcile.
Tables: sort asc/desc, column groups, category+subcategory expand/
collapse, pagination, page-size, search/filter, column visibility.
Strategy: all+individual variants, time filtering, comparison.
Deployment: timestamp filtering, register, identifiers, pipeline,
evidence, verification.
Infrastructure: time filtering, RPC monitoring, capacity, 90/10 model,
visibility, self-diagnosis, self-healing.
Reports: time selector, historical analytics, strategy+chain attribution.
Wallet: MetaMask detection, account selection, address/chain/balance,
total, threshold, manual transfer, activity time filtering.
Global: Profit Pulse, currency+refresh selectors, tooltips, day/night,
logo, footer, Kill Switch, system status, collapsible sidebar.

### 38.15 Blockchain Stream page — real-time execution battlefield

A dedicated **BLOCKCHAIN STREAM** page visualizes the full opportunity
lifecycle in real time: `MARKET → DETECTED → VALIDATED → ECONOMIC ANALYSIS
→ STRATEGY → SIMULATION → RISK/DEFENSE → EXECUTION DECISION → BUILD →
SIGN → SUBMIT → MEMPOOL/RELAY → INCLUSION → CONFIRMATION → SETTLEMENT →
PROFIT → SMART WALLET → COMMANDER WALLET`.

Mandatory properties:

- **Real events only.** Every stage transition originates from a real
  system event (Opportunity/Rejected/ExecRequest/ExecResult/Failure/Heal
  and settlement evidence). Where evidence does not exist, the stage
  renders `UNVERIFIED`/`DATA UNAVAILABLE` — never fabricated activity,
  never decorative animation.
- **Persistent opportunity identity** (`OPP-YYYYMMDD-NNNNNNN`) carried
  through the entire lifecycle and drill-down post-mortem.
- **Stage illumination** follows actual events; a completed remediation
  command is not proof of the stage succeeding.
- **Defense strip** shows live gate states: green = check passed,
  yellow = degraded/uncertain, red = failed, grey = unverified —
  never green-by-default.
- **Failures are first-class**: rejected, expired, simulation-failed,
  risk-blocked, reverted and RPC-blind opportunities are shown alongside
  successes — what AllBright defended against is evidence of intelligence.
- **Money flow continues past confirmation**: settlement verification →
  smart-wallet balance → Commander-wallet transfer status
  (`PENDING/SUBMITTED/CONFIRMED/FAILED/RECONCILED` or `PROFIT HELD —
  MANUAL TRANSFER REQUIRED` when auto-transfer is off). Never imply a
  transfer that did not happen.
- **Funnel visualization** (detected→qualified→simulated→risk-cleared→
  executed→successful) uses real counts only.
- **Single source of truth**: consumes the same authoritative events as
  Profit/Strategy/Infrastructure — no parallel calculations, no second
  event system.
- **Degradation honesty**: on feed interruption, show `STREAM DEGRADED —
  RECONNECTING`, preserve last verified state with timestamps, reconcile
  recovered events on reconnect; never fabricate continuity.
- **Institutional visual language**: restrained blue/white, subtle state
  transitions — no arcade/gambling effects.
- **Capital protection summary**: evaluated, capital at risk, capital
  protected, threats detected, trades blocked, failed/successful trades,
  net realized, profit transferred.

### 38.16 Competitive Analysis page

Sidebar position: between Reports and Wallet. Backed by `/api/competitive`
and the versioned `benchmark_register.json` (§27.1). Renders the
AllBright-measured baseline (live: chains, verified pools, qualified RPCs,
funnel counts, realized P&L) as headline cards, then a sortable KPI
comparison table — rows are the §21/§27 comparison axes (mapped to the
five dimensions), columns are AllBright ★ plus the registered platforms.
Every cell is a rating badge with confidence + evidence tooltip;
`UNVERIFIED`/`NOT DISCLOSED` where evidence is absent. The register file
carries citations, review dates and scope; the dashboard never presents
public marketing claims as measured benchmarks. Page states must degrade
honestly (`DATA UNAVAILABLE` if the register is missing).

### 38.17 Dashboard definition of done

The dashboard is complete only when it is: master-build aligned,
data-wired, profit-centric, time-filterable, strategy-variant-aware,
chain-aware, table-sortable, category-expandable, paginated,
chart/bar-selectable, real-time/refreshable, RPC-visibility-aware,
self-diagnosis-aware, self-healing-aware, wallet-wired,
competitive-benchmark-aware, deployment-aware, Commander-ready, and free
of mock data — delivering
`PROFIT METRICS → BLOCKCHAIN STREAM → STRATEGY → DEPLOYMENT →
INFRASTRUCTURE → REPORTS → COMPETITIVE ANALYSIS → WALLET` with
`Time → Chain → Strategy Variant → P&L → Visualization → Transaction
Evidence` navigable without leaving the Profit page.

## 39. MANDATORY GOVERNANCE, ARCHITECTURAL INTEGRITY & END-TO-END INTEGRATION

**Commander directive — mandatory.** This section governs *how* every
agent works on Allbright, not merely *what* it builds. It supersedes any
inferred, remembered, or generic coding-agent workflow. Violations are
architectural defects, not style issues.

### 39.1 Governing documents are binding

Before any work session, read the **actual current repository versions**
of `AGENTS.md`, `.devin/skills/allbright-engineering/SKILL.md`,
`MASTER_BUILD_GUIDE.md`, and applicable integration specs and audit
reports. Do not substitute remembered instructions, summaries, or
assumptions. Authority order: **MASTER_BUILD_GUIDE governs product scope
and approved architecture → AGENTS.md governs agent governance and
enforcement → SKILL.md governs execution methodology.** If a document is
missing, outdated, contradictory, or inaccessible, report the precise
issue — never invent its contents. Never weaken or silently reinterpret
a governing requirement.

### 39.2 Baseline before change

No substantive change begins without a current-state audit of the
**actual implementation** — not filenames or documentation claims.
The audit must identify: implemented capabilities, partial
implementations, disconnected components, missing integrations, broken
imports/routes/interfaces/event flows, duplicate subsystems, mock or
unverified data in production paths, failing tests, configuration
drift, and security or economic-integrity risks — each with supporting
evidence.

### 39.3 The integrated economic engine

Allbright is one pipeline, and every stage must have a named owner, a
defined input/output, a connecting interface or event, an output
validator, a failure path, and evidence the connection works:

```
MARKET OBSERVATION → DATA INGESTION AND VALIDATION
→ OPPORTUNITY DISCOVERY → ECONOMIC QUALIFICATION
→ STRATEGY SELECTION → SIMULATION → RISK AND SECURITY CONTROLS
→ EXECUTION AUTHORIZATION → TRANSACTION CONSTRUCTION AND SIGNING
→ SUBMISSION AND BLOCKCHAIN CONFIRMATION → SETTLEMENT
→ REALIZED-PROFIT RECONCILIATION → SMART-WALLET ACCOUNTING
→ AUTHORIZED COMMANDER-WALLET TRANSFER → LEARNING AND OPTIMIZATION
```

A feature is not integrated merely because its screen, API, module, or
function exists. A stage with no downstream connection, or an upstream
stage that never produces its input, is a documented gap — not a
completed feature.

### 39.4 Requirement traceability matrix

Approved Master Build requirements are tracked with explicit statuses:
`NOT INSPECTED / MISSING / PRESENT / CONNECTED / WIRED / VERIFIED /
OPERATIONAL / ECONOMICALLY VALIDATED`. Status may only be promoted on
evidence. Every requirement is accounted for even when deferred or
blocked. Each entry records: requirement reference, intended outcome,
owning component, dependencies, integration points, current status,
gap, planned correction, acceptance test, and verification evidence.

### 39.5 No superficial completion — prohibited acts

- Dashboard page without authoritative data wiring.
- Control without its implemented and tested action.
- Success status displayed for an unverified operation.
- A second subsystem when an existing component should be repaired or
  extended.
- A simpler replacement that breaks architectural compatibility.
- Mock data anywhere in a production path.
- Claims of execution, settlement, profit, or wallet transfer without
  evidence.
- Treating a successful build as proof of end-to-end correctness.
- Marking unfinished integration complete because the UI looks polished.

Missing integration is exposed and documented, then implemented only
through the approved architecture.

### 39.6 Approval gates

Until the current-state audit, traceability matrix, risks, and proposed
plan are presented and approved, make no substantive code changes.
The plan states: what changes and why; which governing requirements it
satisfies; which components are reused; which interfaces change; risks
introduced; regression prevention; how success is measured. Implement
the smallest coherent change satisfying the requirement. If
implementation reveals a material architectural conflict or scope
change, stop the affected work and seek a decision — never expand
silently.

### 39.7 Mandatory verification along the whole path

Verification follows the entire affected path, not the modified
component: static analysis, unit, integration, interface-compatibility,
event/data-flow, failure-path, end-to-end, regression, runtime-health,
deployment, and economic reconciliation checks as applicable. On-chain
paths distinguish **simulation / submission / inclusion / confirmation /
verified settlement** as separate states. Financial paths distinguish
**expected profit / realized net profit / smart-wallet balance change /
confirmed Commander-wallet transfer**. Displayed metrics must have a
traceable source and a freshness indicator. Never claim a test that was
not run.

### 39.8 Change control order

Before adding a module, service, registry, event stream, or dependency,
search for an existing implementation. Preference order: **(1) reuse,
(2) repair, (3) extend the interface compatibly, (4) refactor with
approval, (5) introduce new only when a documented gap requires it.**
Preserve unrelated working behavior. Never delete, overwrite, or bypass
security controls to make a test pass. Record material architecture,
schema, API, dependency, and configuration changes.

### 39.9 Reporting standard

Correction is demonstrated through deliverables, not promises.
Pre-implementation report: documents inspected, baseline, mapped
requirements, gaps, risks, proposed changes, approval required.
Post-implementation report: requirements completed, components and
interfaces changed, tests actually run with results, end-to-end behavior
verified, security/failure paths checked, deployment status,
remaining defects, evidence references. Concise, specific, auditable.

### 39.10 Definition of completion

A task is complete only when: the approved requirement is satisfied;
the implementation matches governing architecture; dependencies and
integration points are connected; relevant tests pass; end-to-end
behavior is verified to the extent claimed; security and failure
behavior checked; no unaddressed regressions; telemetry supports the
claim; traceability updated; limitations disclosed. Otherwise report
**PARTIAL, BLOCKED, or FAILED** — never silent completion.

### 39.11 Standing directive

> **Do not optimize for the appearance of progress. Optimize for
> verified architectural integrity, integrated capability, security,
> economic correctness, and measurable results.**

## 40. FINAL ENGINEERING PRINCIPLE

Allbright is not a collection of bots. It is a coordinated arbitrage intelligence and execution platform:

```
DATA → SIGNAL → STRATEGY → SIMULATION → OPTIMIZATION → RISK
     → EXECUTION → OUTCOME → CHALLENGE → CALIBRATION → LEARNING ↺
```

Every agent must preserve this closed loop.

**See more opportunities. Understand them more accurately. Choose better. React faster. Execute more reliably. Challenge continuously. Learn from what was missed. Expand the map. Prove what was achieved.**

**Measure first. Optimize second. Challenge continuously. Execute last.**

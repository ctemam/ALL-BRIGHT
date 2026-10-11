# LEGACY BLUEPRINT GAP REGISTER — ARCHIVED 2026-10-11

> Superseded by the unified gap register in HANDOFF.md §7 (U1–U28).
> Retained for fix-provenance: code comments citing GAP-## refer to this archive.

## APPENDIX B — BLUEPRINT GAP REGISTER (audit 2026-10-10)

Gaps identified by comparing AllBright against the proactive-flash-loan
reference blueprint (mempool ingestion → revm simulation → atomic
bundling → on-chain execution → validator incentive). Each gap is
tracked to closure; statuses update only on measured evidence.

| ID | Gap | Evidence | Economic cost | Closure plan |
|---|---|---|---|---|
| GAP-1 | No EVM-level simulation — math-only sim cannot see reverts | `allbright-simulation` = `amm.rs` math; no `revm` in workspace | 13,028 execs, 0 inclusions; `CamelotPair: K` bait class | `eth_simulateV1` live on all 5 active chains (Nodit: Arb/BNB/Base/OP/Poly; Base public Flashblocks endpoint incl. pre-confirmed state); `eth_call` dry-run gate fallback; anvil fork harness (`alloy::node_bindings`) replays recorded `Pair:K` reverts as the §28 blindness test; revm/AlloyDB hot path roadmap |
| GAP-2 | Shallow pool registry — depth-blind on base/avax | `pools.json` lacks aerodrome/uniswap-v3/pharaoh/pancake-v3 deep pools; no depth fields | Challenger counterfactuals cap at ~$0.03 net — dust-profit ceiling | Depth-aware `fetch_pools.py` re-run; on-chain verify; provenance kept |
| GAP-3 | Block-cadence ingestion only — zero `eth_subscribe` in codebase | `evm_stream.rs`: `newHeads` + Multicall poll; no Swap-log subs, no pending tx (BSC has a public mempool — unused) | State staleness vs event-driven competitors; P05 has no real trigger feed | WSS `Swap`/`Sync`/`PoolSwap` log subscriptions → pool refresh; BSC: `newPendingTransactions` or **48Club Puissant `newPendingAuctions`/`newPendingAuctionsWithState`** (pre-simulated state-change feed + `eth_sendBundle` `backrunTarget` — prebuilt BSC backrun infra) or bloXroute `pendingTxs`/`arbOnlyMEV` |
| GAP-4 | Flashbots submit is a stub | `relay.rs:319` returns literal `0xflashbots-pending` — no signed tx | Fake-hash landmine if triggered | Implement real `eth_sendBundle` (Eth only) or fail-closed |
| GAP-5 | Dead flash capacity | Contract supports `UNIV3_FLASH`+`ERC3156`; `flash_providers()` never emits them | Two zero-fee providers unused | Add verified UniV3-flash/ERC-3156 candidates per chain |

**Expansion (industry-research audit 2026-10-10, 4 rounds):**

| ID | Gap | Evidence | Economic cost | Closure plan |
|---|---|---|---|---|
| GAP-6 | Bundler estimation used as the effective sim gate | 13,028 execs, 100% reverted at Pimlico `eth_estimateUserOperationGas` (`CamelotPair: K`/`UniswapV2: K`) | Submission spam, zero inclusion | `eth_simulateV1`/`eth_call` gate on EVERY submission path incl. the UserOp payload — bundler sim must be the second gate, never the first |
| GAP-7 | Single-fee-class flash sourcing | `flash_providers()` emits Aave (0.05%)/DODO; Balancer V2 0% exists on all 5 active chains | ~0.05% margin surrendered per trade | Balancer-first ordering; FlashRouter-pattern cheapest-provider routing |
| GAP-8 | Hot EOA custody — raw `PRIVATE_KEY` in ENV with unscoped authority | Unrestricted signing on host (security register G2) | Total-capital compromise surface | Defense-in-depth: **Kernel v3 Permissions/session keys** (ZeroDev) — on-chain scoped signer: call-policy → executor `execute`/`sweep` only, gas cap, rate limit; key can't drain even if fully compromised — plus Turnkey enclave custody of the scoped key |
| GAP-9 | Custom deployer, no deterministic deployment | `deploy-executor.exe` reimplements deploy/verify/resume | Address drift; no in-path verification | `forge script` + CreateX CREATE2 (`0xba5E…a5Ed`), `--verify --multi` |
| GAP-10 | Hand-rolled RPC failover + manual endpoint sourcing | Custom rotation; `arb` chain 0 qualified → capacity RED; 79/365 qualified | Chain-level blindness | alloy `FallbackLayer`/`ThrottleLayer`/`RetryBackoffLayer`; scout ingests `chainlist.org/rpcs.json` (DefiLlama, daily) |
| GAP-11 | Vector A = Binance only | `cex_stream.rs` single venue, hand-rolled | Predictive edge narrow | `ccxt-rust` unified multi-CEX WS (vet maturity first) |
| GAP-12 | No token-risk pre-screening | P06/long-tail pools promoted w/o honeypot/tax/owner checks | Toxic-token execution exposure | GoPlus Security API — free, covers all 10 chains (`gplus` crate unofficial — vet) |
| GAP-13 | Lane-position blindness (Arb Timeboost) | FCFS submissions vs express lane; 3 entities win 99.74% of auctions; ~30% boosted txs revert | Structural front-run of detected opps | Honest benchmark axis note; FCFS-viable strategies only until lane economics justify |
| GAP-14 | Peer metrics self-measured only | `peer_live` null; no external competitor feed | Benchmark weak on live rivals | EigenPhi MEVScan leaderboard API (1d/7d bot PnL) + relay Data API refresh |
| GAP-15 | PM2 masking + no external alerting | `rpc-scout` ↺7 shown `online`; `.bat` launcher | Faults hidden until cascade | NSSM/WinSW services + OpenZeppelin Monitor (OSS) alert channels |
| GAP-16 | Hand-rolled ENV parser | split-on-`=` parsing, no validation/layering | Silent misconfig risk | `figment`/`dotenvy` layered config |
| GAP-17 | No adaptive capacity control — worker/probe sizing is static | `evm_stream.rs` shards fixed at boot (`rpcs/4, clamp 1..3`, never rescales); scout `CONCURRENCY` static (24) — PM2 killed it on commit-charge spikes; `/api/capacity` measures but nothing consumes it | Over-provision → kill loops; under-provision → blind spots; no closed loop | Prebuilt limiters: `tower_resilience_adaptive` (AIMD/Vegas/Gradient2) or `throttle-net` `AdaptiveLimiter` on probe+multicall paths; rescale shards on registry change; capacity endpoint feeds the controller |
| GAP-18 | Hand-rolled AA plumbing where alloy ships typed primitives; ALL execution is single-bundler AA — no EOA path exists | `aa.rs` hand-encodes `nonce_calldata`/`kernel_execute_calldata`, polls vendor-specific `pimlico_getUserOperationStatus`; `relay::submit` routes every chain through `submit_via_aa` — a Pimlico outage = total execution outage | Vendor lock; missing receipt fields (actualGasUsed, logs); single point of failure | alloy `Erc4337Api` (`send_user_operation`/`get_user_operation_receipt`/`estimate_user_operation_gas`, EP v0.6+v0.7) — zero new deps; Rundler (Rust, Alchemy) as bundler fallback; silius primitives watchlist |
| GAP-22 | No profit-token → USD valuation — doubles as an execution-coverage gate | `Executed` events carry token amount, not USD; `relay.rs:379` refuses any `token_in` outside `stable_addr()` — **WETH/BNB-settled routes (the dominant L2 profit class) are unexecutable** | Settlement rows unusable + an entire profit class refused | On-chain pool-quote pricing of the route's input token vs the chain's `stable_addr()`-verified stable set → USD conversion for both `minProfit` and ledger; DefiLlama `coins.llama.fi` cross-check |
| GAP-23 | No industry-standard telemetry export — bespoke JSON/dashboard only | `tracing` text logs + BSONL journal + custom UI; no metrics endpoint, no alerting channel — SLOs unbuildable | Ops blind outside the dashboard; no Prometheus/Grafana/alertmanager path | `opentelemetry` + `opentelemetry-otlp` (0.31, OTLP→Prometheus ≥2.47; `opentelemetry-prometheus` is deprecated/vulnerable — do not use) + `tracing-opentelemetry` bridge on existing spans |
| GAP-24 | Fee estimation vendor-specific/legacy | AA path prices gas via `pimlico_getUserOperationGasPrice`; challenger uses raw `eth_gasPrice` (legacy semantics on 1559 chains — overestimates) | Wrong fee models → margin error + submission failures | alloy `Eip1559Estimator`/provider fillers for chain-aware fees; keep bundler gas-price call as userop compat but cross-check vs `eth_maxPriorityFeePerGas` |
| GAP-25 | Route space capped at brute-force ≤3-hop — no graph structure | `solver/arb.rs`: nested O(n²)/O(n³) loops over the pool list (`two_hop`/`three_hop`); no CSR graph, no Bellman-Ford, >3-hop cycles invisible | Opportunity coverage ceiling + quadratic→cubic CPU on registry growth | CSR-packed pool graph + log-space Bellman-Ford cycle finder (dex-optimizer pattern — reference, not dep); bounded k-hop with prune metrics journaled |
| GAP-26 | Settlement credited on first inclusion — no finality check | `user_op_outcome` returns on Pimlico `included` status; zero reorg/finality handling in executor | Included-then-reorged op = phantom realized profit + phantom ledger row | `eth_getBlockByNumber("finalized")` ≥ receipt block before crediting — deterministic finality tag live on BSC (~0.65s, BEP-648) and Polygon (milestones ~2–5s); Arb/Base single-sequencer → `safe` tag or small depth; Avax → `accepted`/depth fallback |
| GAP-27 | Bundler API key leakable into logs | `aa.rs`: key travels in URL (`?apikey=`, Pimlico's documented method) but `anyhow::bail!("pimlico {method}: {err}")` — reqwest error Display embeds the full URL incl. the key | Secret in plaintext journal/log on any RPC error | Strip URL from error context (map_err closure that drops `err.url()`) or `?apikey`-less auth header if Pimlico supports it |
| GAP-28 | No cross-chain profit consolidation — wallet path dead-ends per chain | 5 active chains accrue profit in 5 silos; sweep only moves value within each chain | Treasury fragmented; Commander must manually bridge | LI.FI Intents/Composer REST API (Rust-reqwest compatible — SDK is TS but Intents is HTTP): solver-network bridging, exact-output stablecoin settlement, `/status` tracking — behind the same gated, journaled controls |
| GAP-29 | Exec-path events are best-effort — no at-least-once delivery | `bus.rs`: tokio broadcast, `publish` ignores send errors, lagged subscriber is TERMINATED (`bus.rs:74-98`) — ExecRequests silently dropped during consumer lag/restart; re-detection next block bounds but doesn't eliminate loss | Qualified opportunities silently evaporate under load spikes | Publisher-side spool+replay (embedded `fjall`/`sled` outbox) or NATS JetStream if the bus ever leaves localhost; reconnect-replay via journal scanner as interim |
| GAP-30 | Route calldata exposed pre-inclusion on BSC | `handleOps` tx enters BSC's public mempool carrying full route legs — copy-front-running reads the arb verbatim; L2s are effectively private (no public mempool) | Strategy leak + opportunity theft on our #1 chain | 48Club Puissant private bundle rail for BSC submissions (same wheel as GAP-3 closure); measure first — if copy-cat rate is zero, document and defer |
| GAP-31 | UserOp nonce race — second request inside an inclusion window collides | `sponsored_user_op` reads `EntryPoint.getNonce` per call (`aa.rs:323-331`); a pending mempool op doesn't advance the on-chain nonce → two ExecRequests in one window share a nonce → bundler rejects (AA10-class) | Second qualified opportunity in any window is destroyed by construction | In-process pending-nonce allocator (`onchain + pending_count`) or EP parallel nonce keys (key≠0 sequences run in parallel) |
| GAP-32 | No state-freshness TTL on signals/ExecRequests | `ts_ms` carried but never checked — a signal built on minutes-stale reserves still submits a UserOp (dry-run+minProfit bound loss; the wasted spend is sponsored ops + latency slot) | Sponsored-op waste + submission-slot consumption on dead signals | Bounded `signal_age_ms` check at executor (e.g. ≤2× block cadence); stale → classified Rejected with reason, never silently dropped |
| GAP-19 | Settlement accounting counts EXPECTED profit as realized | `executor/main.rs:87,112` — `settled_profit` += `req.signal.expected_profit_usd` on `included` status; no balance delta, no receipt decode | "Realized" P&L is fiction-grade; sweep trigger fed by unearned numbers | mev-inspect convention: realized = measured token delta. Contract already emits `Executed(token,input,output,profit)` + `Swept` — decode from `eth_getUserOperationReceipt` logs; pre/post `balanceOf` on the executor |
| GAP-20 | Sweep design: native-only, single-chain, amount-blind, EOA destination | `send_gasless_sweep` calls `sweepNative` only (contract also has `sweep(token,to)`); `Chain::ALL…next()` picks first configured executor; `to` is raw EOA | ERC-20 profit never swept; multi-chain profit stranded; hot destination | Per-chain sweep of the executor that earned (native + profit-token ERC-20); destination = Safe + Zodiac Roles Modifier v2 (scoped, allowance-metered, revocable) |
| GAP-21 | Settlement ledger is dead wiring | `data/transfers.jsonl` read by `/api/transfers` but **no writer exists**; `last_swept` advances on submission not confirmation; sweep userop outcome never polled | Failed sweep silently orphans profit; dashboard ledger permanently empty | Track sweep userop to receipt → append ledger row `{ts,chain,token,amount,to,tx,status}` on `Swept` event; advance `last_swept` only on confirmed receipt |

**Round 14 — detection-model audit (industry end-to-end research):**
why 199,398 "detections" produced $0 — mapped against how top searchers
actually find profit. Sources cited per gap.

| ID | Gap | Evidence | Economic cost | Closure plan | Sources |
|---|---|---|---|---|---|
| GAP-33 | Detection consumes **settled-state residuals** — no pre-confirmation trigger on any chain | `evm_stream.rs`: `newHeads` + Multicall poll only; no `newPendingTransactions` consumer exists; BSC/Polygon/Avax have real public mempools we subscribe to nothing on | Every detection is post-extraction residue — the observed universe is structurally zero-profit | `newPendingTransactions` (BSC/Poly/Avax) or 48Club Puissant `newPendingAuctions(WithState)` (pre-simulated state feed) or bloXroute `pendingTxs` → selector-dictionary calldata decode → apply-to-local-state → backrun sim | docs.bloxroute.com/bsc/streams/newtxs-and-pendingtxs; docs.48.club/puissant-builder; chainscorelabs.com mempool streaming guide |
| GAP-34 | **Zero WSS-qualified endpoints** — the event layer has no transport | registry: 365 discovered / 124 qualified / **0 wss entries**; scout skips `wss://` chainlist URLs (`scout/main.rs:346`); `caps.websocket` + `transport` fields never populated | `swap_watch_loop` + discovery WSS are dead code — confirmed-state reaction impossible on all 10 chains | Scout WSS rung: accept `wss://` in discovery, derive `wss://` candidates from live HTTPS endpoints + keyed-provider WS forms, probe `eth_subscribe newHeads` → qualify `transport=wss` records | rpc_registry.json audit; chainlist.org/rpcs.json |
| GAP-35 | Submission rail is the **slowest class** — UserOp → public bundler mempool on every chain | `relay::submit` routes all chains through `submit_via_aa`; no `eth_sendBundle`/`blxr_submit_bundle`/direct-sequencer path exists | Structural last-in-race everywhere; compounds GAP-30 route leak on BSC | BSC: Puissant `eth_sendBundle` or bloXroute `blxr_submit_bundle` (builders: 48club/blockrazor/jetbldr/nodereal); L2: single atomic tx direct to sequencer (FCFS race); UserOp demoted to sweep path only | docs.bloxroute.com/bsc/submit-bundles; docs.48.club/puissant-builder; ai-frb.com Arbitrum playbook |
| GAP-36 | **"Detected" metric counts sub-friction dust** as opportunity | dashboard: 199,398 detected / 0 qualified — gross spreads ~$0.0001 vs a ~$0.003 two-swap fee floor; the industry friction region (fee+base+priority+bribe deadband) is never modeled as a detection precondition | A flattering metric hid total blindness for days — "detected" ≠ "opportunity" | Redefine detected = candidate above friction band on evaluated state; report **executable-universe size** as the headline funnel metric | LIPIcs AFT-2025 searcher-profitability paper (friction region); quantmemo.com CEX-DEX arb explainer |
| GAP-37 | No **expected-value submission** mode — hard profit gate rejects the rational cheap-revert L2 play | `min_profit_usd` hard gate on estimated profit; documented L2 bot fires 1,984 atomic attempts → 8 profitable (revert cost is cents on L2); our gate would refuse that entire strategy class | Coverage ceiling on the one niche evidence says is winnable — FCFS L2 backrun | EV mode per chain: submit when `P(success)·net − revert_cost > 0`, revert-cost cap + journaled rationale; never on expensive-revert paths | devilsintheopcodes.substack.com (L2 bot anatomy); ashiq.co.za Arbitrum latency game |
| GAP-38 | Token/pool universe is a **curated slice**, not an enumeration | pools.json: bsc 216 pools/60 tokens · arb 153/37 · poly 205/43 · base 172/61 · avax 111/25 · eth 63/23; reference engines enumerate every factory (Aether: 6+ protocols; HopScout: full `PairCreated` factory discovery) | Long-tail + connector routes invisible by construction — universe blindness upstream of every strategy | Factory `PairCreated`/`PoolCreated` enumeration per chain → on-chain verify → liquidity-pruned graph; feeds the GAP-25 graph substrate | github.com/Denis123-rg/Aether-V1; github.com/BruceBrasseur/HopScout; pools.json audit |

**Competitive-reality note (measured, not aspirational):** CEX–DEX arb is
a trident — Wintermute/SCP/Kayle captured ~73% of extracted value,
~90% by Q1 2025, via vertical searcher-builder integration (AFT-2025
measurement). Public-RPC polling cannot enter that race. The evidence-
supported winnable niches: **(a) L2 atomic backrun/cyclic on confirmed
state (FCFS, capital-light), (b) BSC backruns via Puissant/bloXroute
pending feeds, (c) long-tail/new-pool detection.** Strategy effort
concentrates there.

**Applicability caveat:** blueprint phases 1/3/5 are Ethereum-mainnet
shaped. Arbitrum and Base have no public mempool and no builder auction —
the portable equivalents are Swap-event ingestion (GAP-3) and private
submission where a verified relay exists. `block.coinbase` tipping is
N/A on L2s; inclusion incentive on active chains runs through the
Pimlico paymaster path.

**Rule:** closing a gap means measured evidence of the capability in
production, not merged code. Each closure re-runs the relevant
blindness test (§28) and is journaled.

### Implementation status (2026-10-10 integration)

Compiled, unit-tested and deployed to the release fleet — awaiting
production evidence before any may be marked CLOSED:

| ID | State | What landed |
|---|---|---|
| GAP-1/6 | implemented | `simgate.rs`: `eth_simulateV1`→`eth_call` fail-closed gate (6 tests) + `simgate_replay` + `flash_provider_probe` examples |
| GAP-3 | implemented (partial) | WSS `Swap`/`Sync`/`Mint`/`Burn` + Solidly `Sync(uint256,uint256)` subs, ≤200-addr filter batching, WSS candidate ladder, classified no-WSS events; Puissant feed still pending |
| GAP-10 | implemented | `rpc_transport.rs`: alloy FallbackLayer + RetryBackoffLayer |
| GAP-12 | implemented | `token_risk.rs` (GoPlus, fail-closed) wired into `discovery_loop` promotion gate |
| GAP-15 | implemented | `deploy/nssm/` installer + `monitoring/oz-monitor-config.yaml` staged |
| GAP-16 | implemented | figment layered config; parity test vs legacy loader on real ENV |
| GAP-17 | implemented | `shard_supervisor`: 120s registry reread, 2-tick hysteresis, Heal event on rescale |
| GAP-18 | implemented | alloy `Erc4337Api` + `PackedUserOperation` replaces hand-rolled AA; `eth_getUserOperationReceipt` replaces `pimlico_getUserOperationStatus` |
| GAP-19 | implemented | realized profit = decoded `Executed` event from receipt logs; `credit_settled` is the only crediting path |
| GAP-21 | implemented | `track_sweep` polls to confirmed `Swept` event → `transfers.jsonl` writer; `last_swept` advances on confirmation only |
| GAP-22 | implemented | `quote_token_usd`: live V2/V3 pool quote vs route's verified-stable legs for non-stable `minProfit` |
| GAP-23 | implemented | `telemetry_export.rs` OTel/OTLP (feature-gated, off by default) |
| GAP-24 | implemented | challenger: `baseFee + maxPriorityFee` with `eth_gasPrice` fallback |
| GAP-26 | implemented | `is_finalized`: `finalized`/`safe` tag gate per chain + depth fallback |
| GAP-27 | implemented | `PimlicoClient::redact` strips apikey from all error paths |
| GAP-29 | implemented | bus outbox spool + replay-on-reconnect |
| GAP-31 | implemented | in-process pending-nonce allocator + `note_userop_included/dropped` |
| GAP-32 | implemented | `max_signal_age_ms` staleness gate (~2× block cadence) → classified Rejected |
| GAP-4 | verified prior | `submit_flashbots` fails closed — confirmed no change needed |
| GAP-2 | verified prior | `fetch_pools.py` depth preservation confirmed |
| GAP-11 | decision | `ccxt-rust` rejected — immature; Binance WS suffices |
| GAP-5/7/8/9/13/14/20/25/28/30 | pending | probe tool exists (5/7/30); designs written (8/20/28); 25 needs solver graph work; 9/13/14 deferred |

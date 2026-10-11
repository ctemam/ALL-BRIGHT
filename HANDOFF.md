# ALLBRIGHT — SESSION HANDOFF (2026-10-11)

## 0. Latest deploy — solver/executor USD-pricing alignment (2026-10-11, deployed)

**Root cause killed:** the `$991M`-class phantom xreqs are eliminated at source.

- `allbright-core::chain::stable_addr_str(chain, addr)` is now the **single canonical stable-address table** (all 10 chains). Executor `relay.rs` delegates to it — no more duplicated list.
- `allbright-solver::pricing::usd_per_unit_via_pools` now accepts **stable legs only, verified by address** (not symbol — a fake "USDC" can't inject a bait quote). WETH/native legs no longer back USD quotes, matching the executor's minProfit-conversion rule exactly.
- Regression tests (`pricing.rs` mod tests): WETH-only leg → 0.0; canonical stable leg → spot price; spoofed stable symbol at wrong address → 0.0. **3/3 pass; 26/26 solver+core suite green.**
- Deployed: solver + executor + sentinel rebuilt release and restarted; ports 3001/3101–3109 verified listening.
- **Verified post-deploy funnel (journal window):** 449 `Rejected` — `below_strategy_threshold` p50 gross **$0.0002, max $0.058** vs $5 floor; `no_usd_pricing` ×32 now rejected at solver source (previously executor-side). **Zero xreq, zero exec, zero `expected_usd` anomalies — honest starvation, not phantom flow.**
- Realized P&L remains **$0.00** — no submission/inclusion/settlement observed. The gate-alignment work is complete; the residual blocker is market truth (dust spreads at block-poll latency), not a code defect.

## 1. Operational state

- **Engine is ARMED LIVE by Commander order** ("Remove halt", 2026-10-11). `HALT.flag` removed; all 11 PM2 processes online; ports 3001/3101–3108 listening on 127.0.0.1.
- `AUTO_EXECUTE=true`, `MIN_PROFIT_USD=5`, `ACTIVE_CHAINS=bsc,arb,poly,base,avax`, `STRATEGIES=P01–P05`.
- All 10 `*_RPC_URL` populated from top-OVS `QUALIFIED` registry endpoints (executor `chains: []` defect fixed at boot).
- New ENV keys: `AVAX_WSS_URL=wss://api.avax.network/ext/bc/C/ws` (blind `/rpc`→`/ws` substitution was 405), `EV_CHAINS`, `EV_FLOOR_USD`, `EV_EMIT_SECS` (EV-class, pending build).
- **Commander directive 2026-10-11: keyed private free-tier RPCs are the primary path; shared public endpoints demoted to fallback tier.** §36.9 expanded to 16 providers / **126 endpoint expansions** (ALCHEMY, INFURA, DRPC, BLASTAPI, ANKR, CHAINSTACK, BLOCKPI, NODIT, TENDERLY, NOWNODES, MORALIS, ONFINALITY, NODEREAL, CHAINNODES, ALLTHATNODE, DVELLIR) — `_API_KEY` slots all in ENV, **ALL EMPTY, awaiting free-tier keys from Commander**. Until keys land the fleet still runs on the public registry (demotion is automatic once keys populate — keyed endpoints prepend).
- **Verified-live keyless premium endpoints added to `*_RPC_URLS` (eth_chainId-probed 2026-10-11):** PublicNode ×5 chains, Tenderly ×4 (bsc dead), 1RPC avax, OnFinality base = 11 endpoints. Dead and excluded: LlamaRPC (525 all), Omnia (521 all).
- **Dashboard RPC-budget surface added (2026-10-11):** `/api/infrastructure` now exposes per-chain `private`/`private_qualified`/`capacity_rps`/`budget_rps`/`budget_day` (80% rule, §36.7/§36.9 — null until scout verifies sustained rps, never fabricated) and a `providers` roster (16 templated keyed providers + 6 paste-in/keyless: QuickNode, GetBlock, Zan, PublicNode, Tenderly keyless, 1RPC) with `configured` flag and advertised free-tier quota. `PRIVATE_PROVIDERS` hoisted to module-level `pub const` in config.rs — the two duplicated tables (`assemble` + `legacy_load` parity oracle) now share one source; `PROVIDER_CATALOG`/`PASTE_IN_PROVIDERS` live beside it. `access_of` fix: keyless `gateway.tenderly.co` URLs (no key path segment) now classify Public — they were mislabeled Private and could enter the private-only production pool. ENV consolidated: all 16 `*_API_KEY` slots under one §36.9 header with advertised quotas + paste-in instructions for QuickNode/GetBlock/Zan.
- Live evidence: bait pools produced $957–$5,270 phantom edges, convicted at simgate (`CamelotPair:K`/`Arbswap:K`); honest funnel now shows sub-floor dust (net −$0.008–0.016).
- `PRIVATE_KEY` remains exposed plaintext — rotation due before real capital.
- **Do not stop the fleet or restore `HALT.flag` without explicit Commander authorization.**

## 2. Honest P&L reality — never relitigate

- Realized profit: **$0.00 lifetime.** 13,028 execution attempts, 100% failed, zero `Executed`/`Swept` events, `data/transfers.jsonl` empty.
- The 199,398 "detected opportunities" were **post-settlement residual spreads ~$0.0001 — sub-fee-floor dust**, not candidates. Detection consumed settled state only.
- The pre-fix app the Commander remembers as "profitable" displayed **hard-coded fabricated profit rows** (removed in commit `facabab`, Sep 29). Real profit never existed.

## 3. The core finding (round-14 industry research)

Allbright's detection model is structurally blind: **no pre-confirmation trigger exists on any chain** (no `newPendingTransactions`, no sequencer feed, no orderflow). It observes settled reserves one heartbeat after extraction. Evidence-supported winnable niches: (a) L2 atomic backrun/cyclic on confirmed state (FCFS), (b) BSC backruns via Puissant/bloXroute pending feeds, (c) long-tail/new-pool detection. CEX–DEX is a Wintermute/SCP/Kayle trident — do not chase.

## 4. Gap register

**Retired.** The legacy GAP-1..38 numbering was superseded 2026-10-11 by the unified register in §7 below (U1–U28). Archive: `data/legacy_gap_register_2026-10-10.md`. New gaps enter the U-register only.

## 5. LANDED: GAP-34 scout WSS rung + Registry-WSS Wiring + Solver Flash Friction

Changed `crates/allbright-rpc-scout`, `crates/allbright-core`, `crates/allbright-ingest`, `crates/allbright-solver`:

- `allbright-core`:
  - Added `RpcRegistry::wss_qualified_urls(chain)` method ordering qualified WSS endpoints by OVS.
  - Saturated arithmetic in `RpcRegistry::upsert()` for probes and errors preventing u64/u32 overflow panics.
- `allbright-rpc-scout`:
  - Cargo.toml: +`tokio-tungstenite`.
  - main.rs: WebSocket ladder probe (`eth_chainId` -> `eth_subscribe newHeads`), `derive_wss()` covering live endpoints, saturating counters on probes/errors.
  - Verified sweep: **49 Qualified WSS endpoints** and **14 Degraded WSS endpoints** discovered and verified across all 10 chains (`eth: 8`, `base: 7`, `op: 7`, `poly: 6`, `arb: 5`, `avax: 4`, `bsc: 3`, `linea: 3`, `blast: 3`, `scroll: 3`).
- `allbright-ingest`:
  - `wss_candidates()` now prefers probe-verified `wss_qualified_urls` from the registry before falling back to blind scheme substitution, with $O(1)$ set lookup.
- `allbright-solver`:
  - Net profit gate in `main.rs` now deducts flash loan premium (`5 bps` Aave V3 baseline on borrowed amount) in addition to gas cost: `net_profit = gross_usd - gas_usd - flash_usd`.

**Status:** Full workspace builds clean (`cargo build --workspace`), all unit test suites pass (`core: 26/26`, `executor: 16/16`, `ingest: 4/4`). Registry holds 49 qualified WSS endpoints.

## 6. Earlier landed work (all tested, deployed pre-shutdown)

- Settlement-truth set in executor: GAP-18/19/21/26/27/31/32 — tests 16/16.
- Ingest: GAP-3 (WSS swap-watch code — dead until GAP-34 feeds it), GAP-12 GoPlus gate, GAP-17 shard supervisor — tests 4/4.
- Core: GAP-16 figment config, GAP-23 telemetry, GAP-29 bus outbox — tests 26/26.
- Economics: GAP-22 `quote_token_usd`, GAP-24 EIP-1559 challenger.
- `AGENTS.md` Appendix D gap-discovery protocol + SKILL §42 codified.

## 7. UNIFIED GAP REGISTER — 3-round deep research (2026-10-11)

28 industry references analyzed (rounds 1–3). `U#` = unified register ID; `GAP-#` = AGENTS.md Appendix B ID where mapped. Before = Allbright today; After = industry standard.

| U# | Layer | Gap | Before | After — import / fix | GAP |
|---|---|---|---|---|---|
| U1 | Detect | ~~No pending-tx trigger~~ **Implemented v1** | `pending_watch_loop` (ingest): `eth_subscribe` full pendingTxs per chain → `PendingSwap` → solver pre-inclusion rescan. Honest limit: chains without a public mempool (Arb/Base/OP sequenced) expose nothing — watcher fails closed there; value lands on ETH/BSC/Poly when keyed WSS is provisioned | Base Flashblocks `newFlashblockTransactions`/`pendingLogs` (free, public RPC); bloXroute `arbOnlyMEV`/`pendingTxs` (BSC/Base/Poly); Puissant (48Club, gated) | GAP-33 |
| U2 | Detect | Shallow pool universe | 1,145 curated pools / 10 chains | Tycho Indexer stream — full-protocol reorg-aware state, auto-discovery | GAP-38 |
| U3 | Detect | WSS endpoint derivation | Blind `https→wss` broke AVAX (`/rpc` vs `/ws`) | Registry OVS + explicit `*_WSS_URL` overrides | — |
| U4 | Detect | Single ordering model | "submit tx" for all chains | Per-chain matrix: FCFS / PGA / Timeboost / bundle-builders | new |
| U5 | Filter | Phantom signals emitted | $957–$5,270 fake edges reached simgate/UserOp | Never trust computed edge — behavior sim before emit | new |
| U6 | Filter | No pre-emission probe | Baits discovered by burning sponsored UserOps | `eth_call` buy→sell roundtrip as real sender (SolidQuant `Simulator.sol`) | new |
| U7 | Filter | No bytecode-hash denylist | Address-level conviction only | Hash-level denylist catches CREATE2 re-deploys (Gemba) | new |
| U8 | Filter | Event-derived state trust | Salmonella surface — logs lie | revm/tycho-simulation on real state, real sender | new |
| U9 | Filter | Conviction-only defense | Reactive quarantine only | Layered: Tycho blocklist → probe → hash denylist → conviction → simgate | new |
| U10 | Compute | ~~Route search ≤3-hop~~ **Implemented v1** | Bellman-Ford-Moore negative cycles live in solver (`negative_cycles`): `-ln(rate×(1-fee))` edges, ≤4 legs, ≤8 cycles/scan, deduped vs brute force; catches 4-hop paths enumeration missed | Bellman-Ford-Moore negative cycles (DeFiPoser, MMBF, RICH; tycho-searcher `bellman_ford.rs`) | GAP-25 |
| U11 | Compute | Pool-math-only sim | **Groundwork live**: `evm_dry.rs` — revm engine + `LazyRpcDb` (cached JSON-RPC state) executes the real `execute()` call locally behind `REVM_DRYRUN=true` (default off; `Db` miss → remote `eth_call`). Full route-level behavior sim still needs indexed state (Tycho) | revm replay on indexed state, zero network | new |
| U12 | Compute | Ternary sizing only | Iterative; dies on state drift | Closed-form `x*` (Quarry); on-chain `amountOut` recompute | new |
| U13 | Compute | State+sim ~15–30ms | Multicall RPC round-trips | In-process state via Tycho stream; bounded by public-RPC constraint | new |
| U14 | Execute | ~~Public bundler only~~ **Implemented v1 (Eth)** | `submit_flashbots` is real: `eth_sendPrivateTransaction`→rpc.flashbots.net fast mode (free/keyless) or `eth_sendBundle`→relay with `FLASHBOTS_AUTH_KEY`; EOA-signed EIP-1559, priority fee = ordering bid (`ETH_PRIORITY_WEI`) | `blxr_submit_bundle`/`bsc_private_tx` (48club/blockrazor/jetbldr/nodereal); Puissant; FastLane — non-Eth chains still Pimlico public | GAP-35 |
| U15 | Execute | ~~Aave-only flash~~ **Implemented** (stale entry) | `flash_providers` is already cheapest-first: Balancer V2 + Morpho 0% before Aave 5bps, probed on-chain per submission; ERC-3156/UniV3-flash dynamic fallbacks exist | Morpho 0% / UniV3 flash-swap / Maker DSS; EIP-3156 provider picker | new |
| U16 | Execute | No flash-swap-as-loan | Borrows where it could flash-swap | First leg via `swap()` callback = 0% (MEV_V2 pattern) | new |
| U17 | Execute | ABI-encoded calldata | Large payloads | Packed u64/u128 + command-byte dispatcher (pawurb/MEV_V2) | new |
| U18 | Execute | Bid channel unmodelled | UserOp priority only | Cheapest bid per chain: gas vs coinbase vs bloXroute dynamic-fee | new |
| U19 | Execute | One submission path | Pimlico for all 5 chains | Base: flashblock-window+BackRunMe; BSC: bundles; Arb: FCFS/Timeboost; Poly: bloXroute/FastLane; Avax: FCFS | new |
| U20 | Execute | No cheap-revert class | $5 floor discarded all sub-floor edges | Optimal on FCFS L2s (arXiv 2506.01462) — **coded, unbuilt**: `ev_mode`/`EV_CHAINS`/`EV_FLOOR_USD`/`EV_EMIT_SECS` | GAP-37 |
| U21 | Execute | Sizing dies on drift | Off-chain size → revert | On-chain optimal-input computation (blind-arb pattern) | new |
| U22 | Settle | — compliant — | Receipt-decoded `Executed`/`Swept`, finality gate, challenger | Already `coinbase_diff`-equivalent; keep | — |
| U23 | Settle | Revert-cost opaque | Sponsored quota burn unmetered | Meter attempt cost → journal (reverts are counterparty revenue) | new |
| U24 | Strategy | Public-RPC infra ceiling | ~15–30ms state vs competitors' in-node ~ns | Unfixable within constraint — win where latency isn't the mechanism | new |
| U25 | Strategy | Independent bid-shading | Would lose sealed-bid races | Avoid auction venues; prefer FCFS/BackRunMe where speed wins | new |
| U26 | Ops | Journal ~21GB/26GB free | Unbounded growth | Rotation/archival schedule | new |
| U27 | Ops | Exposed `PRIVATE_KEY` | Plaintext ENV | Rotate; HSM/remote signer | new |
| U28 | Ops | Uncommitted changeset | Landed + in-flight work not in git | Build → test → commit discipline | new |

**Import order agreed:** ① Tycho stream+sim (state+universe+blocklist) → ② Base Flashblocks (free GAP-33) → ③ probe contract → ④ bloXroute BSC rails (U14) → ⑤ Morpho/UniV3 flash-swap routing (U15/16) → ⑥ negative-cycle route search (U10) → ⑦ packed-calldata executor (U17/21).

**Reference index (28):** Artemis; MEV-Share+simple-blind-arbitrage; bloXroute BDN/streams; Chainbound Fiber; Hindsight; SolidQuant mempool series; Gemba sniper; mevdao v2-sniper; EIP-3156 provider landscape; mev_sendBundle/mev-boost docs; arXiv 2507.13023 (CEX-DEX dark forest); arXiv 2506.01462 (L2 spam optimal); Arbitrum Timeboost+PGA; bloXroute BSC bundles/BackRunMe; Puissant (48Club); Defi-Cartel Salmonella; arXiv 2309.13501 (honeypot taxonomy); DeFiPoser-ARB + MMBF + RICH; Quarry+tycho-searcher+reth-mev; EigenPhi annual data; Tycho Indexer/Simulation; Base Flashblocks API+deep dive; pawurb gas-optimization; adaptive-mev-router MEV_V2.yul; Quantifying REV (Flashbots); integrated-builder advantage + bidding papers; Princeton CEX-DEX gap-survival thesis.

## 8. Rules in force

- No claim without measured evidence; "implemented" ≠ "closed" (register rule).
- Self-heal covers liveness only (62/62 heals) — blindness remediation is an open gap the Commander flagged.
- No fabricated numbers, no guessed addresses, fail-closed everywhere.
- Commander authority is absolute; engine stays dead until ordered.

## 9. Tycho license finding + spike tooling (2026-10-11)

- **`tycho-client` license is NOT MIT on current releases.** `0.435.0`+ (incl. latest `0.452.2`) switched to **Fynd License 1.1** — a custom commercial license: §3.1 mandates every Settlement "materially derived" from a Fynd Output (which *includes indexed pool state*) execute through their **Designated Router**; §5.1(a) forbids settling via an independently implemented executor — i.e. Tycho-state detection + `AllbrightExecutor` settlement is license-forbidden on ≥0.435. Platform Fee ≤15 bps + Positive Slippage Capture + 20% revenue share on Settlement/Service Revenue (Proprietary Trading Return exempted from rev-share but NOT from the router mandate).
- **MIT pin: `=0.430.0`** — the last permissive release (Sep 29, 2026). Perpetual per §9.1. `TychoStreamBuilder`, `enable_partial_blocks` (flashblocks), `blocklisted_ids`, TVL filters all present. Risk: frozen API vs upstream; server-compat is the open question the spike answers.
- License matrix at 0.430.0: `tycho-client` MIT, `tycho-common` MIT, `tycho-simulation` MIT, `tycho-execution` **BUSL-1.1** (not needed — it encodes their router; our executor is our contract).
- **Spike tooling added** (`tools/`): `tycho-client` ships a ready CLI binary (`cargo install tycho-client --version =0.430.0`) — snapshot+delta streaming, JSONL output, no code needed. `tools/tycho_spike.ps1` orchestrates per-chain capture (`-Chain base -Seconds 120 -PartialBlocks`); `tools/tycho_analyze.py` measures msg rate, block cadence, header→recv latency, unique components, flashblock fraction, coverage vs `pools.json` registry → `spikes/summary.tsv`. **Blocked on `TYCHO_AUTH_TOKEN`** (PropellerHeads Telegram bot).

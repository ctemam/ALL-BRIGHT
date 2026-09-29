# HANDOFF — scanner rewiring (P0) and what is left before production

**Date:** 2026-09-28 · **Branch:** `main` · **HEAD:** `c914158`
**Session scope:** P0 only — rewire the `zero-cap-arb-main` backend scanner to read prices
per-venue through verified factories, and remove fabricated addresses. P1 was explicitly
out of scope.

---

## 1. Status right now (what is actually verified)

| Check | Result | Evidence |
|---|---|---|
| `cargo fmt` | clean | exit 0 |
| `cargo test --all-targets` | **78 passed, 0 failed** | `logs/test.out` |
| `cargo clippy --all-targets` | **NOT RE-CONFIRMED at handoff** | job still running; prior run clean except pre-existing dead-code warnings |
| `cargo build` | **NOT RE-CONFIRMED at handoff** | job still running |
| Live scanner accuracy vs real chain state | **NOT VERIFIABLE IN THIS ENVIRONMENT** | see §4 |

Re-run to close the two open rows:

```powershell
cd c:\Users\op\Desktop\zerocap\zero-cap-arb-main\backend
cargo clippy --all-targets
cargo build --release
```

**Everything is uncommitted.** `git status` shows a large diff spanning backend and
frontend, plus new untracked files (`multicall.rs`, `rpc_pool.rs`, `rpc_catalog.rs`,
`config.rs`, `pimlico_client.rs`, `profit_transfer.rs`, `ecosystem.config.js`,
`ports.config.json`, `Dockerfile`, `render.yaml`, `DEPLOYMENT.md`, `docs/`). This is more
than this session's work — split into reviewable commits before merging.

---

## 2. The five structural defects fixed in P0

Arbitrage was structurally impossible before these fixes — the scanner could not have found
a real opportunity regardless of market conditions.

1. **Venue addresses ignored.** One hardcoded venue served every read, so both "sides" of
   each quote came from the same pool and netted to zero. → The scanner now resolves prices
   per venue. Source of truth is `get_venues()` (`chains.rs`); `radar_scanner.rs` quotes
   venues on-chain in two MultiCall3 phases (`quote_venues_on_chain`).
2. **Chain coverage incomplete.** → `get_dexes_for_chain()` now projects from the venue
   table instead of a separate hand-maintained list, so the two cannot drift apart.
3. **Fabricated addresses.** Invented addresses removed. What remains are canonical
   *published* addresses — see §4; they are **not** locally verified in this sandbox.
4. **Malformed calldata.** The `getPair` (68-byte) and `getPool` (dynamic selector)
   encoders in `multicall.rs` produced wrong-length calldata; fixed. `read_u256` is used
   for amounts and `decode_address` was added.
5. **`slot0` sign extension.** Decoding sign-extended a field that must stay unsigned,
   corrupting price reads; fixed.

Diagnostic change in `radar_scanner.rs`: pool-resolution logging now distinguishes
**Reverted** from **Empty/No Pool**. Previously both looked like "no liquidity", which
would have masked a broken integration (bad calldata, wrong factory) as a boring market
result. If you see `Reverted`, suspect the encoding or the address, not the market.

---

## 3. New: the `aggregate3` encoder is pinned to foundry

`multicall::tests::encode_matches_foundry_reference_bytes` compares `encode_aggregate3`
byte-for-byte against Foundry's independent encoder:

```text
cast calldata "aggregate3((address,bool,bytes)[])" \
  "[(0x00000000000000000000000000000000000000AA,true,0x0102),
    (0x00000000000000000000000000000000000000BB,true,0x0902f1ac)]"
```

This replaced a comment that merely *asserted* the bytes had been checked. It paid for
itself immediately: the first run failed, and the diff localized the mismatch to the final
data word (`0902f1ac` vs `415bf38f`) — i.e. my **test input** was wrong (`0x415bf38f` is
`symbol()`; `0x0902f1ac` is the real `getReserves()` selector), while **every layout word
matched**: offsets `0x40`/`0xe0`, tuple heads, tails. Encoder confirmed correct; test input
corrected. Worth knowing how this bug class behaves: a wrong offset table makes the
aggregator revert, and the scanner reports that as "no pools found" — never as the
encoding bug it is.

Two gotchas when extending this test:
- `SubCall::new` sets `allow_failure: true` — a golden vector built with `false` won't match.
- the test helper `addr(n)` fills **all twenty** bytes with `n`; pinning external reference
  bytes needs explicitly constructed addresses.

---

## 4. BLOCKER: no live verification possible in this sandbox

**Read this before trusting any address in `chains.rs`.**

Every public RPC reachable from this environment (publicnode, cloudflare, drpc, …) returns
`eth_getCode` == `0x` for the canonical Uniswap V2 factory and for the mainnet USDC/WETH
pair. They are not serving real mainnet state. Consequences:

- A prior session documented these addresses as "verified on-chain". **That claim was
  false** and has been corrected in `chains.rs` — they are canonical *published* addresses,
  unverified here.
- **End-to-end scanner accuracy is unproven.** Encoders and decoders are tested; whether
  the venue table resolves to real, funded pools is not.

Reproduce (expected to fail here, must succeed on your node):

```powershell
# canonical Uniswap V2 factory; a healthy node returns non-trivial bytecode
$body = '{"jsonrpc":"2.0","id":1,"method":"eth_getCode","params":["0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f","latest"]}'
Invoke-RestMethod -Uri <your-rpc> -Method Post -ContentType 'application/json' -Body $body
```

**On a trusted node, before P1:** for every chain in `chains.rs`, confirm `eth_getCode` is
non-empty for each factory/router/token, then run the scanner and require a non-zero pool
count per venue per chain with **zero `Reverted`** lines in pool-resolution logs.


---

## 5. Remaining work to reach production

### A. Trust & verification (blocking)

1. Point `rpc_pool.rs` / `rpc_catalog.rs` at trusted RPCs with real keys; retire the public
   endpoints as primary (§4 proves they are not even usable for reads here).
2. Run the per-chain `eth_getCode` sweep above; fix or drop any address that fails.
3. Scanner soak against live RPC: non-zero venues resolved, no `Reverted`, quotes
   economically plausible (spot-check a sample against a block explorer).

### B. Build, secrets, runtime

4. **`.env.secrets` does not exist** (`.env` does). `ecosystem.config.js` loads
   `.env.secrets` → `.env` → `.env.local`, later files winning. Create it before any
   production start; keep it out of git.
5. `cargo build --release`. PM2's `resolveBackendBin()` prefers
   `backend/target/release/zero-cap-arb-backend.exe`, falls back to `debug/`;
   `ZCA_BACKEND_BIN` overrides.
6. **The PM2 stack is currently empty — nothing is running** (`pm2 ls` returns no rows).
   If a previous session had it up, it is down now. Start deliberately:
   `ZCA_MODE=simulation; npm run pm2:start`. Simulation and production use disjoint port
   ranges (`ports.config.json` → `backend`, `frontend`) so both stacks can coexist.
   It was not auto-started here: that launches trading behaviour and is your call.

### C. Deploy path (exists, unverified)

7. `Dockerfile`, `docker-entrypoint.sh`, `render.yaml` and `DEPLOYMENT.md` (Frankfurt
   region) are present but have **not** been exercised end-to-end in this session. Build
   the image and run the container before trusting them.
8. Frontend changes (`next.config.js`, `wagmi.ts`, components, untracked
   `package-lock.json`, `frontend/scripts/`, `pimlico.ts`) are uncommitted and were not
   reviewed this session — decide whether they belong in the P0 commit.

### D. P1 (out of scope, not started)

9. P1 scope lives in `docs/GLOBAL_MARKET_STATE_AND_EXECUTION_FABRIC.md` and whatever plan
   you hold alongside it. Nothing here implements execution; the scanner only reads.
   Re-derive P1 tasks *after* §A closes — its assumptions depend on live-verified venues.

---

## 6. Environment notes (cost time this session; they may cost you time)

- **Do not set `CARGO_TARGET_DIR` for this project.** An earlier session pointed it at
  `D:\zt`, outside the repo and invisible in review, and PM2 was quietly running the binary
  from there. The repo convention — and what PM2 resolves — is `backend/target/`. The stray
  scratch files there have been deleted; `D:\zt` now holds only a stale build cache you can
  remove once `backend/target` has a real build.
- Verification artifacts now go to `logs/` (already gitignored), not to ad-hoc paths.
- Shell-tool quirks: commands time out at 30s (run long builds detached and poll for a
  `.done` marker), each command gets a fresh working directory (use absolute paths or one
  combined `Set-Location` call), and a leading `[` in a command string breaks the wrapper.
- `cast` needs each argument quoted separately; passing the tuple array inside one escaped
  string silently produced garbage instead of erroring.

---

## 7. Suggested first actions for the next session

1. `cargo clippy --all-targets && cargo build --release` — close §1's two open rows.
2. Commit P0 — the new files are untracked, so `git add` matters.
3. Swap in trusted RPCs, then run the §4 sweep — everything else is downstream of that.


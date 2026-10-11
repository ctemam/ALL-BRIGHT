# GAP-20 — Sweep Architecture: per-chain, multi-token, Safe + Zodiac Roles v2

## Current defects (verified in code)
1. `aa.rs:519` encodes `sweepNativeCall` only — the contract's
   `sweep(token,to)` (`AllbrightExecutor.sol:342`, selector `0xb8dc491b`) is
   never used → all ERC-20 profit stranded.
2. `aa.rs:504-508` — `Chain::ALL…filter_map(executor_address).next()` sweeps
   the FIRST configured chain only → per-chain profit silos.
3. `config.rs:239` / `data/profit_wallets.json` — destination is a raw EOA
   registry → hot destination, single-sig theft surface.
4. `transfers.jsonl` has no writer (GAP-21) and `last_swept` advances on
   submission not on confirmed receipt.

## What routes actually settle in
`relay.rs:379-381` refuses any `token_in` outside `stable_addr(chain)`, so
profit can ONLY accrue in the per-chain stable list (`relay.rs:101-159`) plus
native dust. The complete sweep set per chain is in
`sweep_architecture.json → sweep_set_per_chain` (all addresses verified
on-chain via `decimals()` on 2026-10-08 per the file header). **Blast note:**
USDB is rebasing — sweep it, but consolidate promptly and treat balance as a
moving target.

## New sweep loop (replaces `send_gasless_sweep` internals)
For every chain in `cfg.active_chains` with `executor_address` + RPC:
1. Probe `balanceOf(executor)` for each listed token + native balance
   (selector `keccak256("balanceOf(address)")[..4]` — pattern already in
   `relay.rs:244`).
2. Each token ≥ dust gate (~$1): UserOp → `sweep(token, CHAIN_SAFE)`.
3. Native ≥ dust: `sweepNative(CHAIN_SAFE)` (selector `0x78c093cf`).
4. Optionally batch via CALLTYPE_BATCH **only after** confirming the GAP-8
   CallPolicy covers batch mode; else sequential ops under the GAP-31
   pending-nonce allocator.
5. Poll `user_op_outcome` → on `included`, decode `Swept(token,to,amount)`
   from the receipt → append `data/transfers.jsonl`
   `{ts,chain,token,amount,to,tx,userop_hash,status}`; advance `last_swept`
   ONLY on confirmed receipt (closes the GAP-21 writer hole too).

## Destination — one Safe address, every chain
- `SafeProxyFactory.createProxyWithNonce(singleton, init, salt)` is
  cross-chain deterministic → **same Safe address on all 10 chains** (do NOT
  use `createChainSpecificProxyWithNonce` — that binds chainId into the salt).
- Verified addresses: factory `0x4e1DCf7AD4e460CfD30791CCC4F9c8a4f820ec67`
  (v1.4.1), singletons `0x41675C099F32341bf84BFc5382aF534df5C7461a` (Safe) /
  `0x29fcB43b46531BcA003ddC8FCB67FFE91900C762` (SafeL2), singleton factory
  `0x914d7Fec6aaC8cd542e72Bca78B30650d45643d7` — sources: safe-global
  safe-contracts v1.4.1 CHANGELOG, safe-fndn/safe-singleton-factory README;
  re-verify via `@safe-global/safe-deployments` at deploy time.
- Config: 2-of-3 Commander cold keys. Use SafeL2 singleton on L2s, Safe on
  L1-style chains — identical proxy address either way.

## Zodiac Roles Modifier v2 — onward-movement scoping
The Kernel CallPolicy already pins `to`=Safe (GAP-8). The Roles mod governs
what can move OUT of the Safe without multisig ceremony:

- **Topology**: one Roles instance per chain, module-enabled on that chain's
  Safe; `owner = avatar = target = the Safe`
  (docs.zodiac.eco/developers/roles + /quickstart-roles pattern).
- **Mastercopy**: `0xF2964CE6161ce0e75964Fe7927cE114cb0B283D5` (v2.1.1;
  `zodiac-roles-sdk` v4 targets v2 contracts).
- **Role `conveyor`** (member = Turnkey-scoped conveyor key or the Kernel
  account):
  - `scopeTarget` each stable token → `scopeFunction approve(spender)` with
    `spender ONE_OF [0x1231DEB6f5749EF6cE6943a275A1D3E7486F4EaE]` +
    `withinAllowance` USD budget;
  - `scopeTarget` LI.FI diamond → scoped call selectors, `Send` option for
    native legs;
  - `allowFunction … transfer(to)` with `to == TREASURY_SAFE` — direct
    consolidation rail;
  - `setAllowance(key, balance, maxRefill, refill, period=86400, ts)` —
    rolling daily cap.
- **Exec path**: member calls `execTransactionWithRole(to,value,data,op,
  roleKey,shouldRevert)`; the condition tree is enforced on-chain — a
  compromised conveyor cannot redirect value anywhere the role doesn't
  already allow.
- **Revocation**: Safe threshold tx → `assignRoles(member,[role],[false])`
  or `disableModule` — instant.

## Preconditions / rollback / split
See `sweep_architecture.json`. Key ordering: deploy Safe + Roles BEFORE
GAP-8 CallPolicy args are frozen (the policy embeds the Safe address).

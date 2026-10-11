# GAP-8 — Key Custody: Kernel v3 Permissions + Turnkey Enclave

## Problem
`PRIVATE_KEY` sits hot in ENV (`config.rs:238`) as the Kernel account's ECDSA
**root validator** — unrestricted signing authority over the smart account that
owns every per-chain `AllbrightExecutor`. Total-compromise surface.

## Design — two layers, fail-closed

### Layer 1: on-chain scoped signer (ZeroDev Kernel v3 Permissions)
`Permission = 1 signer + N policies + 1 action`
(docs.zerodev.app/sdk/v5_3_x/permissions/intro)

- **Signer**: new ECDSA "ops key" (held in Turnkey — Layer 2).
- **Action**: `0xe9ae5c53` — ERC-7579 `execute(bytes32,bytes)` single mode only.
- **CallPolicy** `0x67fa43ca2d689bea4d10b9f2c96828828a2ab275` (kernel_v3,
  deterministic across chains — verify `eth_getCode` per chain):
  - `target` = each `ALLBRIGHT_EXECUTOR_<CHAIN>` address; `valueLimit = 0`.
  - Allowed functions (selectors, verified):
    - `execute((address,address,address,uint8,uint24)[],uint256,uint256,address,bytes)`
      — **literal selector: derive at build** (`calldata::executeCall::SELECTOR`;
      not in public selector DBs — do not guess). Arg conditions:
      `amountIn <= MAX_BORROW`, `flashProvider ONE_OF <per-chain verified
      providers from relay.rs:44-96>`, dynamic args `null`.
    - `sweep(address,address)` = `0xb8dc491b` — `token ONE_OF <stable_addr(chain)>`,
      **`to EQUAL <CHAIN_PROFIT_SAFE>`**.
    - `sweepNative(address)` = `0x78c093cf` — **`to EQUAL <CHAIN_PROFIT_SAFE>`**.
  - **Excluded**: `setPaused` `0x16c38b3c`, all callbacks, every other target —
    unlisted = revert.
- **GasPolicy**: `{ allowed: 0, enforcePaymaster: true }` — the key can only
  send Pimlico-sponsored UserOps; it cannot spend a wei of gas.
- **RateLimitPolicy** `0x6a06358e6b283921deceabe7e8a3741d506cca9b`
  (with-reset; documented working with Pimlico): `count=60, interval=3600`.
  Fallback: scheduled variant `0xf63d4139B25c836334edD76641356c6b74C86873`.

**Enable flow (Commander action, once per chain):** Kernel v3 selects validators
via the userOp nonce key (validation type + validator id — packing per
zerodevapp/kernel v3.1 `ValidationManager.sol`). The first op is enable-mode:
`signature = root-signed approval of (signer+policies+action) ‖ ops-key sig`.
The existing PRIVATE_KEY signs the approval once — scripted, offline preferred.

### Layer 2: Turnkey enclave
Ops key generated inside a Turnkey sub-org; host holds only an API credential.
Signing = `POST /public/v1/submit/sign_raw_payload` on the 32-byte userOpHash —
byte-identical output to today's `owner.sign_message`.

**Honest limitation:** raw-payload signing has no `eth.tx.*` context — Turnkey
cannot inspect calldata inside a hash. That is by design here: semantic
confinement is enforced **on-chain** (Layer 1 reverts disallowed calls), while
Turnkey provides non-exportability, actor scoping, quorum consensus, instant
revocation, and an immutable audit log.

Policy sketch (see JSON for full set):
```json
{ "policyName": "ops-key-signs-raw-userops-only",
  "effect": "EFFECT_ALLOW",
  "consensus": "approvers.any(user, user.id == '<AUTOMATION_API_USER_ID>')",
  "condition": "activity.type == 'ACTIVITY_TYPE_SIGN_RAW_PAYLOAD_V2' && private_key.id == '<OPS_KEY_ID>'" }
```
For a future tx-signing conveyor key (GAP-28), `eth.tx.*` IS available:
`eth.tx.chain_id in [10-chain list] && eth.tx.value < cap && eth.tx.to in
[ROLES_MOD, LIFI_DIAMOND]`; ABI upload enables `eth.tx.function_name` /
`eth.tx.contract_call_args` conditions
(docs.turnkey.com/features/policies/language, /smart-contract-interfaces).

## Compromise matrix
| Event | Result |
|---|---|
| Host RCE reads ENV | no key material present; API cred is policy-scoped |
| Turnkey API key leaks | sig-requests only; on-chain CallPolicy still binds output; instant revoke |
| Ops key fully extracted | can ONLY call executor fns with `to`=Safe, whitelisted providers, sponsored gas, ≤60 ops/hr |
| Root key compromise | residual god-mode → rotate root to cold/Turnkey post-enable (Commander) |

## Preconditions / rollback / split
See `key_custody.json` — verbatim fields `preconditions`, `rollback`,
`implementation_split` (Rust: enable-mode encoding, permission nonce ops,
TurnkeyRawPayloadSigner; Commander: Turnkey setup, enable signature, Safe
deployment ordering, root rotation).

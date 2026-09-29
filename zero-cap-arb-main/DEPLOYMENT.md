# Local Ports & Deployment

## Port map

All ports live in [`ports.config.json`](./ports.config.json) — the single source of
truth shared by `ecosystem.config.js`, `backend/src/config.rs`, and the Dockerfile.
Do not hardcode port numbers elsewhere.

| Service   | Env var          | Simulation         | Production        |
|-----------|------------------|--------------------|-------------------|
| Backend   | `PORT`           | **3001** (3011, 3021) | **4001** (4011, 4021) |
| Frontend  | `FRONTEND_PORT`  | **3000** (3010, 3020) | **4000** (4010, 4020) |

The two ranges are disjoint, so a simulation stack and a production stack can run
side by side on the same machine. Parenthetical values are the reserved backup
ports.

`PORT` always wins when set, so PM2 and container runtimes can override without a
code change.

## PM2 (local process supervision)

```bash
# Simulation stack
ZCA_MODE=simulation pm2 start ecosystem.config.js
pm2 status
pm2 logs
pm2 stop && pm2 delete all

# Production stack (disjoint ports, same config)
ZCA_MODE=production pm2 start ecosystem.config.js
```

Useful environment variables:

| Variable | Default | Purpose |
|---|---|---|
| `ZCA_MODE` | `simulation` | `simulation` or `production` — selects the port range and process names |
| `ZCA_BACKEND_BIN` | auto | Override the backend binary path (release → debug fallback) |
| `DEPLOYMENT_REGION` | `frankfurt` | Region label; see below |
| `PROFIT_TRANSFER_MODE` | `MANUAL` | `AUTO` or `MANUAL` — see "Profit withdrawal" below |
| `PROFIT_TRANSFER_MIN_USD` | `25` | Server-enforced withdrawal floor |

App names are suffixed with the mode (`zcab-backend-simulation`,
`zcab-frontend-production`) so both stacks are distinguishable in `pm2 list`.

## Frankfurt region

`DEPLOYMENT_REGION=frankfurt` is the **advisory** part: the backend logs it at
startup and RPC endpoint selection prefers EU-hosted providers.

The part that actually determines latency is **where the process runs**. A region
label cannot move a process. Three real options:

| Option | What you get | Requires |
|---|---|---|
| **A. Frankfurt VPS** | ~5–15 ms to EU builders and EU RPC hosts. Best latency. | A host in Frankfurt (Hetzner CX/CPX, OVH, Contabo) + SSH access |
| **B. Local + Frankfurt pin** | RPC round-trips improve because EU endpoints are preferred; wire-to-builder latency unchanged | Nothing |
| **C. Frankfurt engine, local control plane** | Engine near builders, dashboard on your machine | Option A host + a tunnel |

The `Dockerfile` and `render.yaml` are built so that deploying to Frankfurt is a
configuration step, not a code change:

```bash
# Option A — build and run on a Frankfurt host
docker build -t zero-cap-arb .
docker run -d --name zca -p 3000:3000 -p 3001:3001 \
  -e DEPLOYMENT_REGION=frankfurt -e ZCA_MODE=production \
  --env-file .env.secrets zero-cap-arb
```

```bash
# Option — Render Frankfurt
# render.yaml already pins region: frankfurt; set the sync:false env vars
render deploys create . --name zero-cap-arb
```

Verify where you actually ended up:

```bash
curl -s http://localhost:3001/api/health
# and check the startup log line:
# INFO zero_cap_arb_backend: mode=production bind=0.0.0.0:4001 region=frankfurt profit_transfer=MANUAL
```

## Profit withdrawal safety

`PROFIT_TRANSFER_MODE` defaults to **`MANUAL`**. Automatic withdrawal must be
explicitly enabled, and the server enforces a minimum (`PROFIT_TRANSFER_MIN_USD`,
default 25) so a dust balance cannot trigger a transfer. Keep it on `MANUAL`
until destination addresses are configured.

## RPC pool (150+ free endpoints)

`backend/src/rpc_catalog.rs` ships 161 default endpoints across the 6 chains
(25-29 each). The pool in `backend/src/rpc_pool.rs` registers them at startup
alongside any `*_RPC_URL` env overrides and tracks each one individually:

- latency-ranked selection (fastest healthy endpoint, not just the first)
- HTTP 429 / 503 -> 60s cooldown; other failures -> 15s
- 5 consecutive failures -> 5 minute quarantine
- a quarantined endpoint needs 2 consecutive successes to recover, so one lucky
  response cannot re-enable a rate-limited node
- a global `Semaphore` caps concurrent outbound requests

Endpoints going stale is expected and harmless: the pool quarantines them
automatically. Operators can extend or override any list via
`ETH_RPC_URL`, `ETH_RPC_URL_1`, ... or a comma-separated list.

Inspect live health:

```bash
curl -s localhost:3001/api/rpc/health
```

`best_latency_ms` is `null` until an endpoint has been probed.

## Profit transfer (AUTO / MANUAL)

`PROFIT_TRANSFER_MODE` selects the mode. **MANUAL is the default** — `AUTO` must
be set explicitly, and even then the sweep only fires once accrued profit
reaches the threshold.

| Variable | Default | Meaning |
|---|---|---|
| `PROFIT_TRANSFER_MODE` | `MANUAL` | `AUTO` enables the background sweep |
| `PROFIT_TRANSFER_MIN_USD` | `25` | Server-enforced floor; cannot be bypassed by the caller |
| `PROFIT_TRANSFER_MAX_USD` | unset | Optional per-transfer cap |
| `PROFIT_TRANSFER_INTERVAL_SECS` | `300` | AUTO sweep interval |
| `PROFIT_DESTINATIONS` | unset | `address:label:pct` triples, comma separated |

Example:

```bash
PROFIT_DESTINATIONS="0xYourWallet:70,0xReserve:20"   # invalid, both are dropped
PROFIT_DESTINATIONS="0x2eF3...D56:Wallet:70,0x1111...1111:Reserve:20"
```

Safety properties enforced server-side:

- Addresses are validated as 20-byte hex. Placeholders like `0xYourWallet` are
  **dropped with a warning** rather than treated as a destination.
- Shares summing above 100% are rejected; a remainder under 100% is reported
  as `unallocated_usd` rather than silently reassigned.
- Transfers below the minimum, above the cap, or with no valid destinations are
  refused with a reason and recorded in the ledger.
- **No executor is wired into the scanner**, so a transfer is reported as
  `simulated: true` and `ok: false` — nothing is broadcast. Supply a
  `TransferExecutor` to enable real sends; the module never claims to have sent
  funds it did not send.

```bash
curl -s localhost:3001/api/profit/config
curl -s -X POST localhost:3001/api/profit/preview  -d '{"amount_usd":1000}' -H 'Content-Type: application/json'
curl -s -X POST localhost:3001/api/profit/transfer -d '{"amount_usd":1000}'  -H 'Content-Type: application/json'
curl -s localhost:3001/api/profit/history
```

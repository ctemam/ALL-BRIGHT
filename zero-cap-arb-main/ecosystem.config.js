// ── Zero-Cap Arbitrage — local PM2 process supervision ───────────────────────
// Supervises the Rust backend and the Next.js frontend so the stack survives
// crashes and restarts automatically.
//
// Port values come from ports.config.json (single source of truth, shared with
// backend/src/ports.rs). Do not hardcode port numbers in this file.
//
// Mode selection: set ZCA_MODE=simulation (default) or ZCA_MODE=production.
// Simulation and production use disjoint port ranges, so both stacks can run
// at the same time without conflict.
//
//   ZCA_MODE=simulation  npm run pm2:start
//   ZCA_MODE=production  npm run pm2:start
//   npm run pm2:stop
//
// See SETUP.md and DEPLOYMENT.md for the Frankfurt region deployment.

const path = require('path');

// Load secrets first so the public .env template can stay in version control
// without exposing credentials. Order matters: later files win.
for (const f of ['.env.secrets', '.env', '.env.local']) {
  const p = path.join(__dirname, f);
  try {
    require('dotenv').config({ path: p });
  } catch (_) {
    // dotenv missing or file absent — non-fatal, env vars come from PM2.
  }
}

const ports = require('./ports.config.json').services;
const MODE = (process.env.ZCA_MODE || 'simulation').toLowerCase() === 'production' ? 'production' : 'simulation';
const portFor = (svc) => ports[svc][MODE].primary;

const ROOT = __dirname;
const backendPort = portFor('backend');
const frontendPort = portFor('frontend');

// Resolve the compiled backend binary. A release build is preferred, but a
// debug build is used as a fallback so `pm2 start` works during development
// without waiting on a full --release compile. Set ZCA_BACKEND_BIN to point at
// a specific binary (or a custom wrapper script).
function resolveBackendBin() {
  const explicit = process.env.ZCA_BACKEND_BIN;
  if (explicit) return explicit;
  const base = path.join(ROOT, 'backend', 'target');
  const name = process.platform === 'win32'
    ? 'zero-cap-arb-backend.exe'
    : 'zero-cap-arb-backend';
  for (const profile of ['release', 'debug']) {
    const candidate = path.join(base, profile, name);
    if (require('fs').existsSync(candidate)) return candidate;
  }
  // Nothing built yet: return the expected release path so PM2 reports a clear
  // "script not found" naming the exact binary the operator needs to build.
  return path.join(base, 'release', name);
}

// The Next.js config sets output: 'standalone'. If the standalone bundle is not
// present (e.g. the app has not been built yet), fall back to the Next CLI so
// PM2 reports the missing build rather than a bare "script not found".
function resolveFrontendScript() {
  const standalone = path.join(ROOT, 'frontend', '.next', 'standalone', 'server.js');
  if (require('fs').existsSync(standalone)) {
    return { script: standalone, args: '' };
  }
  const nextBin = path.join(ROOT, 'frontend', 'node_modules', 'next', 'dist', 'bin', 'next');
  return { script: process.execPath, args: nextBin + ' start' };
}

const backendBin = resolveBackendBin();
const frontend = resolveFrontendScript();

const sharedEnv = {
  ZCA_MODE: MODE,
  DEPLOYMENT_REGION: process.env.DEPLOYMENT_REGION || 'frankfurt',
  RUST_LOG: process.env.RUST_LOG || 'info',
};

module.exports = {
  apps: [
    {
      name: `zcab-backend-${MODE}`,
      script: backendBin,
      cwd: path.join(ROOT, 'backend'),
      env: {
        ...sharedEnv,
        PORT: String(backendPort),
        // Profit transfer: MANUAL by default so nothing auto-sends funds.
        // Set ZCA_MODE-independent override via .env.secrets to enable AUTO.
        PROFIT_TRANSFER_MODE: process.env.PROFIT_TRANSFER_MODE || 'MANUAL',
        PROFIT_TRANSFER_MIN_USD: process.env.PROFIT_TRANSFER_MIN_USD || '25',
        RPC_MAX_CONCURRENCY: process.env.RPC_MAX_CONCURRENCY || '16',
      },
      out_file: path.join(ROOT, 'logs', `backend-${MODE}-out.log`),
      error_file: path.join(ROOT, 'logs', `backend-${MODE}-error.log`),
      log_date_format: 'YYYY-MM-DD HH:mm:ss Z',
      autorestart: true,
      max_restarts: 10,
      restart_delay: 3000,
      min_uptime: '10s',
      max_memory_restart: '1G',
    },
    {
      name: `zcab-frontend-${MODE}`,
      // The Next.js config sets output: 'standalone', so serve the compiled
      // server directly. `next start` is incompatible with standalone output;
      // resolveFrontendScript() falls back to the CLI only if the bundle is absent.
      script: frontend.script,
      args: frontend.args,
      cwd: path.join(ROOT, 'frontend'),
      env: {
        ...sharedEnv,
        PORT: String(frontendPort),
        HOSTNAME: '0.0.0.0',
        NODE_ENV: 'production',
        // Server-side proxy target: the backend on its own mode port.
        RUST_BACKEND_URL: `http://127.0.0.1:${backendPort}`,
        NEXT_PUBLIC_API_BASE: `http://127.0.0.1:${backendPort}`,
      },
      out_file: path.join(ROOT, 'logs', `frontend-${MODE}-out.log`),
      error_file: path.join(ROOT, 'logs', `frontend-${MODE}-error.log`),
      log_date_format: 'YYYY-MM-DD HH:mm:ss Z',
      autorestart: true,
      max_restarts: 10,
      restart_delay: 3000,
      min_uptime: '10s',
      max_memory_restart: '512M',
    },
  ],
};

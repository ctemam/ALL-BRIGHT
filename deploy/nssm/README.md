# NSSM services — PM2 autorestart replacement

**What this is.** `install_services.ps1` registers every `allbright-*.exe` in
`D:\allbright-target\release` as a Windows service via NSSM, replacing the
`pm2 start ecosystem.config.js` autorestart plane on Windows. Nothing is
installed by this repo — NSSM itself is an external prerequisite
(https://nssm.cc), and running the script is an explicit operator action.

## PM2 → NSSM mapping

| PM2 | NSSM |
|---|---|
| `autorestart: true` | `AppExit Default Restart` (app-level) + optional `sc.exe failure` (SCM-level) |
| restart delay | `AppRestartDelay` (2000 ms) |
| rapid crash loops | `AppThrottle` (10 s window — see below) |
| `out_file`/`error_file` | `AppStdout`/`AppStderr` → `logs\<name>.{out,err}.log` |
| `pm2-logrotate` | `AppRotateFiles`/`AppRotateOnline`/`AppRotateBytes` (10 MiB) |
| `env: {RUST_LOG=…}` | `AppEnvironmentExtra` |
| `max_memory_restart` | **deliberately absent** — RSS limits belong to sentinel policy, not the supervisor |
| `cron_restart` (pool-curator) | Scheduled Task (`schtasks /SC DAILY`), **not** a service |

`AppDirectory` is pinned to `D:\Allbright`: `Config::load()` discovers the
`ENV` file relative to the working directory — wrong `AppDirectory` = the
service boots on defaults + process env only.

## Why restart-loops become *visible* instead of masked

PM2's autorestart hides loops two ways: rapid respawns look like uptime in
`pm2 list`, and unbounded restarts have no backoff signal. NSSM makes the
loop legible:

- **`AppThrottle` (10 s):** a process that exits inside the throttle window
  delays its next start by the remainder. A crash-looping binary degrades
  to ≤ 1 attempt / 10 s — slow enough that `Get-Service` shows the service
  flickering STOPPED↔RUNNING and the loop is measurable in the log files.
- **Per-exit evidence:** `logs\<name>.err.log` + `nssm status <name>` + the
  Windows Application event log (`Event ID` from `nssm` source) record every
  exit code and restart timestamp — a loop leaves a countable trail instead
  of a blurred "online".
- **No silent swallow:** NSSM never auto-succeeds. If the binary exits 0
  immediately and repeatedly, that IS the signal — the service stays
  "started" only while the app genuinely runs.

Check a service: `nssm status allbright-ingest`, `Get-Content logs\allbright-ingest.err.log -Tail 50 -Wait`.

## Operate

```powershell
# install / update parameters (idempotent; -Reinstall recreates, -StartNow starts)
powershell -ExecutionPolicy Bypass -File deploy\nssm\install_services.ps1 -Reinstall -StartNow

nssm start|stop|restart <service>
nssm remove <service> confirm
```

Deploy flow unchanged: `cargo build --release` → `nssm restart <name>` per
service. HALT.flag and execution gates are untouched by the supervisor —
a service restart is never an execution decision.

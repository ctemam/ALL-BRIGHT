#Requires -RunAsAdministrator
<#
.SYNOPSIS
    Allbright — NSSM service definitions (replaces PM2 autorestart on Windows).

.DESCRIPTION
    Registers every allbright-* release binary as a Windows service via NSSM
    (the Non-Sucking Service Manager, https://nssm.cc). This is the native
    replacement for `pm2 start ecosystem.config.js` / `autorestart: true`.

    Design (mirrors ecosystem.config.js, comments cite the NSSM param used):

      PM2 concept                    NSSM equivalent
      -----------------            ----------------------------------------
      autorestart: true            AppExit Default Restart (+ Windows SCM
                                   recovery, see note at bottom)
      restart loop / crash loops   AppThrottle — if the process dies inside
      (pm2 "errored" status)       this window the next start is DELAYED by
                                   the remainder of the window, so loops are
                                   visible in logs + SCM instead of spinning
      restart delay                AppRestartDelay (ms between restarts)
      out_file / error_file        AppStdout / AppStderr (+ AppRotate* —
                                   pm2 rotate equivalent, no pm2-logrotate
                                   plugin needed)
      max_memory_restart           NOT replicated — see README.md (restart-on-
                                   RSS needs the sentinel, not a service loop)
      env: { RUST_LOG=... }        AppEnvironmentExtra
      interpreter: "none"          native .exe — AppParameters stays empty
      cron_restart (pool-curator)  NOT a service — scheduled task (see bottom)

    CRITICAL: AppDirectory is D:\Allbright — allbright-core's Config::load()
    discovers the `ENV` file relative to the process working directory.

    Idempotent: re-running re-sets parameters (install is skipped if the
    service already exists; use -Reinstall to force remove+recreate).

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File deploy\nssm\install_services.ps1
    powershell -ExecutionPolicy Bypass -File deploy\nssm\install_services.ps1 -Reinstall
#>
param(
    [string]$NssmExe   = "nssm",                       # or full path, e.g. C:\nssm\nssm.exe
    [string]$RepoRoot  = "D:\Allbright",
    [string]$BinDir    = "D:\allbright-target\release",
    [string]$LogDir    = "D:\Allbright\logs",
    [switch]$Reinstall,
    [switch]$StartNow                                   # also start services after config
)

$ErrorActionPreference = 'Stop'

# --- The fleet: PM2 `apps` entries that are Rust binaries -------------------
# name, exe, extra env vars (beyond RUST_LOG), startup order hint.
# deploy-executor.exe is intentionally EXCLUDED: it is an interactive
# deployment tool, not a daemon (never run under a supervisor).
$services = @(
    @{ name = 'allbright-ingest';      env = @() },
    @{ name = 'allbright-simulation';  env = @() },
    @{ name = 'allbright-solver';      env = @() },
    @{ name = 'allbright-executor';    env = @() },
    @{ name = 'allbright-research';    env = @() },
    @{ name = 'allbright-observatory'; env = @() },
    @{ name = 'allbright-challenger';  env = @() },
    @{ name = 'allbright-sentinel';    env = @() },
    @{ name = 'allbright-rpc-scout';   env = @('RPC_SCOUT_INTERVAL_SECS=1800') },
    @{ name = 'allbright-dashboard';   env = @() }
)

function Invoke-Nssm {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Args)
    & $NssmExe @Args | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "nssm $($Args -join ' ') failed (exit $LASTEXITCODE)" }
}

if (-not (Get-Command $NssmExe -ErrorAction SilentlyContinue)) {
    throw "NSSM not found at '$NssmExe'. Install nssm (https://nssm.cc) or pass -NssmExe <path>."
}
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null

foreach ($svc in $services) {
    $name = $svc.name
    $exe  = Join-Path $BinDir "$name.exe"
    if (-not (Test-Path $exe)) {
        Write-Warning "SKIP $name — $exe not built (cargo build --release -p $name)"
        continue
    }

    $existing = Get-Service -Name $name -ErrorAction SilentlyContinue
    if ($existing -and $Reinstall) {
        if ($existing.Status -ne 'Stopped') { Invoke-Nssm 'stop' $name }
        Invoke-Nssm 'remove' $name 'confirm'
        $existing = $null
    }
    if (-not $existing) {
        Invoke-Nssm 'install' $name $exe
    }

    # Working directory: REQUIRED for ENV-file discovery in Config::load().
    Invoke-Nssm 'set' $name 'AppDirectory' $RepoRoot

    # Autorestart + restart throttling:
    #  AppRestartDelay — fixed wait between exit and relaunch (ms).
    #  AppThrottle     — crash-loop guard: a process that exits inside this
    #                    window delays its next start by the remainder.
    #                    A crash-looping binary degrades to <=1 start/10s
    #                    and stays visible (event log + stderr), instead of
    #                    PM2's opaque rapid-respawn.
    Invoke-Nssm 'set' $name 'AppExit' 'Default' 'Restart'
    Invoke-Nssm 'set' $name 'AppRestartDelay' '2000'
    Invoke-Nssm 'set' $name 'AppThrottle' '10000'

    # Graceful stop: Ctrl+C first (services handle SIGINT-ish console ctrl),
    # then force. Times are ms.
    Invoke-Nssm 'set' $name 'AppStopMethodConsole' '5000'
    Invoke-Nssm 'set' $name 'AppStopMethodSkip' '6'   # skip WM_CLOSE+Terminate on console-only apps

    # stdout/stderr → logs\<name>.{out,err}.log with in-place rotation
    # (NSSM rotates on size AND while running — pm2-logrotate equivalent).
    Invoke-Nssm 'set' $name 'AppStdout' (Join-Path $LogDir "$name.out.log")
    Invoke-Nssm 'set' $name 'AppStderr' (Join-Path $LogDir "$name.err.log")
    Invoke-Nssm 'set' $name 'AppRotateFiles' '1'
    Invoke-Nssm 'set' $name 'AppRotateOnline' '1'
    Invoke-Nssm 'set' $name 'AppRotateBytes' '10485760'   # 10 MiB per segment
    Invoke-Nssm 'set' $name 'AppRotateBytesHigh' '0'

    # Env: RUST_LOG filter identical to ecosystem.config.js, plus per-app extras.
    $envVars = @('RUST_LOG=info,allbright=info') + $svc.env
    Invoke-Nssm 'set' $name 'AppEnvironmentExtra' @envVars

    Invoke-Nssm 'set' $name 'AppNoConsole' '1'
    Invoke-Nssm 'set' $name 'Start' 'SERVICE_AUTO_START'
    Invoke-Nssm 'set' $name 'Description' "Allbright service — $name (managed by NSSM; see deploy/nssm/README.md)"

    if ($StartNow) { Invoke-Nssm 'start' $name }
    Write-Host "OK   $name  <-  $exe" -ForegroundColor Green
}

# --- pool-curator: NOT a service ---------------------------------------------
# ecosystem.config.js runs it with autorestart:false + cron_restart daily —
# a service would mask its exit. Map the cron to a Scheduled Task instead:
#
#   schtasks /Create /SC DAILY /ST 05:00 /TN "allbright-pool-curator" ^
#     /TR "python D:\Allbright\tools\pool_curator.py" /F
#
# (Left commented intentionally — enabling it is an operator decision.)
#
# --- Windows SCM recovery (belt & suspenders) --------------------------------
# NSSM handles app-level restarts; SCM failure actions cover the nssm shim
# itself dying. Optional:
#   sc.exe failure <name> reset= 86400 actions= restart/5000/restart/5000/restart/30000

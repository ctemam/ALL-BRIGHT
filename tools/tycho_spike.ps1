# Tycho feed spike — measures stream quality for the hybrid-ingest decision.
#
# Usage:
#   $env:TYCHO_AUTH_TOKEN = "<key from @PropellerHeads bot>"
#   powershell -File tools/tycho_spike.ps1 -Chain base -Seconds 120
#   powershell -File tools/tycho_spike.ps1 -Chain bsc -Seconds 120
#
# Pins tycho-client 0.430.0 — the LAST MIT-licensed release (>=0.435 is Fynd
# License 1.1, which forbids settling Fynd-derived routes outside their router).
# Writes spikes/<chain>.jsonl then runs tools/tycho_analyze.py.

param(
    [Parameter(Mandatory = $true)][string]$Chain,
    [int]$Seconds = 120,
    [int]$MaxMessages = 0,
    [string]$TychoUrl = "tycho-beta.propellerheads.xyz",
    [string[]]$Exchanges = @(),
    [string]$MinTvl = "",
    [switch]$PartialBlocks
)

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

if (-not $env:TYCHO_AUTH_TOKEN) {
    Write-Error "TYCHO_AUTH_TOKEN not set — request a key via the PropellerHeads Telegram bot."
}

# per-chain exchange defaults (Tycho extractor names)
$defaults = @{
    base     = @("uniswap_v2", "uniswap_v3", "uniswap_v4", "aerodrome", "pancakeswap_v3")
    ethereum = @("uniswap_v2", "uniswap_v3", "uniswap_v4", "sushiswap_v2", "pancakeswap_v3")
    bsc      = @("uniswap_v2", "uniswap_v3", "pancakeswap_v2", "pancakeswap_v3")
}
if ($Exchanges.Count -eq 0) {
    if ($defaults.ContainsKey($Chain)) { $Exchanges = $defaults[$Chain] }
    else { $Exchanges = @("uniswap_v2", "uniswap_v3") }
}

# ensure MIT-pinned CLI
$cli = Get-Command tycho-client -ErrorAction SilentlyContinue
if (-not $cli) {
    Write-Host "tycho-client not found — installing 0.430.0 (MIT, last permissive release)..."
    cargo install tycho-client --version "=0.430.0"
    $cli = Get-Command tycho-client -ErrorAction SilentlyContinue
    if (-not $cli) { Write-Error "install failed — cargo install tycho-client --version 0.430.0" }
}
& tycho-client --version

New-Item -ItemType Directory -Force -Path "spikes" | Out-Null
$out = "spikes/$Chain.jsonl"

$cliArgs = @(
    "--tycho-url", $TychoUrl,
    "--chain", $Chain,
    "--include-tvl",
    "--log-folder", "spikes/logs-$Chain"
)
foreach ($e in $Exchanges) { $cliArgs += @("--exchange", $e) }
if ($MinTvl) { $cliArgs += @("--min-tvl", $MinTvl) }
if ($PartialBlocks) { $cliArgs += "--partial-blocks" }
if ($MaxMessages -gt 0) { $cliArgs += @("--max-messages", "$MaxMessages") }

Write-Host "spike: chain=$Chain exchanges=[$($Exchanges -join ',')] seconds=$Seconds -> $out"
Write-Host "cmd : tycho-client $($cliArgs -join ' ')"

$job = Start-Job -ScriptBlock {
    param($args_, $out_)
    & tycho-client @args_ 2>$null | Out-File -Encoding utf8 $out_
} -ArgumentList ($cliArgs, $out)

Wait-Job $job -Timeout $Seconds | Out-Null
if ($job.State -eq "Running") { Stop-Job $job }
Receive-Job $job | Out-Null
Remove-Job $job -Force

$lines = (Get-Content $out | Measure-Object -Line).Lines
Write-Host "captured $lines JSONL lines -> $out"
python tools/tycho_analyze.py --chain $Chain --file $out --summary-file spikes/summary.tsv

#!/usr/bin/env python
"""Allbright pool curator — scheduled system job (PM2 cron_restart).

Systemic pool-registry maintenance (AGENTS §28, §34.0): runs the
depth-aware fetch pipeline on a schedule, then restarts ONLY the ingest
collector when pools.json actually changed — ingest reloads the registry
at startup, so a bounded restart is the mechanism by which new verified
coverage reaches the live universe.

This job exists so registry refresh is a system capability, not manual
agent work. Bounded: one service, only on real change. Everything it
does lands in PM2 logs + data/universe_coverage.json.
"""
import hashlib
import json
import os
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
POOLS = os.path.join(ROOT, "pools.json")
STATE = os.path.join(ROOT, "data", "pool_curator_state.json")


def digest(path):
    try:
        with open(path, "rb") as f:
            return hashlib.sha256(f.read()).hexdigest()
    except OSError:
        return None


def run_fetch():
    r = subprocess.run(
        [sys.executable, os.path.join(ROOT, "tools", "fetch_pools.py")],
        cwd=ROOT, capture_output=True, text=True, timeout=3600)
    sys.stdout.write(r.stdout[-4000:])
    sys.stderr.write(r.stderr[-1000:])
    return r.returncode


def restart_ingest():
    r = subprocess.run(["pm2", "restart", "allbright-ingest"],
                       capture_output=True, text=True, timeout=60)
    sys.stdout.write(r.stdout[-500:])
    return r.returncode == 0


def main():
    os.makedirs(os.path.dirname(STATE), exist_ok=True)
    before = digest(POOLS)
    rc = run_fetch()
    after = digest(POOLS)
    changed = after is not None and after != before
    restarted = False
    if changed:
        restarted = restart_ingest()
        print("pools.json changed — ingest restarted (new universe live)")
    else:
        print("pools.json unchanged — no restart")
    state = {"ts": time.time(), "fetch_rc": rc, "pools_hash": after,
             "changed": changed, "ingest_restarted": restarted}
    with open(STATE, "w") as f:
        json.dump(state, f, indent=1)
    print("curator state →", STATE)


if __name__ == "__main__":
    main()

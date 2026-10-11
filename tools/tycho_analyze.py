#!/usr/bin/env python3
"""Tycho feed spike analyzer.

Reads JSONL FeedMessage lines (from `tycho-client` CLI stdout) either live
(stdin, timestamps each message on receipt) or from a saved file.

Measures the go/no-go metrics for the hybrid ingest decision:
  - message rate + stream duration
  - distinct blocks, block cadence, partial-block (flashblocks) fraction
  - header.latency = receipt_time - header.timestamp (server block time vs local clock)
  - unique component IDs seen (pool universe coverage)
  - per-exchange component counts
  - snapshot vs delta volume, removed components, synchronizer states
  - coverage vs pools.json registry for the chain (addresses intersected)

Usage:
  tycho-client ... | python tools/tycho_analyze.py --chain base
  python tools/tycho_analyze.py --chain base --file spikes/base.jsonl
"""
import argparse
import json
import sys
import time
from collections import Counter, defaultdict


def load_registry(path, chain):
    try:
        with open(path, encoding="utf-8") as f:
            pools = json.load(f)
    except Exception:
        return set(), 0
    addrs = {
        p["address"].lower()
        for p in pools
        if p.get("chain") == chain and p.get("address")
    }
    return addrs, len([p for p in pools if p.get("chain") == chain])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--chain", required=True)
    ap.add_argument("--file", help="saved JSONL; omit to read stdin live")
    ap.add_argument("--registry", default="pools.json")
    ap.add_argument("--summary-file", help="append one-line TSV summary")
    args = ap.parse_args()

    registry_addrs, registry_total = load_registry(args.registry, args.chain)

    n_msgs = 0
    blocks = set()
    partial_blocks = 0
    full_blocks = 0
    latencies = []
    components = {}  # id -> exchange
    deltas_updates = 0
    snapshot_states = 0
    removed = 0
    sync_states = Counter()
    tvl = []
    first_ts = last_ts = None
    first_recv = last_recv = None

    def consume(line, recv):
        nonlocal n_msgs, partial_blocks, full_blocks, deltas_updates
        nonlocal snapshot_states, removed, first_ts, last_ts, first_recv, last_recv
        line = line.strip()
        if not line or not line.startswith("{"):
            return
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            return
        state_msgs = msg.get("state_msgs") or {}
        sync = msg.get("sync_states") or {}
        if not state_msgs and not sync:
            return  # not a FeedMessage line (logs etc.)
        n_msgs += 1
        first_recv = first_recv or recv
        last_recv = recv
        for ex, sm in state_msgs.items():
            hdr = sm.get("header") or {}
            bn = hdr.get("number")
            if bn is not None:
                blocks.add(bn)
                if hdr.get("partial_block_index") is not None:
                    partial_blocks += 1
                else:
                    full_blocks += 1
            ts = hdr.get("timestamp")
            if ts:
                first_ts = first_ts or ts
                last_ts = max(last_ts or ts, ts)
                latencies.append(recv - ts)
            for cid, _cws in ((sm.get("snapshots") or {}).get("states") or {}).items():
                snapshot_states += 1
                components.setdefault(cid.lower(), ex)
            d = sm.get("deltas") or {}
            for cid in ((d.get("state_updates") or {}).keys()):
                deltas_updates += 1
                components.setdefault(cid.lower(), ex)
            for cid in (sm.get("removed_components") or {}).keys():
                removed += 1
        for _ex, st in sync.items():
            if isinstance(st, dict):
                sync_states[st.get("status", "?")] += 1
            else:
                sync_states[str(st)] += 1

    if args.file:
        base_recv = None
        with open(args.file, encoding="utf-8", errors="replace") as f:
            for line in f:
                # post-hoc: no true receipt time; use file position order only for
                # counting; latency uses header ts deltas, flagged as approximate
                consume(line, time.time())
        recv_mode = "file(no recv ts)"
    else:
        for line in sys.stdin:
            consume(line, time.time())
        recv_mode = "live"

    dur = (last_recv - first_recv) if (first_recv and last_recv) else 0.0
    n_components = len(components)
    per_ex = Counter(components.values())
    coverage_hit = len(set(components) & registry_addrs)
    coverage_pct = (100.0 * coverage_hit / registry_total) if registry_total else 0.0

    lat_sorted = sorted(latencies)
    lat_med = lat_sorted[len(lat_sorted) // 2] if lat_sorted else 0
    lat_p95 = lat_sorted[int(len(lat_sorted) * 0.95)] if lat_sorted else 0
    lat_max = lat_sorted[-1] if lat_sorted else 0

    print(f"\n=== TYCHO SPIKE [{args.chain}] ({recv_mode}) ===")
    print(f"messages          : {n_msgs}")
    print(f"duration          : {dur:.1f}s   rate={n_msgs/dur:.2f}/s" if dur else "duration: n/a")
    print(f"distinct blocks   : {len(blocks)}  partial(flashblock) msgs={partial_blocks} full={full_blocks}")
    print(f"header latency    : median={lat_med:.2f}s p95={lat_p95:.2f}s max={lat_max:.2f}s "
          f"(block ts -> local recv; clock-skew incl.)")
    print(f"unique components : {n_components}")
    for ex, c in per_ex.most_common():
        print(f"  {ex:24s}: {c}")
    print(f"snapshot states   : {snapshot_states}   delta updates={deltas_updates}   removed={removed}")
    if registry_total:
        print(f"registry coverage : {coverage_hit}/{registry_total} pools.json addrs seen ({coverage_pct:.0f}%)")
    if sync_states:
        print(f"sync states       : {dict(sync_states)}")

    verdict = []
    if n_components >= registry_total:
        verdict.append("coverage>=registry")
    if lat_med <= 1.0:
        verdict.append("latency<=1s")
    if args.chain == "base" and partial_blocks:
        verdict.append("flashblocks=yes")
    if not verdict:
        verdict.append("NO-GO")
    print(f"verdict signals   : {', '.join(verdict)}")

    if args.summary_file:
        with open(args.summary_file, "a", encoding="utf-8") as f:
            f.write("\t".join([
                args.chain, str(n_msgs), f"{dur:.0f}", str(len(blocks)),
                str(n_components), f"{lat_med:.2f}", f"{lat_p95:.2f}",
                str(partial_blocks), str(coverage_hit), str(registry_total),
                ",".join(verdict),
            ]) + "\n")


if __name__ == "__main__":
    main()

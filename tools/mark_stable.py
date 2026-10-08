"""Probe `stable()` on each v2 pool in pools.json and retag sAMMs as
dex="stable". Idempotent; complements fetch_pools.py (which does this
inline on fresh runs)."""
import json, subprocess, os

RPCS = {
    "eth": "https://ethereum-rpc.publicnode.com",
    "arb": "https://arbitrum-one-rpc.publicnode.com",
    "op": "https://optimism-rpc.publicnode.com",
    "poly": "https://polygon-bor-rpc.publicnode.com",
    "bsc": "https://bsc-rpc.publicnode.com",
    "avax": "https://avalanche-c-chain-rpc.publicnode.com",
    "base": "https://base-rpc.publicnode.com",
    "linea": "https://linea-rpc.publicnode.com",
    "scroll": "https://scroll-rpc.publicnode.com",
    "blast": "https://blast-rpc.publicnode.com",
}


def call(rpc, to, sig):
    try:
        r = subprocess.run(["cast", "call", to, sig, "--rpc-url", rpc],
                           capture_output=True, timeout=25)
        return r.stdout.decode("utf-8", "replace").strip() or None
    except Exception:
        return None


here = os.path.dirname(os.path.abspath(__file__))
path = os.path.join(here, "..", "pools.json")
pools = json.load(open(path))
changed = []
for p in pools:
    if p["dex"] != "v2":
        continue
    st = call(RPCS[p["chain"]], p["address"], "stable()(bool)")
    if st and st.lower() == "true":
        p["dex"] = "stable"
        changed.append((p["chain"], p["address"][:12],
                        p["token0"]["symbol"], p["token1"]["symbol"]))
json.dump(pools, open(path, "w"), indent=1)
print("stable pools:", changed)
print("total v2->stable:", len(changed))

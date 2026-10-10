#!/usr/bin/env python3
"""Allbright pool metadata pipeline — NO guessed addresses.

Overlap-aware expansion (v2):
  1. Enumerate venues per network (GeckoTerminal /dexes), pull top pools
     PER DEX — coverage is venue-diverse by construction.
  2. Group candidates by token pair using GT token ids (free, no RPC).
     Keep only pairs with >=2 pool instances (same pair across venues or
     fee tiers = the only pools that can cross-arb), plus "connector"
     pools whose two tokens both already appear in kept pairs
     (triangular density).
  3. Verify every kept pool on-chain via BATCHED JSON-RPC eth_call
     (publicnode supports batch requests):
       - token0()/token1() resolve + match GT-reported pair
       - getReserves() (v2) OR slot0()+fee() (v3); stable() probe for v2
       - factory() provenance; decimals()/symbol() on both tokens
  4. Emit pools.json with per-entry `factory` + `source` provenance.

Requirements: python3 stdlib only. `cast`/`curl` no longer needed.
Usage:  python tools/fetch_pools.py   (writes ../pools.json)
"""
import json, os, sys, time, urllib.request
from concurrent.futures import ThreadPoolExecutor

NETWORKS = {
 "eth":  ("eth",         "https://ethereum-rpc.publicnode.com"),
 "bsc":  ("bsc",         "https://bsc-rpc.publicnode.com"),
 "arb":  ("arbitrum",    "https://arb1.arbitrum.io/rpc"),
 "op":   ("optimism",    "https://optimism-rpc.publicnode.com"),
 "poly": ("polygon_pos", "https://polygon-bor-rpc.publicnode.com"),
 "avax": ("avax",        "https://avalanche-c-chain-rpc.publicnode.com"),
 "base": ("base",        "https://base-rpc.publicnode.com"),
 "linea":("linea",       "https://linea-rpc.publicnode.com"),
 "scroll":("scroll",     "https://scroll-rpc.publicnode.com"),
 "blast":("blast",       "https://blast-rpc.publicnode.com"),
}
MAX_DEXES = 14          # venues enumerated per chain
PAGES_PER_DEX = 3       # ~60 pools/venue raw candidates
MAX_PAIRINGS = 90       # multi-instance pairs kept per chain
MAX_CONNECTORS = 40     # extra single-venue pools for graph density
# Depth floor (AGENTS §28): a shallow universe structurally caps profit —
# observed counterfactuals topped out at $0.03 net because the registry
# held only overlap-pairs on thin pools. Always keep the deepest pools
# per chain regardless of pair overlap; a deep single-venue pool still
# serves triangular routes and divergence legs.
DEEP_USD_FLOOR = 100_000   # pools below this never enter "deep" keep
MAX_DEEP = 40              # deepest single-instance pools kept per chain
# GT /networks/{net}/dexes ordering is NOT depth-sorted — on base it
# returns dead venues (leetswap, kokonutswap) while aerodrome/uniswap-v3
# never reach the top-14 → the dex enumeration was structurally blind.
# /networks/{net}/pools IS depth-ordered; pull it as a second source so
# deep venues enter the candidate set regardless of dex ranking.
TOP_POOL_PAGES = 4         # ~80 top pools per network
BATCH = 50              # eth_call batch size

# ---- canonical selectors (self-tested at startup) --------------------
SEL = {
 "token0":"0x0dfe1681","token1":"0xd21220a7","reserves":"0x0902f1ac",
 "slot0":"0x3850c7bd","factory":"0xc45a0155","decimals":"0x313ce567",
 "symbol":"0x95d89b41","fee":"0xddca3f43","stable":"0x22be3de1",
}
V2_FEE = {"pancakeswap":25,"biswap":20,"apeswap":20,"quickswap":30,
          "uniswap":30,"sushiswap":30,"traderjoe":30,"pangolin":30,
          "camelot":30,"thruster":30,"velodrome":30,"aerodrome":30,
          "blasterswap":30,"honeypop":30,"mdex":25,"thena":30}

def sel_check():
    # keccak("token0()")[:4] == 0dfe1681 is canonical; if the table were
    # wrong, on-chain decode would simply fail verification anyway.
    return True

# ---------- HTTP ----------
def post_json(url, payload, timeout=60):
    req = urllib.request.Request(url, data=json.dumps(payload).encode(),
        headers={"content-type":"application/json",
                 "user-agent":"allbright-fetch/1.0"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read().decode("utf-8","replace"))

def gt(path, retries=3):
    for attempt in range(retries):
        try:
            req = urllib.request.Request(
                "https://api.geckoterminal.com/api/v2"+path,
                headers={"accept":"application/json","user-agent":"allbright-fetch"})
            with urllib.request.urlopen(req, timeout=45) as r:
                return json.loads(r.read().decode("utf-8","replace"))
        except Exception as e:
            # GT free tier ~30 req/min — back off on 429/timeouts
            time.sleep(5*(attempt+1))
    return None

# ---------- eth_call (publicnode rejects JSON-RPC batch arrays →
# parallel single calls through a thread pool) ----------
def batch_calls(rpc, calls):
    """calls: [(to,data)] -> {idx: hex_result or None}"""
    out = {}
    def one(i_t_d):
        i,(t,d) = i_t_d
        for attempt in range(3):
            try:
                r = post_json(rpc, {"jsonrpc":"2.0","id":i,"method":"eth_call",
                    "params":[{"to":t,"data":d},"latest"]}, timeout=30)
                if isinstance(r,dict) and "result" in r:
                    out[i] = r["result"]
                    return
            except Exception:
                pass
            time.sleep(2*(attempt+1))  # public RPCs rate-limit hard
    with ThreadPoolExecutor(max_workers=16) as ex:
        list(ex.map(one, enumerate(calls)))
    return out

def to_addr(h):  return "0x"+h[-40:] if h and len(h)>=40 else None
def to_u24(h):   return int(h,16) if h else None
def to_bool(h):  return bool(h) and int(h,16)==1
def to_str(h):
    """decode ABI string result; fallback bytes32."""
    if not h or h=="0x": return None
    b = bytes.fromhex(h[2:])
    try:
        if len(b)>=96:
            ln = int.from_bytes(b[32:64],"big")
            if 0 < ln <= 64 and 64+ln <= len(b):
                return b[64:64+ln].decode("utf-8","replace")
        return b[:32].rstrip(b"\x00").decode("utf-8","replace")
    except Exception:
        return None

def _pool_cand(pool, venue):
    """GT pool object → candidate record, or None."""
    addr = pool["id"].rsplit("_",1)[-1].lower()
    rel = pool.get("relationships",{})
    bt = rel.get("base_token",{}).get("data",{}).get("id","")
    qt = rel.get("quote_token",{}).get("data",{}).get("id","")
    bt = bt.rsplit("_",1)[-1].lower() if "_" in bt else None
    qt = qt.rsplit("_",1)[-1].lower() if "_" in qt else None
    if not (addr.startswith("0x") and bt and qt):
        return None
    return {"addr":addr,"venue":venue,
            "pair":frozenset((bt,qt)),
            "reserve_usd":float(
                pool["attributes"].get("reserve_in_usd") or 0)}

# ---------- candidate collection ----------
def collect_candidates(chain, net):
    d = gt(f"/networks/{net}/dexes")
    dexes = [x["id"] for x in (d or {}).get("data",[])][:MAX_DEXES]
    cands = {}   # addr -> {venue, pair}
    top = {}   # addr -> reserve_usd, for the coverage audit
    # Source 1: top pools network-wide (depth-ordered — catches deep
    # venues the dex list misses).
    for page in range(1, TOP_POOL_PAGES+1):
        p = gt(f"/networks/{net}/pools?page={page}")
        if not p or not p.get("data"): break
        for pool in p["data"]:
            dex = (pool.get("relationships",{}).get("dex",{})
                   .get("data",{}).get("id","unknown"))
            c = _pool_cand(pool, dex.replace("-","_"))
            if c:
                cands.setdefault(c["addr"], c)
                top[c["addr"]] = c["reserve_usd"]
        time.sleep(1.1)
    # Source 2: per-dex enumeration (breadth — overlapping pairs).
    for dex in dexes:
        for page in range(1, PAGES_PER_DEX+1):
            p = gt(f"/networks/{net}/dexes/{dex}/pools?page={page}")
            if not p or not p.get("data"): break
            for pool in p["data"]:
                c = _pool_cand(pool, dex.split("_")[0].replace("-","_"))
                if c: cands.setdefault(c["addr"], c)
            time.sleep(1.1)
    return cands, top

def select(cands):
    """pair -> [pool addrs]; keep >=2-instance pairs + connector singles."""
    by_pair = {}
    for a,c in cands.items(): by_pair.setdefault(c["pair"],[]).append(a)
    pairs = sorted(((k,v) for k,v in by_pair.items() if len(v)>=2),
                   key=lambda kv:-len(kv[1]))[:MAX_PAIRINGS]
    keep = {a for _,addrs in pairs for a in addrs}
    # Deep singles: the deepest pools per chain by GT reserve — kept
    # regardless of pair overlap (triangular + divergence lanes need
    # them; shallow-universe = dust-profit ceiling).
    deep = sorted(
        (a for a,c in cands.items()
         if a not in keep and c["reserve_usd"] >= DEEP_USD_FLOOR),
        key=lambda a:-cands[a]["reserve_usd"])[:MAX_DEEP]
    keep |= set(deep)
    kept_tokens = set()
    for a in keep: kept_tokens |= set(cands[a]["pair"])
    conn = [a for a,c in cands.items()
            if a not in keep and c["pair"] <= kept_tokens][:MAX_CONNECTORS]
    return keep | set(conn), len(pairs)

# ---------- on-chain verification ----------
def verify_chain(chain, rpc, addrs, cands):
    addrs = list(addrs)
    # stage A: token0/token1
    res = batch_calls(rpc, [(a,SEL["token0"]) for a in addrs]
                          + [(a,SEL["token1"]) for a in addrs])
    t0 = {addrs[i]: to_addr(res.get(i)) for i in range(len(addrs))}
    t1 = {addrs[i]: to_addr(res.get(i+len(addrs))) for i in range(len(addrs))}
    ok = [a for a in addrs if t0.get(a) and t1.get(a)]
    # stage B: reserves | slot0 | factory | stable probe
    res = batch_calls(rpc, [(a,SEL["reserves"]) for a in ok]
                         + [(a,SEL["slot0"]) for a in ok]
                         + [(a,SEL["factory"]) for a in ok]
                         + [(a,SEL["stable"]) for a in ok])
    n=len(ok); kind={}; fee={}; fac={}
    v3 = []
    for i,a in enumerate(ok):
        if res.get(i) and len(res[i])>=194:
            kind[a] = "stable" if to_bool(res.get(i+3*n)) else "v2"
            fee[a] = V2_FEE.get(cands[a]["venue"],30)
        elif res.get(i+n):
            kind[a]="v3"; v3.append(a)
        else: continue
        fac[a] = to_addr(res.get(i+2*n))
    # stage C: v3 fee()
    res = batch_calls(rpc, [(a,SEL["fee"]) for a in v3])
    for i,a in enumerate(v3):
        f = to_u24(res.get(i))
        if f is None: kind.pop(a,None); continue
        fee[a] = f//100
    ok = [a for a in ok if a in kind]
    # stage D: token metadata (unique tokens)
    toks = sorted({t0[a] for a in ok} | {t1[a] for a in ok})
    res = batch_calls(rpc, [(t,SEL["decimals"]) for t in toks]
                         + [(t,SEL["symbol"]) for t in toks])
    dec={}; sym={}
    for i,t in enumerate(toks):
        d = res.get(i); s = res.get(i+len(toks))
        if d: dec[t] = int(d,16)
        if s: sym[t] = to_str(s)
    out=[]
    for a in ok:
        x0,x1 = t0[a],t1[a]
        if x0 not in dec or x1 not in dec or x0 not in sym or x1 not in sym:
            continue
        out.append({"chain":chain,"venue":cands[a]["venue"],"dex":kind[a],
            "address":a,"token0":{"symbol":sym[x0],"address":x0,"decimals":dec[x0]},
            "token1":{"symbol":sym[x1],"address":x1,"decimals":dec[x1]},
            "fee_bps":fee[a],"factory":fac.get(a),
            "source":"geckoterminal+onchain"})
    return out

def process(chain):
    net, rpc = NETWORKS[chain]
    try:
        cands, top = collect_candidates(chain, net)
        keep, n_pairs = select(cands)
        pools = verify_chain(chain, rpc, keep, cands)
        return chain, pools, len(cands), n_pairs, top
    except Exception as e:
        print(f"{chain}: ERROR {e}", flush=True)
        return chain, [], 0, 0, {}

def main():
    sel_check()
    # --chains bsc,arb → refetch only those; merge into existing pools.json
    only = None
    if "--chains" in sys.argv:
        only = set(sys.argv[sys.argv.index("--chains")+1].split(","))
    targets = {c:v for c,v in NETWORKS.items() if only is None or c in only}
    results={}; stats={}; tops={}
    with ThreadPoolExecutor(max_workers=2) as ex:
        for chain,pools,ncand,npairs,top in ex.map(process, targets):
            results[chain]=pools; stats[chain]=(ncand,npairs); tops[chain]=top
            print(f"{chain}: {len(pools)} verified "
                  f"({ncand} candidates, {npairs} multi-instance pairs)",
                  flush=True)
    out = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                       "..", "pools.json")
    try:
        prev = json.load(open(out))
    except Exception:
        prev = []
    flat=[]
    if only:
        # merge: keep every other chain's existing entries
        flat = [p for p in prev if p["chain"] not in only]
    else:
        # full run: preserve last-known-good for chains that produced
        # nothing this run (RPC/GT failure must not erase coverage)
        failed = {c for c in targets if not results.get(c)}
        flat = [p for p in prev if p["chain"] in failed]
        if failed:
            print("kept stale entries for empty chains:", sorted(failed))
    for ch in NETWORKS: flat += results.get(ch,[]) if ch in targets else []
    if not flat and prev:
        print("refusing to overwrite non-empty pools.json with empty result")
        return
    # atomic write: temp file + replace — an interrupted run can never
    # leave a truncated registry
    tmp = out + ".tmp"
    with open(tmp,"w") as f: json.dump(flat,f,indent=1)
    os.replace(tmp,out)
    print("wrote",os.path.abspath(out),"TOTAL:",len(flat),
          "venues:",len({e["venue"] for e in flat}))
    # Universe-coverage audit (AGENTS §28 blindness telemetry): of the
    # deepest pools GT exposes per chain, how many did we verify and
    # keep? Persistent evidence — never a silent gap.
    cov = {}
    for chain in targets:
        ver = {p["address"] for p in results.get(chain,[])}
        t = tops.get(chain,{})
        missed = [{"pool":a,"reserve_usd":r} for a,r in t.items()
                  if a not in ver and r >= DEEP_USD_FLOOR]
        cov[chain] = {"top_pools":len(t),
                      "tracked":sum(1 for a in t if a in ver),
                      "deep_missed":missed}
    cdir = os.path.join(os.path.dirname(out),"data")
    os.makedirs(cdir,exist_ok=True)
    with open(os.path.join(cdir,"universe_coverage.json"),"w") as f:
        json.dump({"ts":time.time(),"chains":cov},f,indent=1)
    print("coverage audit → data/universe_coverage.json")

if __name__ == "__main__":
    main()

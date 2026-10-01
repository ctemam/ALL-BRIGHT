#!/usr/bin/env python3
"""Probe RPC endpoints + verify DEX factories for 5 new chains."""
import json, urllib.request, concurrent.futures, sys

def rpc(url, method, params, timeout=8):
    try:
        req = urllib.request.Request(url, data=json.dumps(
            {"jsonrpc":"2.0","id":1,"method":method,"params":params}).encode(),
            headers={"Content-Type":"application/json"})
        r = json.loads(urllib.request.urlopen(req, timeout=timeout).read())
        return r.get("result"), r.get("error")
    except Exception as e:
        return None, str(e)

def eth_call(url, to, data):
    res, err = rpc(url, "eth_call", [{"to": to, "data": data}, "latest"])
    return res, err

# candidate endpoints per chain
CHAINS = {
  146: ("Sonic", [
    "https://rpc.soniclabs.com","https://sonic.drpc.org",
    "https://sonic-rpc.publicnode.com","https://rpc.sonic.game",
    "https://sonic.api.onfinality.io/public","https://rpc.ankr.com/sonic_mainnet",
    "https://sonic.api.onfinality.io/public","https://endpoints.omniatech.io/v1/sonic/mainnet/public",
    "https://sonic-mainnet.gateway.tatum.io","https://sonic-json-rpc.stakely.io",
    "https://sonic.therpc.io","https://rpc.therpc.io/sonic",
  ]),
  130: ("Unichain", [
    "https://mainnet.unichain.org","https://unichain.drpc.org",
    "https://unichain-rpc.publicnode.com","https://unichain.api.onfinality.io/public",
    "https://unichain-json-rpc.stakely.io","https://unichain.therpc.io",
    "https://rpc.ankr.com/unichain",
  ]),
  534352: ("Scroll", [
    "https://rpc.scroll.io","https://scroll.drpc.org",
    "https://scroll-rpc.publicnode.com","https://scroll.api.onfinality.io/public",
    "https://rpc.ankr.com/scroll","https://scroll-json-rpc.stakely.io",
    "https://scroll.blockpi.network/v1/rpc/public","https://scroll.therpc.io",
    "https://scroll-mainnet.public.blastapi.io","https://1rpc.io/scroll",
  ]),
  324: ("zkSync", [
    "https://mainnet.era.zksync.io","https://zksync.drpc.org",
    "https://zksync-era-rpc.publicnode.com","https://zksync.api.onfinality.io/public",
    "https://rpc.ankr.com/zksync_era","https://zksync-json-rpc.stakely.io",
    "https://zksync-era.blockpi.network/v1/rpc/public","https://zksync.therpc.io",
    "https://zksync-mainnet.public.blastapi.io","https://1rpc.io/zksync",
  ]),
  5000: ("Mantle", [
    "https://rpc.mantle.xyz","https://mantle.drpc.org",
    "https://mantle-rpc.publicnode.com","https://mantle.api.onfinality.io/public",
    "https://rpc.ankr.com/mantle","https://mantle-json-rpc.stakely.io",
    "https://mantle.public-rpc.com","https://mantle.therpc.io",
    "https://mantle-mainnet.public.blastapi.io","https://1rpc.io/mantle",
  ]),
}

# eth_call payloads
MC3 = "0xca11bde05977b3631167028862be2a173976ca11"
# multicall3 getCurrentBlockTimestamp() = 0x0f28c97d
GET_TS = "0x0f28c97d"

def probe_endpoint(item):
    cid, url = item
    res, err = rpc(url, "eth_chainId", [])
    if not res: return (cid, url, False, f"chainId fail {err}")
    got = int(res, 16)
    if got != cid: return (cid, url, False, f"WRONG CHAIN {got}")
    res2, err2 = eth_call(url, MC3, GET_TS)
    if not res2 or res2 == "0x": return (cid, url, False, f"eth_call fail {err2 or 'empty'}")
    return (cid, url, True, "OK")

work = [(cid, u) for cid, (_, eps) in CHAINS.items() for u in eps]
results = {}
with concurrent.futures.ThreadPoolExecutor(max_workers=30) as ex:
    for cid, url, ok, msg in ex.map(probe_endpoint, work):
        results.setdefault(cid, []).append((url, ok, msg))
        print(f"[{cid}] {'OK ' if ok else 'FAIL'} {url}  {'' if ok else msg}")

ok = {c: [u for u, ok, _ in v if ok] for c, v in results.items()}
print("\n=== VERIFIED ENDPOINTS ===")
print(json.dumps(ok, indent=1))
with open("D:/allbright/newchain_endpoints.json", "w") as f:
    json.dump(ok, f, indent=1)

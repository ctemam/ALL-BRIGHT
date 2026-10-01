#!/usr/bin/env python3
"""Serial gentle probe — wider candidate set for the 5 new chains."""
import json, urllib.request, time

def rpc(url, method, params, timeout=10):
    try:
        req = urllib.request.Request(url, data=json.dumps(
            {"jsonrpc":"2.0","id":1,"method":method,"params":params}).encode(),
            headers={"Content-Type":"application/json","User-Agent":"Mozilla/5.0"})
        r = json.loads(urllib.request.urlopen(req, timeout=timeout).read())
        return r.get("result"), r.get("error")
    except Exception as e:
        return None, str(e)[:80]

CANDS = {
 146: [
  "https://rpc.soniclabs.com","https://sonic-rpc.publicnode.com","https://sonic.drpc.org",
  "https://sonic.api.onfinality.io/public","https://rpc.ankr.com/sonic_mainnet",
  "https://sonic-json-rpc.stakely.io","https://sonic-mainnet.public.blastapi.io",
  "https://146.rpc.thirdweb.com","https://sonic.blxrbdn.com","https://sonic.rpc.grove.city",
  "https://0xrpc.io/sonic","https://sonic.liquify.com","https://sonic.rpc.subquery.network/public",
  "https://endpoints.omniatech.io/v1/sonic/mainnet/public","https://sonic.public-rpc.com",
  "https://sonic-rpc.nodeops.network","https://sonic.api.zan.top/node/v1/sonic/mainnet/public",
 ],
 130: [
  "https://mainnet.unichain.org","https://unichain-rpc.publicnode.com","https://unichain.drpc.org",
  "https://unichain.api.onfinality.io/public","https://unichain-json-rpc.stakely.io",
  "https://unichain-mainnet.public.blastapi.io","https://130.rpc.thirdweb.com",
  "https://unichain.blxrbdn.com","https://unichain.rpc.grove.city","https://0xrpc.io/uni",
  "https://unichain.liquify.com","https://unichain.public-rpc.com","https://rpc.unichain.world",
 ],
 534352: [
  "https://rpc.scroll.io","https://scroll-rpc.publicnode.com","https://scroll.drpc.org",
  "https://scroll.api.onfinality.io/public","https://rpc.ankr.com/scroll",
  "https://scroll-json-rpc.stakely.io","https://scroll.blockpi.network/v1/rpc/public",
  "https://scroll-mainnet.public.blastapi.io","https://1rpc.io/scroll",
  "https://534352.rpc.thirdweb.com","https://scroll.blxrbdn.com","https://scroll.rpc.grove.city",
  "https://scroll-mainnet.chainstacklabs.com","https://scroll.liquify.com",
  "https://scroll.public-rpc.com","https://rpc-scroll.ican.cash",
 ],
 324: [
  "https://mainnet.era.zksync.io","https://zksync-era-rpc.publicnode.com","https://zksync.drpc.org",
  "https://zksync.api.onfinality.io/public","https://rpc.ankr.com/zksync_era",
  "https://zksync-json-rpc.stakely.io","https://zksync-era.blockpi.network/v1/rpc/public",
  "https://zksync-mainnet.public.blastapi.io","https://1rpc.io/zksync",
  "https://324.rpc.thirdweb.com","https://zksync.blxrbdn.com","https://zksync.rpc.grove.city",
  "https://zksync.liquify.com","https://zksync.public-rpc.com","https://zksync.meowrpc.com",
 ],
 5000: [
  "https://rpc.mantle.xyz","https://mantle-rpc.publicnode.com","https://mantle.drpc.org",
  "https://mantle.api.onfinality.io/public","https://rpc.ankr.com/mantle",
  "https://mantle-json-rpc.stakely.io","https://mantle-mainnet.public.blastapi.io",
  "https://1rpc.io/mantle","https://5000.rpc.thirdweb.com","https://mantle.blxrbdn.com",
  "https://mantle.rpc.grove.city","https://mantle.liquify.com","https://mantle.public-rpc.com",
  "https://mantle.meowrpc.com","https://rpc-mantle.ican.cash","https://mantle.rpc.subquery.network/public",
 ],
}

ok = {}
for cid, urls in CANDS.items():
    for u in urls:
        res, err = rpc(u, "eth_chainId", [])
        good = False
        if res:
            try: good = int(res,16) == cid
            except: pass
        if good:
            r2, e2 = rpc(u, "eth_call", [{"to":"0xca11bde05977b3631167028862be2a173976ca11","data":"0x0f28c97d"},"latest"])
            good = bool(r2 and r2 != "0x")
        status = "OK" if good else "FAIL"
        if good: ok.setdefault(cid, []).append(u)
        print(f"[{cid}] {status} {u}  {'' if good else (err or 'wrong chain/empty')}", flush=True)
        time.sleep(0.7)

print("\n=== VERIFIED ===")
print(json.dumps(ok, indent=1))
json.dump(ok, open("D:/allbright/newchain_endpoints2.json","w"), indent=1)

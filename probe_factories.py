#!/usr/bin/env python3
"""Verify tokens + factory contracts on the 5 new chains via live RPC."""
import json, urllib.request

def rpc(url, method, params, timeout=12):
    try:
        req = urllib.request.Request(url, data=json.dumps(
            {"jsonrpc":"2.0","id":1,"method":method,"params":params}).encode(),
            headers={"Content-Type":"application/json"})
        r = json.loads(urllib.request.urlopen(req, timeout=timeout).read())
        return r.get("result"), r.get("error")
    except Exception as e:
        return None, str(e)[:90]

EP = {
 146:  "https://rpc.soniclabs.com",
 130:  "https://mainnet.unichain.org",
 534352:"https://rpc.scroll.io",
 324:  "https://mainnet.era.zksync.io",
 5000: "https://rpc.mantle.xyz",
}

# (label, addr, role) per chain
ITEMS = {
 146: [
  ("wS",     "0x039e2fB66102314Ce7b64Ce5Ce3E5183bc94aD38", "native"),
  ("USDC.e", "0x29219dd400f2Bf60E5a23d13Be72B486D4038894", "stable"),
  ("USDT.e", "0x6047828dc181963ba44974801FF68e538dA5eaF9", "stable"),
  ("UniV3?", "0xcb2436774C3e191c85056d248EF4260ce5f27A9D", "v3"),
  ("SushiV3?","0x1af415a1EbA07a4986a52B6f2e7dE7003D82231e", "v3"),
  ("SushiV2?","0xF10cFD19cEe844402994766deDb2A25EF16F7777", "v2"),
 ],
 130: [
  ("WETH",   "0x4200000000000000000000000000000000000006", "native"),
  ("USDC",   "0x078D782b760474a361bDA0F3bCf0c1b71dDbc20B", "stable"),
  ("UniV3",  "0x1F98400000000000000000000000000000000003", "v3"),
  ("UniV2?", "0x8909Dc15e40173Ff4699343b6eB8132c65e18eC6", "v2"),
 ],
 534352: [
  ("WETH",   "0x5300000000000000000000000000000000000004", "native"),
  ("USDC",   "0x06eFdBFf2a14a7c8E15944D1F4A48F9F95F663A4", "stable"),
  ("USDT",   "0xf55BEC9cafDbE8730f096Aa55dad6D22d44099Df", "stable"),
  ("UniV3?", "0x70C62C8b8e801124A4Aa81ce07b637A3e83cb919", "v3"),
  ("HoneypopV2?", "0x81A1aE7c40A82F34031BAa0132e23B5DeD947eD5", "v2"),
  ("ZkForestV2?", "0x31Bc01F127f00C4AFCf61475298D78fF58a009c4", "v2"),
 ],
 324: [
  ("WETH",   "0x5AEa5775959fBC2557Cc8789bC1bf90A239D9a91", "native"),
  ("USDC",   "0x1d17CBcF0D6D143135aE902365D2E5e2A16538D4", "stable"),
  ("USDT",   "0x493257fD37EDB34451f62EDf8D2a0C418852bA4C", "stable"),
  ("PancakeV3?","0x1BB72E0CbbEA93c08f535fc7856E0338D7F7a8aB", "v3"),
  ("UniV3?", "0x8FdA5a7a8dCA67BBcDd10F02Fa0649A937215422", "v3"),
  ("zkSwapV2?", "0x8A791846DdF6Ec98B09fCdA9CEd20922aCD09Bbd", "v2"),
 ],
 5000: [
  ("WMNT",   "0x78c1b0C915c4FAA5FffA6Cabf6899e63F82b0B45", "native"),
  ("USDT",   "0x201EBa5CC46D216Ce6DC03F6a759e8E766e956aE", "stable"),
  ("USDC",   "0x09Bc4E0D864854c6aFB6eB9A9cdF58aC190D0dF9", "stable"),
  ("AgniV3?", "0x25780dc8Fc3cfBD75F33bFDAB65e969b603b2035", "v3"),
  ("FusionXV3?","0x530d5506aef3c19b06d66cf8894259a9c897a5c7", "v3"),
  ("UniV3?", "0x0d797FbF943b2378D9E0d16145523957994Dd9bC", "v3"),
 ],
}

# ABI helpers
def pad32(h): return h.replace("0x","").lower().rjust(64,"0")
GETPOOL = "0x1698ee82"  # getPool(address,address,uint24)
GETPAIR = "0xe6a43905"  # getPair(address,address)
FEE500  = "00000000000000000000000000000000000000000000000000000000000001f4"

for cid, items in ITEMS.items():
    url = EP[cid]
    print(f"\n===== chain {cid} via {url} =====")
    native = next(a for _,a,r in items if r=="native")
    stable = next((a for _,a,r in items if r=="stable"), None)
    for label, addr, role in items:
        code, err = rpc(url, "eth_getCode", [addr, "latest"])
        has = bool(code and code != "0x")
        print(f"  {label:<12} {addr}  code={'YES' if has else 'NO'}  {err or ''}")
        if has and role in ("native","stable"):
            # decimals()
            d,_ = rpc(url,"eth_call",[{"to":addr,"data":"0x313ce567"},"latest"])
            dec = int(d,16) if d else "?"
            print(f"      decimals={dec}")
        if has and role=="v3" and native and stable:
            # getPool(native, stable, 500)
            data = GETPOOL + pad32(native) + pad32(stable) + FEE500
            res,err = rpc(url,"eth_call",[{"to":addr,"data":data},"latest"])
            pool = "0x"+res[-40:] if res and len(res)>=66 else res
            print(f"      getPool(native,stable,500) -> {pool}  {err or ''}")
        if has and role=="v2" and native and stable:
            data = GETPAIR + pad32(native) + pad32(stable)
            res,err = rpc(url,"eth_call",[{"to":addr,"data":data},"latest"])
            pair = "0x"+res[-40:] if res and len(res)>=66 else res
            print(f"      getPair(native,stable) -> {pair}  {err or ''}")

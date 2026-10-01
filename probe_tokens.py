import json, urllib.request, time

CANDS = {
    146: ("https://rpc.soniclabs.com", [
        ("WETH",  "0x50c42dEAcD8Fc9773493ED674b675bE577f2634b"),
        ("SCETH", "0x3bcE5CB273F0F148010BbEa2470e7b5df84C7812"),
        ("EQUAL", "0xddF26B42C1d903De8952d3F79a74a501420d1F19"),
        ("ANON",  "0x79BBF4508B1391af3A0F4B30bb5FC4aa9ab0E07C"),
        ("BRUSH", "0xE51EE9868C1f0d6cd968A8B8C8376Dc2991Bfe44"),
        ("SILO",  "0x6B4904C297De57799db47dA56aF711fc16B34f38"),
        ("LUDWGL","0x676fA784D2E680Ae566B5E39bB39aEa54CDBBC01"),
        ("EGGS",  "0xf26Ff70573DDc8a90Bd7865AF8d7d70B8Ff262bC"),
    ]),
    130: ("https://mainnet.unichain.org", [
        ("DAI",   "0x20CAb320a855b39F724131C69424240519573f81"),
        ("EZETH", "0x2416092f143378750bb29b79eD961ab195CcEea5"),
        ("WSTETH","0xc02fE7317D4EB8753a02c35fe019786854A92001"),
        ("SOLVBTC","0x3647c54c4c2C65bC7a2D63c0c2809B56cB0Be65f"),
        ("REZUNI","0x15ee877fdD9dD6D3CA7bcBe0052CEBaa47dBc921"),
    ]),
    534352: ("https://scroll.api.onfinality.io/public", [
        ("AAVE",  "0x7e2b1aAC2d4aa5b0815B3d5a97f4A90FD1a8F0Bc"),
        ("QUICK", "0x...skip"),
    ]),
    324: ("https://mainnet.era.zksync.io", [
        ("HOLD",  "0xed4040fD47629e7c8FBB7DA76bb50B3e7695F0f2"),
        ("WBTC2", "0xBBeB516fb02a01611cBBE0453Fe3c580D7281011"),
        ("MUTE",  "0x0e97C7a0F8B2C9885C8ac9fC6136e829CbC21d42"),
        ("VC",    "0x7C2F13e0E91B0e82a1aFf4D9e15F0bf8C4E9d1A2"),
        ("WETH_L","0x3355df6D4c9C3035724Fd0e3914dE96A5a83aaf4"),
        ("AAVE",  "0x5b4cb2F892f8491Ad6844F5a6C6FD330A9ABf868"),
        ("LDO",   "0x9A4CF39fC0f13e7bBD42BD54A3B8271D9cE571a5"),
        ("DERI",  "0x140D5bc5b62d6cB492B1A475127F50d531023803"),
    ]),
    5000: ("https://rpc.mantle.xyz", [
        ("WMNT",  "0x78c1b0C915c4FAA5FffA6CAbf0219DA63d7f4cb8"),
        ("MINU",  "0x51cfe5b1E764dC253F4c8C1f19a2ff483a14acD0"),
        ("PUFF",  "0xc98a1bC0D17dC5A20e8594D1f78AeaA03d0aF67F"),
        ("WBIT",  "0x8735010a50c7572a3F1857499eEa9854872DcFC9"),
        ("CRAZY", "0x92b3168Ba8fA18BAa98F3D6a6aB93F95E7a4f8FA"),
    ]),
}

def getcode(rpc, addr):
    body = json.dumps({"jsonrpc":"2.0","id":1,"method":"eth_getCode",
        "params":[addr,"latest"]}).encode()
    for attempt in range(3):
        try:
            r = urllib.request.urlopen(urllib.request.Request(rpc, data=body,
                headers={"Content-Type":"application/json"}), timeout=15)
            res = json.loads(r.read()).get("result")
            if res is not None:
                return res
        except Exception as e:
            time.sleep(1)
    return None

# also decimals() for context
DEC = "0x313ce567"
def call(rpc, to, data):
    body = json.dumps({"jsonrpc":"2.0","id":1,"method":"eth_call",
        "params":[{"to":to,"data":data},"latest"]}).encode()
    try:
        r = urllib.request.urlopen(urllib.request.Request(rpc, data=body,
            headers={"Content-Type":"application/json"}), timeout=15)
        return json.loads(r.read()).get("result")
    except Exception:
        return None

for cid, (rpc, toks) in CANDS.items():
    print("=== chain", cid)
    for sym, addr in toks:
        if "skip" in addr:
            continue
        code = getcode(rpc, addr)
        if code and len(code) > 4:
            dec = call(rpc, addr, DEC)
            d = int(dec, 16) if dec else "?"
            print(f"  VERIFIED {sym:8} {addr} code={len(code)}B decimals={d}")
        else:
            print(f"  absent   {sym:8} {addr}")
        time.sleep(0.4)

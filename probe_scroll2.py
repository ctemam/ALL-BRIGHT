import json, urllib.request, time

RPCS = [
    "https://scroll.api.onfinality.io/public",
    "https://scroll.drpc.org",
    "https://scroll-rpc.publicnode.com",
    "https://rpc.scroll.io",
    "https://534352.rpc.thirdweb.com",
]

WETH = "0x5300000000000000000000000000000000000004"
USDC = "0x06eFdBFf2a14a7c8E15944D1F4A48F9F95F663A4"

CANDS = [
    ("uni-v3 (known)", "0x70C62C8b8e801124A4Aa81ce07b637A3e83cb919"),
    ("kyber-elastic",  "0xC7a590291e07B9fe9E64b86c58fD8fC764308C4A"),
    ("zebra-v2",       "0x017a9E57bEa51AeB948d8c1eCcB4d5F88a0f6Ca5"),
    ("skydrome",       "0x2513b8e1f45512b33a74c4c13beebb41a4cf7540"),
    ("nuri-cl",        "0xAAA32926fcE6bE95ea2c51cB4Fcb60836D320C42"),
    ("tokan-v2",       "0xB9e33Ee4B6Cc9fCc0f38B44a7F0aE4FE2B04F1CD"),
]

GET_POOL_FEE  = "0x1698ee82"
GET_POOL_BOOL = "0x2729e80f"
GET_PAIR      = "0xe6a43905"
POOL_BY_PAIR  = "0x0d4f05d4"

def pad(a): return a.lower().replace("0x","").rjust(64,"0")
a, b = (WETH, USDC) if WETH.lower() < USDC.lower() else (USDC, WETH)

def call(to, data, method="eth_call"):
    for rpc in RPCS:
        params = [{"to": to, "data": data}, "latest"] if method == "eth_call" else [to, "latest"]
        body = json.dumps({"jsonrpc":"2.0","id":1,"method":method,"params":params}).encode()
        try:
            r = urllib.request.urlopen(urllib.request.Request(rpc, data=body,
                headers={"Content-Type":"application/json"}), timeout=15)
            res = json.loads(r.read()).get("result")
            if res is not None:
                return res, rpc
        except Exception:
            pass
        time.sleep(0.4)
    return None, None

for name, f in CANDS:
    code, rpc = call(f, "", "eth_getCode")
    clen = len(code) if code else 0
    print(f"{name:16} {f}  code={clen} via={rpc}")
    if not code or clen < 100:
        continue
    for label, data in [
        ("getPair", GET_PAIR + pad(a) + pad(b)),
        ("poolByPair", POOL_BY_PAIR + pad(a) + pad(b)),
        ("getPool/true", GET_POOL_BOOL + pad(a) + pad(b) + "1".rjust(64,"0")),
    ] + [(f"getPool/{fee}", GET_POOL_FEE + pad(a) + pad(b) + hex(fee)[2:].rjust(64,"0"))
         for fee in (100, 500, 3000, 10000)]:
        res, _ = call(f, data)
        if res and res != "0x" + "0" * 64:
            print(f"    {label} => 0x{res[-40:]}")
        time.sleep(0.4)

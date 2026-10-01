import json, urllib.request

RPCS = ["https://rpc.soniclabs.com", "https://sonic-json-rpc.stakely.io", "https://146.rpc.thirdweb.com"]

def call(rpc, to, data):
    body = json.dumps({"jsonrpc":"2.0","id":1,"method":"eth_call",
        "params":[{"to":to,"data":data},"latest"]}).encode()
    try:
        r = urllib.request.urlopen(urllib.request.Request(rpc, data=body,
            headers={"Content-Type":"application/json"}), timeout=12)
        res = json.loads(r.read())
        return res.get("result")
    except Exception as e:
        return f"ERR {e}"

def anycall(to, data):
    for rpc in RPCS:
        res = call(rpc, to, data)
        if res and not str(res).startswith("ERR"):
            return res
    return res

def pad(a): return a.lower().replace("0x","").rjust(64,"0")

WS = "0x039e2fB66102314Ce7b64Ce5Ce3E5183bc94aD38"
USDC = "0x29219dd400f2Bf60E5a23d13Be72B486D4038894"
a, b = (WS, USDC) if WS.lower() < USDC.lower() else (USDC, WS)

GET_POOL_FEE  = "0x1698ee82"  # getPool(address,address,uint24)
GET_POOL_BOOL = "0x2729e80f"  # getPool(address,address,bool) -- solidly (verify)
GET_PAIR      = "0xe6a43905"  # getPair(address,address)
POOL_BY_PAIR  = "0x0d4f05d4"  # poolByPair(address,address) -- Algebra Integral

FACTORIES = {
    "shadow(cl?)":  "0xcd2d0637c94fe77c2896bbcbb174ceffb08de6d7",
    "wagmi(v3)":    "0x56cfc796bc88c9c7e1b38c2b0af9b7120b079aef",
    "swapx(alg?)":  "0x8121a3f8c4176e9765deea0b95fa2bdfd3016794",
    "defive(v2)":   "0x47524ca6578e172878abf6fd6f3e1cd106c551e6",
    "equalizer":    "0x7ca1dccfb4f49564b8f13e18a67747fd428f1c40",
    "spooky(v2)":   "0x3d91b700252e0e3ee7805d12e048a988ab69c8ad",
    "solidlycom":   "0x777faca731b17e8847ebf175c94dbe9d81a8f630",
}

for name, f in FACTORIES.items():
    out = {}
    # v2 getPair
    out["getPair"] = anycall(f, GET_PAIR + pad(a) + pad(b))
    # v3 getPool fees
    for fee in [500, 3000, 10000]:
        out[f"getPool/{fee}"] = anycall(f, GET_POOL_FEE + pad(a) + pad(b) + hex(fee)[2:].rjust(64,"0"))
    # solidly bool
    out["getPool/true"] = anycall(f, GET_POOL_BOOL + pad(a) + pad(b) + "1".rjust(64,"0"))
    # algebra poolByPair
    out["poolByPair"] = anycall(f, POOL_BY_PAIR + pad(a) + pad(b))
    hits = {k: v for k, v in out.items() if v and v != "0x" + "0"*0 and not str(v).startswith("ERR") and v != "0x"+"0"*64}
    print(name, f)
    for k, v in out.items():
        tag = ""
        if v and not str(v).startswith("ERR") and v != "0x" + "0"*64:
            tag = "  *** POOL " + "0x"+v[-40:]
        print("   ", k, "=>", (v[:42]+"...") if v and len(str(v))>46 else v, tag)

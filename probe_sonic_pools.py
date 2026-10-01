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

POOLS = {
    "shadow":  "0x324963c267C354c7660Ce8CA3F5f167E05649970",
    "wagmi":   "0x25f746bB206041Ed8dA6F08Ed1D32454A5856D37",
    "swapx":   "0x5C4B7d607aAF7B5CDE9F09b5F03Cf3b5c923AEEa",
    "defive":  "0x6eB32C8dB5Ff2878acbCB6a1Ec5e301F60884dA4",
    "equalizer":"0xb1BC4B830FCbA2184B92e15b9133c41160518038",
    "metropolis":"0x32c0D87389E72E46b54bc4Ea6310C1a0e921C4DC",
    "spooky":  "0x216A86c8716Fad79E05D23b1622cA432A739582A",
    "silverswap":"0x9f46dd8F2A4016C26c1Cf1f4ef90e5E1928D756B",
    "solidlycom":"0x67bcED305725349783d986362Af4092252A7F51A",
}
FACTORY_SEL = "0xc45a0155"  # factory()
# also try poolDeployer/pool factory variants
for name, pool in POOLS.items():
    res = None
    for rpc in RPCS:
        res = call(rpc, pool, FACTORY_SEL)
        if res and not str(res).startswith("ERR"):
            break
    fac = "0x" + res[-40:] if res and res != "0x" and not str(res).startswith("ERR") else res
    print(name, pool, "-> factory:", fac)

# verify getPool(a,b,fee) / getPool(a,b,bool) / getPair on derived factories
WS = "0x039e2fB66102314Ce7b64Ce5Ce3E5183bc94aD38"
USDC = "0x29219dd400f2Bf60E5a23d13Be72B486D4038894"
def pad(a): return a.lower().replace("0x","").rjust(64,"0")
a, b = (WS, USDC) if WS.lower() < USDC.lower() else (USDC, WS)
print("sorted:", a, b)

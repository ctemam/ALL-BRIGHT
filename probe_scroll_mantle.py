import json, urllib.request, time

CHAINS = {
    "scroll": {
        "rpcs": ["https://rpc.scroll.io", "https://534352.rpc.thirdweb.com"],
        "A": "0x5300000000000000000000000000000000000004",  # WETH
        "B": "0x06eFdBFf2a14a7c8E15944D1F4A48F9F95F663A4",  # USDC
        "factories": [
            ("kyber-elastic", "0xC7a590291e07B9fe9E64b86c58fD8fC764308C4A"),
            ("zebra-v2",      "0x017a9E57bEa51AeB948d8c1eCcB4d5F88a0f6Ca5"),
            ("skydrome",      "0x2513b8e1f45512b33a74c4c13beebb41a4cf7540"),
            ("nuri-cl",       "0xAAA32926fcE6bE95ea2c51cB4Fcb60836D320C42"),
            ("pencil-v3",     "0xB5a7E34e7eC279d47CcF5a73970df6b98f90D839"),
        ],
    },
    "mantle": {
        "rpcs": ["https://rpc.mantle.xyz", "https://5000.rpc.thirdweb.com"],
        "A": "0xdEAddEaDdeadDEadDEADDEAddEADDEAddead1111",  # bridged WETH
        "B": "0x201EBa5CC46D216Ce6DC03F6a759e8E766e956aE",  # USDT
        "factories": [
            ("fusionx-v3",  "0x59b360c9A53ec523613a9a7e92959b1b4a0157f6"),
            ("uni-v3",      "0x0d922Fb1Bc191F64970ac40376643808b4B74Df9"),
            ("cleopatra",   "0xAAA16c016BF556fcD620328f0759252E29b1AB57"),
            ("fusionx-v2",  "0xE5020961fA51ffd3662CDf307dEf18b9a87CCE7c"),
        ],
    },
}

GET_POOL_FEE  = "0x1698ee82"
GET_POOL_BOOL = "0x2729e80f"
GET_PAIR      = "0xe6a43905"
POOL_BY_PAIR  = "0x0d4f05d4"

def pad(a): return a.lower().replace("0x","").rjust(64,"0")

def call(rpc, to, data):
    body = json.dumps({"jsonrpc":"2.0","id":1,"method":"eth_call",
        "params":[{"to":to,"data":data},"latest"]}).encode()
    try:
        r = urllib.request.urlopen(urllib.request.Request(rpc, data=body,
            headers={"Content-Type":"application/json"}), timeout=15)
        res = json.loads(r.read())
        return res.get("result") or "REVERT"
    except Exception as e:
        return f"ERR {e}"

for chain, cfg in CHAINS.items():
    a, b = (cfg["A"], cfg["B"]) if cfg["A"].lower() < cfg["B"].lower() else (cfg["B"], cfg["A"])
    print("=====", chain)
    for name, f in cfg["factories"]:
        # bytecode check
        code = None
        for rpc in cfg["rpcs"]:
            body = json.dumps({"jsonrpc":"2.0","id":1,"method":"eth_getCode",
                "params":[f,"latest"]}).encode()
            try:
                r = urllib.request.urlopen(urllib.request.Request(rpc, data=body,
                    headers={"Content-Type":"application/json"}), timeout=15)
                code = json.loads(r.read()).get("result")
            except Exception as e:
                code = f"ERR {e}"
            if code and not str(code).startswith("ERR"):
                break
        has_code = code and len(code) > 4
        print(f"  {name} {f} code={len(code) if has_code else code}")
        if not has_code:
            continue
        for label, data in [
            ("getPair", GET_PAIR + pad(a) + pad(b)),
            ("poolByPair", POOL_BY_PAIR + pad(a) + pad(b)),
            ("getPool/true", GET_POOL_BOOL + pad(a) + pad(b) + "1".rjust(64,"0")),
        ] + [ (f"getPool/{fee}", GET_POOL_FEE + pad(a) + pad(b) + hex(fee)[2:].rjust(64,"0"))
              for fee in (100,500,3000,10000) ]:
            res = None
            for rpc in cfg["rpcs"]:
                res = call(rpc, f, data)
                if res and not str(res).startswith("ERR"):
                    break
            if res and res != "REVERT" and res != "0x" + "0"*64:
                print(f"    {label} => 0x{res[-40:]}")
            time.sleep(0.3)
        time.sleep(0.5)

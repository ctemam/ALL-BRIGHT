import json, urllib.request, itertools

def rpc(url, to, data):
    req = urllib.request.Request(url, data=json.dumps({
        "jsonrpc":"2.0","id":1,"method":"eth_call",
        "params":[{"to":to,"data":data},"latest"]}).encode(),
        headers={"Content-Type":"application/json"})
    try:
        r = json.loads(urllib.request.urlopen(req, timeout=10).read())
        if "error" in r: return f"ERR:{r['error'].get('message','?')[:40]}"
        return r.get("result","0x")
    except Exception as e:
        return f"EXC:{str(e)[:40]}"

def word(a): return a.lower().replace("0x","").rjust(64,"0")

def sel(sig):
    import hashlib
    # keccak via pycryptodome if present else manual? use eth_utils-free: try sha3_256 (wrong) — better use web3-free keccak
    try:
        from Crypto.Hash import keccak
        k = keccak.new(digest_bits=256); k.update(sig.encode()); return k.hexdigest()[:8]
    except Exception:
        pass
    raise SystemExit("no keccak")

GET_PAIR = "e6a43905"            # getPair(address,address)
GET_POOL_UINT24 = "1698ee82"     # getPool(address,address,uint24)
GET_POOL_BOOL = None
# getPool(address,address,bool) selector — compute
GET_POOL_BOOL = sel("getPool(address,address,bool)")

# Optimism probe: USDC + WETH known pair
OP = "https://optimism-rpc.publicnode.com"
OP_USDC = "0x0b2C639c533813f4Aa9D7837CAf62653d097Ff85"
OP_WETH = "0x4200000000000000000000000000000000000006"
VELO_V2 = "0xF1046053aa5682b4F9a81b5481394DA16BE5FF5a"
SUSHI_OP = "0xc35DADB65012eC5796536bD9864eD8773aBc74C4"

print("== OP: Velodrome V2 factory ==")
for flag in ("0","1"):
    data = "0x"+GET_POOL_BOOL+word(OP_USDC)+word(OP_WETH)+flag.rjust(64,"0")
    print(f"  getPool(a,b,{flag}):", rpc(OP, VELO_V2, data))
for fee in (1,5,25,30,100):
    data = "0x"+GET_POOL_UINT24+word(OP_USDC)+word(OP_WETH)+f"{fee:064x}"
    print(f"  getPool(a,b,{fee}):", rpc(OP, VELO_V2, data))
print("== OP: SushiSwap V2 getPair(USDC,WETH) ==")
print("  ", rpc(OP, SUSHI_OP, "0x"+GET_PAIR+word(OP_USDC)+word(OP_WETH)))

# BSC probe: USDT + WBNB
BSC = "https://bsc-rpc.publicnode.com"
BSC_USDT = "0x55d398326f99059fF775485246999027B3197955"
BSC_WBNB = "0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c"
for name, fac in [("ApeSwap",  "0xCf083Be4164828f00cAE704EC15a36D711491284"),
                  ("ApeSwap-alt","0x0841BD0B734E4F5853f0dD8d7Ea041c241fb0Da6"),
                  ("BiSwap",   "0x858E3312ed3A876947EA49d572A7C42DE08af7EE"),
                  ("BakerySwap","0x01bF7C66c6BD861915CdaaE475042d3c4BaE16A7")]:
    print(f"== BSC: {name} getPair(USDT,WBNB) ==")
    print("  ", rpc(BSC, fac, "0x"+GET_PAIR+word(BSC_USDT)+word(BSC_WBNB)))

# Base: Aerodrome bool sig confirm + BaseSwap
BASE = "https://base-rpc.publicnode.com"
BASE_USDC = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"
BASE_WETH = "0x4200000000000000000000000000000000000006"
AERO = "0x420DD381b31aEf6683db6B902084cB0FFECe40Ab"
BASESWAP = "0xFDa619b6d20975be80A10332cD39b9a4b0FAa8BB"
print("== BASE: Aerodrome getPool(a,b,bool) ==")
for flag in ("0","1"):
    data = "0x"+GET_POOL_BOOL+word(BASE_USDC)+word(BASE_WETH)+flag.rjust(64,"0")
    print(f"  flag={flag}:", rpc(BASE, AERO, data))
print("== BASE: BaseSwap getPair(USDC,WETH) ==")
print("  ", rpc(BASE, BASESWAP, "0x"+GET_PAIR+word(BASE_USDC)+word(BASE_WETH)))

# Gnosis: Sushi V2 + Swapr
GNO = "https://gnosis-rpc.publicnode.com"
GNO_WXDAI = "0xe91D153E0b41518A2Ce8Dd3D7944Fa863463a97d"
GNO_USDC  = "0xDDAfbb505ad214D7b80b1f830fcCc89B60fb7A83"
for name, fac in [("SushiSwap V2","0xc35DADB65012eC5796536bD9864eD8773aBc74C4"),
                  ("Swapr V2","0x5D48C95AdfFD4B40c1AAADc4e08fc44117E02117")]:
    print(f"== GNOSIS: {name} getPair(WXDAI,USDC) ==")
    print("  ", rpc(GNO, fac, "0x"+GET_PAIR+word(GNO_WXDAI)+word(GNO_USDC)))

# Celo: Ubeswap
CELO = "https://celo-rpc.publicnode.com"
CELO_CELO = "0x471EcE3750Da237f93B8E339c536989b8978a438"   # wrapped? CELO native == ERC20 itself
CELO_CUSD = "0x765DE816845861e75A25fCA122bb6898B8B1282a"
UBE = "0x62d5b84bE28a183aBB507E125B384122D2C25fAE"
print("== CELO: Ubeswap getPair(CELO,cUSD) ==")
print("  ", rpc(CELO, UBE, "0x"+GET_PAIR+word(CELO_CELO)+word(CELO_CUSD)))

# Linea: PCS V3 + V2
LINEA = "https://linea-rpc.publicnode.com"
LINEA_WETH = "0xe5d7c2a44ffddf6b295a15c148167daaaf5cf34f"
LINEA_USDC = "0x176211869cA2b568f2A7D4EE941E073a821EE1ff"
PCS3 = "0x0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865"
PCS2 = "0x02a84c1b3BBD7401a5f7fa98a384EBC70bB5749E"
print("== LINEA: PCS V3 getPool(USDC,WETH,500) ==")
print("  ", rpc(LINEA, PCS3, "0x"+GET_POOL_UINT24+word(LINEA_USDC)+word(LINEA_WETH)+f"{500:064x}"))
print("== LINEA: PCS V2 getPair(USDC,WETH) ==")
print("  ", rpc(LINEA, PCS2, "0x"+GET_PAIR+word(LINEA_USDC)+word(LINEA_WETH)))

# Polygon: Sushi V3 + ApeSwap
POLY = "https://polygon-bor-rpc.publicnode.com"
POLY_USDC = "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174"
POLY_WMATIC = "0x0d500B1d8E8eF31E21C99d1Db9A6444d3ADf1270"
SUSHI3_POLY = "0x917933899c6a5F8E37F31E19f92CdBFF7e8FF0e2"
APESWAP_POLY = "0xCf083Be4164828f00cAE704EC15a36D711491284"
print("== POLY: Sushi V3 getPool(USDC,WMATIC,500) ==")
print("  ", rpc(POLY, SUSHI3_POLY, "0x"+GET_POOL_UINT24+word(POLY_USDC)+word(POLY_WMATIC)+f"{500:064x}"))
print("== POLY: ApeSwap getPair(USDC,WMATIC) ==")
print("  ", rpc(POLY, APESWAP_POLY, "0x"+GET_PAIR+word(POLY_USDC)+word(POLY_WMATIC)))

# ETH: Sushi V2 + PCS V3
ETH = "https://ethereum-rpc.publicnode.com"
ETH_USDC = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"
ETH_WETH = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"
print("== ETH: Sushi V2 getPair(USDC,WETH) ==")
print("  ", rpc(ETH, "0xC0AEe478e3658e2610c5F7A4A2E1777cE9e4f2Ac", "0x"+GET_PAIR+word(ETH_USDC)+word(ETH_WETH)))
print("== ETH: PCS V3 getPool(USDC,WETH,500) ==")
print("  ", rpc(ETH, PCS3, "0x"+GET_POOL_UINT24+word(ETH_USDC)+word(ETH_WETH)+f"{500:064x}"))

# Arbitrum: PCS V3
ARB = "https://arbitrum-one-rpc.publicnode.com"
ARB_USDC = "0xaf88d065e77c8cC2239327C5EDb3A432268e5831"
ARB_WETH = "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1"
print("== ARB: PCS V3 getPool(USDC,WETH,500) ==")
print("  ", rpc(ARB, PCS3, "0x"+GET_POOL_UINT24+word(ARB_USDC)+word(ARB_WETH)+f"{500:064x}"))

# Avalanche: TraderJoe, Pangolin, Sushi
AVAX = "https://avalanche-c-chain-rpc.publicnode.com"
AVAX_USDC = "0xB97EF9Ef8734C71904D8002F8b6Bc66Dd9c48a6E"
AVAX_WAVAX = "0xB31f66AA3C1e785363F0875A1B74E27b85FD66c7"
for name, fac in [("TraderJoe","0x9Ad6C38BE94206cA50bb0d90783181662f0CFC10"),
                  ("Pangolin","0xefa94DE7a4656D787667C749f7E1223D71E9FD88"),
                  ("SushiSwap","0xc35DADB65012eC5796536bD9864eD8773aBc74C4")]:
    print(f"== AVAX: {name} getPair(USDC,WAVAX) ==")
    print("  ", rpc(AVAX, fac, "0x"+GET_PAIR+word(AVAX_USDC)+word(AVAX_WAVAX)))

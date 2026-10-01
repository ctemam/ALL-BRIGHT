#!/usr/bin/env python3
import json, urllib.request, time

def rpc(url, method, params, timeout=15):
    try:
        req = urllib.request.Request(url, data=json.dumps(
            {"jsonrpc":"2.0","id":1,"method":method,"params":params}).encode(),
            headers={"Content-Type":"application/json"})
        r = json.loads(urllib.request.urlopen(req, timeout=timeout).read())
        return r.get("result"), r.get("error")
    except Exception as e:
        return None, str(e)[:90]

def pad32(h): return h.replace("0x","").lower().rjust(64,"0")
GETPOOL="0x1698ee82"; GETPAIR="0xe6a43905"
def fee(f): return format(f,"064x")

def try_fees(url, fac, a, b, fees=(100,500,3000,10000)):
    for f in fees:
        d = GETPOOL + pad32(a) + pad32(b) + fee(f)
        r,_ = rpc(url,"eth_call",[{"to":fac,"data":d},"latest"])
        if r and len(r)>=66 and int(r,16)!=0:
            print(f"      getPool fee={f} -> 0x{r[-40:]}")
            return f
    print("      no pool for any fee tier")
    return None

# Unichain via publicnode
print("===== Unichain (publicnode) =====")
u="https://unichain-rpc.publicnode.com"
weth="0x4200000000000000000000000000000000000006"
usdc="0x078D782b760474a361bDA0F3bCf0c1b71dDbc20B"
for lbl,a in [("WETH",weth),("USDC",usdc),("UniV3","0x1F98400000000000000000000000000000000003"),("UniV2","0x8909Dc15e40173Ff4699343b6eB8132c65e18eC6")]:
    c,e = rpc(u,"eth_getCode",[a,"latest"])
    print(f"  {lbl:<8} code={'YES' if c and c!='0x' else 'NO'} {e or ''}")
    time.sleep(.4)
try_fees(u,"0x1F98400000000000000000000000000000000003",weth,usdc)
d = GETPAIR+pad32(weth)+pad32(usdc)
r,e = rpc(u,"eth_call",[{"to":"0x8909Dc15e40173Ff4699343b6eB8132c65e18eC6","data":d},"latest"])
print(f"  UniV2 getPair -> {('0x'+r[-40:]) if r else None} {e or ''}")

# Scroll via publicnode
print("\n===== Scroll (publicnode) =====")
u="https://scroll-rpc.publicnode.com"
weth="0x5300000000000000000000000000000000000004"
usdc="0x06eFdBFf2a14a7c8E15944D1F4A48F9F95F663A4"
usdt="0xf55BEC9cafDbE8730f096Aa55dad6D22d44099Df"
for lbl,a in [("WETH",weth),("USDC",usdc),("USDT",usdt),
              ("UniV3","0x70C62C8b8e801124A4Aa81ce07b637A3e83cb919"),
              ("Honeypop","0x81A1aE7c40A82F34031BAa0132e23B5DeD947eD5"),
              ("NURI?","0xAAA20C5a584a9fECdFEDD71B46DA77C0Ca4A47f2"),
              ("Honeypop2?","0xfd356dd75CD8e67120e0b70a4Db0e01b804eee37")]:
    c,e = rpc(u,"eth_getCode",[a,"latest"])
    print(f"  {lbl:<10} {a} code={'YES' if c and c!='0x' else 'NO'} {e or ''}")
    time.sleep(.4)
try_fees(u,"0x70C62C8b8e801124A4Aa81ce07b637A3e83cb919",weth,usdc)
try_fees(u,"0xAAA20C5a584a9fECdFEDD71B46DA77C0Ca4A47f2",weth,usdc)  # NURI is UniV3-fork?
d=GETPAIR+pad32(weth)+pad32(usdc)
for n,f in [("Honeypop","0x81A1aE7c40A82F34031BAa0132e23B5DeD947eD5"),("Honeypop2","0xfd356dd75CD8e67120e0b70a4Db0e01b804eee37")]:
    r,e = rpc(u,"eth_call",[{"to":f,"data":d},"latest"])
    print(f"  {n} getPair -> {('0x'+r[-40:]) if r else None} {e or ''}")

# Mantle — WMNT candidates
print("\n===== Mantle =====")
u="https://mantle-rpc.publicnode.com"
usdt="0x201EBa5CC46D216Ce6DC03F6a759e8E766e956aE"
for lbl,a in [("WMNT-78c1","0x78c1b0C915c4FAA5FffA6Cabf6899e63F82b0B45"),
              ("WMNT-201e","0x201EBa5CC46D216Ce6DC03F6a759e8E766e956aE"),
              ("MNT-predep","0xDeadDeAddeAddEAddeadDeaDdeAdDeaDDeAD0000"),
              ("USDC","0x09Bc4E0D864854c6aFB6eB9A9cdF58aC190D0dF9"),
              ("Agni","0x25780dc8Fc3cfBD75F33bFDAB65e969b603b2035"),
              ("FusionX","0x530d5506aef3c19b06d66cf8894259a9c897a5c7"),
              ("CrustV3?","0xAAA32926fcE6bE95ea2c51cB4Fcb60836D320C42"),
              ("FusionXV2?","0x530d5506aef3c19b06d66cf8894259a9c897a5c7"),
              ("Cleopatra?","0xAAA16c016BF556Fcb4269f3C43E7A8c16984B51D"),
              ("KimV4?","0xAAA45c8F5ef92a000a121d49F4b9BAAe7a5F8068")]:
    c,e = rpc(u,"eth_getCode",[a,"latest"])
    has = c and c!="0x"
    print(f"  {lbl:<11} {a} code={'YES' if has else 'NO'} {e or ''}")
    if has and lbl.startswith(("WMNT","MNT")):
        d,_ = rpc(u,"eth_call",[{"to":a,"data":"0x313ce567"},"latest"])
        s,_ = rpc(u,"eth_call",[{"to":a,"data":"0x95d89b41"},"latest"])
        print(f"      decimals={int(d,16) if d else '?'} sym={s[:80] if s else None}")
    time.sleep(.4)

# Sonic — more factories + pair variants
print("\n===== Sonic =====")
u="https://sonic-rpc.publicnode.com"
ws="0x039e2fB66102314Ce7b64Ce5Ce3E5183bc94aD38"
usdc="0x29219dd400f2Bf60E5a23d13Be72B486D4038894"
weth="0x50c42dEAcD8Fc9773493ED674b675c5775777aef"
for lbl,a in [("WETH-br","0x50c42dEAcD8Fc9773493ED674b675c5775777aef"),
              ("UniV3","0xcb2436774C3e191c85056d248EF4260ce5f27A9D"),
              ("WagmiV3?","0x7e90CE1271a8E231A3D9B92BbDc56a0Ef8f524a3"),
              ("ShadowCL?","0x32467c5ae05165E4F24FEa46B7Fb880cfFF94119"),
              ("SwapXV2?","0xA359F23145EbbFbd96183eC1A1f2cCCE8551765f"),
              ("Equalizer?","0xDDD9845Ba0D8f38d3045f804f67A1a8B9A528FcC"),
              ("WagmiV2?","0x4CD0C60B5F4E21f7c87A85d05b6B80cdAeE8625e")]:
    c,e = rpc(u,"eth_getCode",[a,"latest"])
    print(f"  {lbl:<11} {a} code={'YES' if c and c!='0x' else 'NO'} {e or ''}")
    time.sleep(.4)
for fac_lbl, fac in [("UniV3","0xcb2436774C3e191c85056d248EF4260ce5f27A9D"),
                     ("WagmiV3","0x7e90CE1271a8E231A3D9B92BbDc56a0Ef8f524a3"),
                     ("ShadowCL","0x32467c5ae05165E4F24FEa46B7Fb880cfFF94119")]:
    print(f"  {fac_lbl}:")
    try_fees(u,fac,weth,usdc)
    try_fees(u,fac,ws,usdc)

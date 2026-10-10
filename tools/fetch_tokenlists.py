#!/usr/bin/env python3
"""Fetch industry-standard token lists (Uniswap TokenList format) and
DefiLlama TVL weights -> data/token_universe.json + data/tvl_weights.json.

Pre-built sources only (AGENTS: no invented wheels):
  - Uniswap default token list   (tokenlists.org standard)
  - CoinGecko-derived list (via Uniswap tokenlist CDN mirror)
  - DefiLlama /api/protocols     (free, no key — TVL weighting)
"""
import json, time, urllib.request

UA = {"User-Agent": "allbright/1.0"}
OUT_TOK = "data/token_universe.json"
OUT_TVL = "data/tvl_weights.json"

CHAIN_IDS = {  # AllBright active chains -> EIP-155
    56: "bsc", 42161: "arbitrum", 137: "polygon", 8453: "base", 43114: "avalanche",
    1: "ethereum", 10: "optimism", 59144: "linea", 534352: "scroll", 81457: "blast",
}

LISTS = [
    "https://tokens.uniswap.org",                       # Uniswap default list
    "https://gateway.ipfs.io/ipns/tokens.uniswap.org",  # mirror fallback
]

def get(url, timeout=30):
    req = urllib.request.Request(url, headers=UA)
    return json.loads(urllib.request.urlopen(req, timeout=timeout).read())

def main():
    tokens, seen = [], set()
    for url in LISTS:
        try:
            doc = get(url)
            for t in doc.get("tokens", []):
                cid, addr = t.get("chainId"), (t.get("address") or "").lower()
                if cid in CHAIN_IDS and addr and (cid, addr) not in seen:
                    seen.add((cid, addr))
                    tokens.append({
                        "chainId": cid, "chain": CHAIN_IDS[cid], "address": addr,
                        "symbol": t.get("symbol"), "name": t.get("name"),
                        "decimals": t.get("decimals"),
                        "tags": t.get("tags", []), "list": url.split("/")[2],
                    })
            if tokens:
                break
        except Exception as e:
            print("list failed", url, e)
    json.dump({"asof": int(time.time()), "source": "uniswap-tokenlist-standard",
               "count": len(tokens), "tokens": tokens},
              open(OUT_TOK, "w"), indent=2)
    print("token_universe:", len(tokens), "tokens")

    try:
        prots = get("https://api.llama.fi/protocols")
        weights = {}
        for p in prots:
            for c in (p.get("chains") or []):
                k = c.lower()
                weights[k] = weights.get(k, 0) + (p.get("tvl") or 0)
        json.dump({"asof": int(time.time()), "source": "defillama/protocols",
                   "chain_tvl_usd": weights}, open(OUT_TVL, "w"), indent=2)
        print("tvl_weights:", len(weights), "chains")
    except Exception as e:
        print("defillama failed", e)

if __name__ == "__main__":
    main()

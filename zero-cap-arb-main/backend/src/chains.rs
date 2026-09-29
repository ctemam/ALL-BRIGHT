use crate::types::{ChainConfig, DexConfig};
use std::sync::OnceLock;

pub static CHAINS: OnceLock<Vec<ChainConfig>> = OnceLock::new();

/// Read an env var and split by comma â†’ multiple RPC URLs
/// If the var is `KEY` it reads `KEY`, `KEY_1`, `KEY_2`, ...
/// Example:
///   ETH_RPC_URL=https://eth-mainnet.g.alchemy.com/v2/xxx
///   ETH_RPC_URL_1=https://mainnet.infura.io/v3/yyy
/// Or all in one:
///   ETH_RPC_URL="https://eth-mainnet.alchemy.io/xxx,https://mainnet.infura.io/yyy"
fn read_rpc_urls(env_key: &str, default: &str) -> Vec<String> {
    let primary = std::env::var(env_key).unwrap_or_else(|_| default.to_string());

    // Try splitting by comma
    let parts: Vec<&str> = primary
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if parts.len() > 1 {
        return parts.into_iter().map(|s| s.to_string()).collect();
    }

    // Try numbered fallbacks: KEY_1, KEY_2, ...
    let mut urls = vec![primary];
    for i in 1..=3 {
        let fallback_key = format!("{}_{}", env_key, i);
        match std::env::var(&fallback_key) {
            Ok(url) => urls.push(url),
            Err(_) => break,
        }
    }

    urls
}

pub fn get_chains() -> &'static Vec<ChainConfig> {
    CHAINS.get_or_init(|| {
        vec![
            ChainConfig {
                id: 1,
                name: "Ethereum".to_string(),
                rpc_url: std::env::var("ETH_RPC_URL")
                    .unwrap_or_else(|_| "https://eth.merkle.io".to_string()),
                rpc_urls: read_rpc_urls("ETH_RPC_URL", "https://eth.merkle.io"),
                native_currency: "ETH".to_string(),
                explorer_url: "https://etherscan.io".to_string(),
            },
            ChainConfig {
                id: 42161,
                name: "Arbitrum".to_string(),
                rpc_url: std::env::var("ARB_RPC_URL")
                    .unwrap_or_else(|_| "https://arb1.arbitrum.io/rpc".to_string()),
                rpc_urls: read_rpc_urls("ARB_RPC_URL", "https://arb1.arbitrum.io/rpc"),
                native_currency: "ETH".to_string(),
                explorer_url: "https://arbiscan.io".to_string(),
            },
            ChainConfig {
                id: 10,
                name: "Optimism".to_string(),
                rpc_url: std::env::var("OP_RPC_URL")
                    .unwrap_or_else(|_| "https://mainnet.optimism.io".to_string()),
                rpc_urls: read_rpc_urls("OP_RPC_URL", "https://mainnet.optimism.io"),
                native_currency: "ETH".to_string(),
                explorer_url: "https://optimistic.etherscan.io".to_string(),
            },
            ChainConfig {
                id: 137,
                name: "Polygon".to_string(),
                rpc_url: std::env::var("POLY_RPC_URL")
                    .unwrap_or_else(|_| "https://polygon-rpc.com".to_string()),
                rpc_urls: read_rpc_urls("POLY_RPC_URL", "https://polygon-rpc.com"),
                native_currency: "MATIC".to_string(),
                explorer_url: "https://polygonscan.com".to_string(),
            },
            ChainConfig {
                id: 56,
                name: "BSC".to_string(),
                rpc_url: std::env::var("BSC_RPC_URL")
                    .unwrap_or_else(|_| "https://bsc-dataseed.binance.org".to_string()),
                rpc_urls: read_rpc_urls("BSC_RPC_URL", "https://bsc-dataseed.binance.org"),
                native_currency: "BNB".to_string(),
                explorer_url: "https://bscscan.com".to_string(),
            },
            ChainConfig {
                id: 43114,
                name: "Avalanche".to_string(),
                rpc_url: std::env::var("AVAX_RPC_URL")
                    .unwrap_or_else(|_| "https://api.avax.network/ext/bc/C/rpc".to_string()),
                rpc_urls: read_rpc_urls("AVAX_RPC_URL", "https://api.avax.network/ext/bc/C/rpc"),
                native_currency: "AVAX".to_string(),
                explorer_url: "https://snowtrace.io".to_string(),
            },
            ChainConfig {
                id: 8453,
                name: "Base".to_string(),
                rpc_url: std::env::var("BASE_RPC_URL")
                    .unwrap_or_else(|_| "https://mainnet.base.org".to_string()),
                rpc_urls: read_rpc_urls("BASE_RPC_URL", "https://mainnet.base.org"),
                native_currency: "ETH".to_string(),
                explorer_url: "https://basescan.org".to_string(),
            },
            ChainConfig {
                id: 42220,
                name: "Celo".to_string(),
                rpc_url: std::env::var("CELO_RPC_URL")
                    .unwrap_or_else(|_| "https://forno.celo.org".to_string()),
                rpc_urls: read_rpc_urls("CELO_RPC_URL", "https://forno.celo.org"),
                native_currency: "CELO".to_string(),
                explorer_url: "https://celoscan.io".to_string(),
            },
            ChainConfig {
                id: 100,
                name: "Gnosis".to_string(),
                rpc_url: std::env::var("GNOSIS_RPC_URL")
                    .unwrap_or_else(|_| "https://rpc.gnosischain.com".to_string()),
                rpc_urls: read_rpc_urls("GNOSIS_RPC_URL", "https://rpc.gnosischain.com"),
                native_currency: "xDAI".to_string(),
                explorer_url: "https://gnosisscan.io".to_string(),
            },
            ChainConfig {
                id: 59144,
                name: "Linea".to_string(),
                rpc_url: std::env::var("LINEA_RPC_URL")
                    .unwrap_or_else(|_| "https://rpc.linea.build".to_string()),
                rpc_urls: read_rpc_urls("LINEA_RPC_URL", "https://rpc.linea.build"),
                native_currency: "ETH".to_string(),
                explorer_url: "https://lineascan.build".to_string(),
            },
        ]
    })
}

/// DEX list for a chain, derived from the venue registry.
///
/// This used to be a hand-maintained table ("50+ DEXes across all 6 chains")
/// that mixed real routers with invented filler addresses (a Curve entry of
/// `0x5a0F6fC7...`, a Maker PSM entry of `0xf6f9cD9C...`, and others) and
/// reused *Ethereum* routers as "Uniswap V3"/"SushiSwap" on five other chains.
/// It also covered only 6 of the 10 configured chains, so Base, Celo, Gnosis
/// and Linea fell through to `vec![]` and were skipped by every scan path.
///
/// It is now a projection of `get_venues`, so it can only report addresses that
/// the venue registry carries, every configured chain is represented, and venue
/// data has a single source of truth.
///
/// NOTE: these are factory addresses, not router addresses. Anything that
/// needs a router (quoting a swap) must not use this function.
pub fn get_dexes_for_chain(chain_id: u64) -> Vec<DexConfig> {
    get_venues(chain_id)
        .iter()
        .map(|v| DexConfig {
            name: v.name.to_string(),
            address: v.address.to_string(),
            chain_id,
            router_abi: None,
        })
        .collect()
}

/// Canonical UniswapV2-compatible factory per chain.
///
/// Derived from `get_venues`: a second hand-written table is exactly how two
/// views of one registry drift apart, and a drifted address silently reads a
/// different contract and fabricates prices. Callers that need every venue, not
/// just the V2 one, should use `get_venues` directly.
///
/// Returns "" for a chain with no registry-listed V2 market (Celo).
pub fn get_v2_factory(chain_id: u64) -> &'static str {
    get_venues(chain_id)
        .iter()
        .find(|v| v.protocol == Protocol::V2)
        .map(|v| v.address)
        .unwrap_or("")
}

/// UniswapV3-compatible factories per chain.
///
/// Derived from `get_venues`, so each venue's fee tiers and pool-lookup
/// signature are declared in exactly one place (`V3Params`).
pub fn get_v3_factories(chain_id: u64) -> Vec<Factory> {
    get_venues(chain_id)
        .iter()
        .filter(|v| v.protocol == Protocol::V3)
        .map(|v| Factory {
            name: v.name,
            address: v.address,
        })
        .collect()
}

/// True when the chain has a known V2 factory to resolve pairs against.
pub fn has_v2_factory(chain_id: u64) -> bool {
    !get_v2_factory(chain_id).is_empty()
}

/// True when the chain has a known V3 factory.
pub fn has_v3_factory(chain_id: u64) -> bool {
    !get_v3_factories(chain_id).is_empty()
}

/// Common token addresses across chains
/// Common token addresses across chains (wrapped native).
///
/// Canonical wrapped-native address per chain, taken from each chain's own
/// documentation. Not re-verified from this development sandbox, whose RPC
/// endpoints do not serve real chain state (see [`get_venues`]).
pub fn get_wrapped_native(chain_id: u64) -> &'static str {
    match chain_id {
        1 => "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2",
        42161 => "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1",
        10 => "0x4200000000000000000000000000000000000006",
        137 => "0x0d500B1d8E8eF31E21C99d1Db9A6444d3ADf1270",
        56 => "0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c",
        43114 => "0xB31f66AA3C1e785363F0875A1B74E27b85FD66c7",
        8453 => "0x4200000000000000000000000000000000000006",
        // Celo has no WETH9: the protocol uses the CELO ERC-20 as wrapped native.
        42220 => "0x471EcE3750Da237f93B8e339c536989b8978a438",
        100 => "0xe91D153E0b41518A2Ce8Dd3D7944Fa863463a97d",
        // Linea's WETH is NOT the 0x4200...0006 predeploy (that has no code
        // on Linea); this is the real WETH9 deployment.
        59144 => "0xe5d7c2a44ffddf6b295a15c148167daaaf5cf34f",
        _ => "",
    }
}

/// A USDC-pegged stablecoin per chain, used as the quote leg for spreads.
///
/// Canonical Circle-issued USDC per chain. An earlier revision substituted DAI
/// on Base because `eth_getCode` through the sandbox's RPC endpoints reported no
/// bytecode at native USDC's canonical Base address. That check was invalid: the
/// same endpoints report no bytecode for the canonical Uniswap V2 factory on
/// mainnet, which has had code since 2020 (see [`get_venues`]). DAI is not a
/// USDC-pegged asset either, so it did not satisfy this function's own contract
/// even where it exists. Re-verify every entry against a trusted node before P1
/// starts pricing spreads against these.
pub fn get_stable_token(chain_id: u64) -> &'static str {
    match chain_id {
        1 => "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
        42161 => "0xaf88d065e77c8cC2239327C5EDb3A432268e5831",
        10 => "0x0b2C639c533813f4Aa9D7837CAf62653d097Ff85",
        137 => "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174",
        56 => "0x8AC76a51cc950d9822D68b83fE1Ad97B32Cd580d",
        43114 => "0xB97EF9Ef8734C71904D8002F8b6Bc66Dd9c48a6E",
        // Circle-issued native USDC on Base, not DAI.
        8453 => "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913",
        42220 => "0x48065fbbe25f71c9282ddf5e1cd6d6a887483d5e", // USDCe (USD₮)
        100 => "0xddafbb505ad214d7b80b1f830fccc89b60fb7a83",   // bridged USDC
        59144 => "0x176211869cA2b568f2A7D4EE941E073a821EE1ff",
        _ => "",
    }
}

pub fn get_chain_name(chain_id: u64) -> &'static str {
    match chain_id {
        1 => "Ethereum",
        42161 => "Arbitrum",
        10 => "Optimism",
        137 => "Polygon",
        56 => "BSC",
        43114 => "Avalanche",
        8453 => "Base",
        42220 => "Celo",
        100 => "Gnosis",
        59144 => "Linea",
        _ => "Unknown",
    }
}
/// The protocol that determines how a pool is resolved and priced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// Constant product: `getPair(a,b)` then read `getReserves()`.
    V2,
    /// Concentrated liquidity: `getPool(a,b,fee)` then read `slot0`/`liquidity`.
    V3,
}

impl Protocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Protocol::V2 => "v2",
            Protocol::V3 => "v3",
        }
    }
}

/// Uniswap V3 family fee tiers, in hundredths of a bip (500 = 0.05%).
pub const UNI_V3_FEES: &[u32] = &[100, 500, 3000, 10000];

/// Velodrome Slipstream keys pools by tick spacing rather than by fee. These are
/// the spacings the Celo deployment uses; a spacing with no deployed pool simply
/// resolves to the zero address and is dropped.
pub const SLIPSTREAM_TICK_SPACINGS: &[u32] = &[1, 50, 100, 200];

/// `getPool(address,address,uint24)` — the Uniswap V3 family.
pub const SIG_GET_POOL_UINT24: &str = "getPool(address,address,uint24)";

/// `getPool(address,address,int24)` — Velodrome Slipstream. A *different*
/// 4-byte selector from the uint24 form, so the two cannot share an encoding.
pub const SIG_GET_POOL_INT24: &str = "getPool(address,address,int24)";

/// How a V3 venue resolves the pool for a token pair.
///
/// Kept separate from [`Protocol`] because `Protocol::V3` covers two families
/// whose factory call is not interchangeable.
#[derive(Debug, Clone, Copy)]
pub struct V3Params {
    /// Candidate fee tiers / tick spacings to probe. A tier with no pool is
    /// dropped at resolution time, so listing an extra one is harmless.
    pub fees: &'static [u32],
    /// ABI signature of the factory's pool lookup.
    pub pool_sig: &'static str,
}

/// A V2/V3-compatible factory: the only way to resolve a real pool.
#[derive(Debug, Clone, Copy)]
pub struct Factory {
    pub name: &'static str,
    pub address: &'static str,
}

/// A registry-listed venue that can be priced independently.
#[derive(Debug, Clone, Copy)]
pub struct Venue {
    pub name: &'static str,
    /// The *factory*, not a router: the scanner resolves pools through it.
    pub address: &'static str,
    pub protocol: Protocol,
    /// `Some` exactly when `protocol` is [`Protocol::V3`].
    pub v3: Option<V3Params>,
}

/// Every venue we can resolve a real pool against, per chain.
///
/// A single factory per chain is not enough: `getPair` on one factory can only
/// return that factory's own pools, so every "DEX" would quote an identical
/// price and no spread could ever be observed. The scanner batches resolution
/// across all of these in MultiCall3 to get independent quotes.
///
/// Provenance: these are the canonical deployment addresses published by each
/// protocol (Uniswap's official `deployments.json` plus per-chain token lists),
/// not addresses invented for this codebase.
///
/// They are NOT verified on-chain by this build, and an earlier claim that they
/// were is withdrawn. The RPC endpoints reachable from the development sandbox
/// do not serve real chain state: `eth_getCode` reports empty code for both the
/// canonical Uniswap V2 factory (`0x5C69bEe7...5541d5`) and the mainnet
/// USDC/WETH pair (`0xB4e16d01...c8c0d2`), while USDC's own proxy returns code
/// normally. A "verification" run through such an endpoint proves nothing, and
/// at least one decision was already made on that basis (see
/// [`get_stable_token`]).
///
/// Before real funds are involved, every address here must be re-checked against
/// a trusted node, two levels deep:
///   1. `eth_getCode <factory>` is non-empty;
///   2. `getPair`/`getPool` for a known pair returns a pool whose code is also
///      non-empty.
///
/// Checking only the factory is not enough. A wrong address silently reads a
/// different contract and produces fabricated prices, which is far worse than
/// reporting "no pool" for that venue.
/// V2 venues have no V3 parameters.
const V2_ONLY: Option<V3Params> = None;
/// Uniswap V3 family: `uint24` fee tiers.
const V3_UNI: Option<V3Params> = Some(V3Params {
    fees: UNI_V3_FEES,
    pool_sig: SIG_GET_POOL_UINT24,
});
/// Velodrome Slipstream: `int24` tick spacing, a different selector.
const V3_SLIPSTREAM: Option<V3Params> = Some(V3Params {
    fees: SLIPSTREAM_TICK_SPACINGS,
    pool_sig: SIG_GET_POOL_INT24,
});

pub fn get_venues(chain_id: u64) -> &'static [Venue] {
    match chain_id {
        1 => &[
            Venue {
                name: "Uniswap V2",
                address: "0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f",
                protocol: Protocol::V2,
                v3: V2_ONLY,
            },
            Venue {
                name: "Uniswap V3",
                address: "0x1F98431c8aD98523631AE4a59f267346ea31F984",
                protocol: Protocol::V3,
                v3: V3_UNI,
            },
        ],
        42161 => &[
            Venue {
                name: "SushiSwap V2",
                address: "0xf1D7CC64Fb4452F05c498126312eBE29f30Fbcf9",
                protocol: Protocol::V2,
                v3: V2_ONLY,
            },
            Venue {
                name: "Uniswap V3",
                address: "0x1F98431c8aD98523631AE4a59f267346ea31F984",
                protocol: Protocol::V3,
                v3: V3_UNI,
            },
        ],
        10 => &[
            Venue {
                name: "Uniswap V2",
                address: "0x0c3c1c532F1e39EdF36BE9Fe0bE1410313E074Bf",
                protocol: Protocol::V2,
                v3: V2_ONLY,
            },
            Venue {
                name: "Uniswap V3",
                address: "0x1F98431c8aD98523631AE4a59f267346ea31F984",
                protocol: Protocol::V3,
                v3: V3_UNI,
            },
        ],
        137 => &[
            Venue {
                name: "Uniswap V2",
                address: "0x9e5A52f57b3038F1B8EeE45F28b3C1967e22799C",
                protocol: Protocol::V2,
                v3: V2_ONLY,
            },
            Venue {
                name: "Uniswap V3",
                address: "0x1F98431c8aD98523631AE4a59f267346ea31F984",
                protocol: Protocol::V3,
                v3: V3_UNI,
            },
        ],
        56 => &[
            Venue {
                name: "Uniswap V2",
                address: "0x8909Dc15e40173Ff4699343b6eB8132c65e18eC6",
                protocol: Protocol::V2,
                v3: V2_ONLY,
            },
            Venue {
                name: "Uniswap V3",
                address: "0xdB1d10011AD0Ff90774D0C6Bb92e5C5c8b4461F7",
                protocol: Protocol::V3,
                v3: V3_UNI,
            },
        ],
        43114 => &[
            Venue {
                name: "Uniswap V2",
                address: "0x9e5A52f57b3038F1B8EeE45F28b3C1967e22799C",
                protocol: Protocol::V2,
                v3: V2_ONLY,
            },
            Venue {
                name: "Uniswap V3",
                address: "0x740b1c1de25031C31FF4fC9A62f554A55cdC1baD",
                protocol: Protocol::V3,
                v3: V3_UNI,
            },
        ],
        8453 => &[
            Venue {
                name: "Uniswap V2",
                address: "0x8909Dc15e40173Ff4699343b6eB8132c65e18eC6",
                protocol: Protocol::V2,
                v3: V2_ONLY,
            },
            Venue {
                name: "Uniswap V3",
                address: "0x33128a8fC17869897dcE68Ed026d694621f6FDfD",
                protocol: Protocol::V3,
                v3: V3_UNI,
            },
        ],
        42220 => &[
            // The Celo UniswapV2Factory is deployed but has no pairs, so the
            // chain is covered by two independent V3-style venues instead.
            Venue {
                name: "Uniswap V3",
                address: "0xAfE208a311B21f13EF87E33A90049fC17A7acDEc",
                protocol: Protocol::V3,
                v3: V3_UNI,
            },
            Venue {
                name: "Velodrome Slipstream",
                address: "0x04625b046c69577efc40e6c0bb83cdbafab5a55f",
                protocol: Protocol::V3,
                v3: V3_SLIPSTREAM,
            },
        ],
        100 => &[
            Venue {
                name: "Uniswap V3",
                address: "0xe32F7dD7e3f098D518ff19A22d5f028e076489B1",
                protocol: Protocol::V3,
                v3: V3_UNI,
            },
            Venue {
                name: "SushiSwap V3",
                address: "0xf78031cbca409f2fb6876bdfdbc1b2df24cf9bef",
                protocol: Protocol::V3,
                v3: V3_UNI,
            },
            Venue {
                name: "Honeyswap V2",
                address: "0xa818b4f111ccac7aa31d0bcc0806d64f2e0737d7",
                protocol: Protocol::V2,
                v3: V2_ONLY,
            },
        ],
        59144 => &[
            Venue {
                name: "Uniswap V2",
                address: "0x114A43DF6C5f54EBB8A9d70Cd1951D3dD68004c7",
                protocol: Protocol::V2,
                v3: V2_ONLY,
            },
            Venue {
                name: "Uniswap V3",
                address: "0x31FAfd4889FA1269F7a13A66eE0fB458f27D72A9",
                protocol: Protocol::V3,
                v3: V3_UNI,
            },
        ],
        _ => &[],
    }
}

/// Backwards-compatible view: V2-compatible factories for a chain.
pub fn get_factories(chain_id: u64) -> Vec<Factory> {
    get_venues(chain_id)
        .iter()
        .filter(|v| v.protocol == Protocol::V2)
        .map(|v| Factory {
            name: v.name,
            address: v.address,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHAINS: [u64; 10] = [1, 42161, 10, 137, 56, 43114, 8453, 42220, 100, 59144];

    #[test]
    fn all_ten_chains_are_configured() {
        let configured: Vec<u64> = get_chains().iter().map(|c| c.id).collect();
        for id in CHAINS {
            assert!(
                configured.contains(&id),
                "chain {id} missing from get_chains()"
            );
        }
        assert_eq!(configured.len(), 10, "expected exactly 10 chains");
    }

    #[test]
    fn every_chain_has_a_wrapped_native() {
        for id in CHAINS {
            let w = get_wrapped_native(id);
            assert!(w.len() == 42, "chain {id} wrapped native is malformed: {w}");
        }
    }

    #[test]
    fn every_chain_has_a_name() {
        for id in CHAINS {
            assert_ne!(get_chain_name(id), "Unknown", "chain {id} has no name");
        }
    }

    /// Venue addresses must be real 20-byte hex, or the scanner reads a
    /// different contract and reports fabricated prices.
    #[test]
    fn venue_addresses_are_well_formed() {
        for id in CHAINS {
            for v in get_venues(id) {
                assert_eq!(v.address.len(), 42, "chain {id} {}: bad address", v.name);
                assert!(v.address.starts_with("0x"), "chain {id} {}", v.name);
                assert!(
                    v.address[2..].chars().all(|c| c.is_ascii_hexdigit()),
                    "chain {id} {}: non-hex address {}",
                    v.name,
                    v.address
                );
            }
        }
    }

    /// Every one of the ten chains must have at least one registry-listed venue,
    /// otherwise it is configured for RPC access but can never quote a price.
    #[test]
    fn every_chain_has_at_least_one_registry_venue() {
        for id in CHAINS {
            assert!(
                !get_venues(id).is_empty(),
                "chain {id} ({}) has no registry-listed venue",
                get_chain_name(id)
            );
        }
    }

    /// Spread detection is impossible with one venue: every DEX would quote the
    /// same pool and the difference would always be exactly zero.
    #[test]
    fn most_chains_have_multiple_venues() {
        for id in CHAINS {
            assert!(
                get_venues(id).len() >= 2,
                "chain {id} ({}) has {} venue(s); no spread is observable",
                get_chain_name(id),
                get_venues(id).len()
            );
        }
    }

    /// A quote leg is required to price anything against wrapped native.
    #[test]
    fn every_chain_has_a_stable_token() {
        for id in CHAINS {
            let s = get_stable_token(id);
            assert_eq!(s.len(), 42, "chain {id} stable token is malformed: {s}");
        }
    }

    /// A single factory per chain is not enough: `getPair` on one factory can
    /// only return that factory's own pools, so every "DEX" would quote an
    /// identical price and no spread could ever be observed.
    #[test]
    fn ethereum_has_multiple_venues() {
        assert!(get_venues(1).len() >= 2);
        assert!(!get_factories(1).is_empty());
    }

    /// The V2 compatibility shim must agree with the venue table.
    #[test]
    fn v2_shim_matches_venue_table() {
        for id in CHAINS {
            let expected = get_venues(id)
                .iter()
                .filter(|v| v.protocol == Protocol::V2)
                .count();
            assert_eq!(
                get_factories(id).len(),
                expected,
                "chain {id} v2 shim mismatch"
            );
            assert_eq!(
                has_v2_factory(id),
                expected > 0,
                "chain {id} has_v2_factory mismatch"
            );
        }
    }

    /// Chains with no registry-listed V2 market must still be covered by V3.
    #[test]
    fn v3_covers_chains_without_v2() {
        // Celo's V2 factory is deployed but holds no pairs, so it must be
        // served by V3-style venues.
        assert!(
            !has_v2_factory(42220),
            "Celo unexpectedly has a usable V2 market"
        );
        assert!(
            has_v3_factory(42220),
            "Celo must be served by V3-style venues"
        );
        // Every chain must have at least one V3-capable venue.
        for id in CHAINS {
            assert!(has_v3_factory(id), "chain {id} has no V3 venue");
        }
    }
    /// The DEX list must be a projection of the venue registry, so it can only
    /// report addresses the registry carries. It previously carried invented filler
    /// addresses for venues that do not exist, and covered just 6 of the 10 chains.
    #[test]
    fn dex_list_is_derived_from_the_venue_registry() {
        for id in CHAINS {
            let dexes = get_dexes_for_chain(id);
            let venues = get_venues(id);
            assert_eq!(
                dexes.len(),
                venues.len(),
                "chain {id}: dex list {} != venue count {}",
                dexes.len(),
                venues.len()
            );
            // An empty list makes the scan loop `continue`, which is how four
            // configured chains were silently never scanned.
            assert!(
                !dexes.is_empty(),
                "chain {id} ({}) produces an empty DEX list and is skipped",
                get_chain_name(id)
            );
            for (dex, venue) in dexes.iter().zip(venues.iter()) {
                assert_eq!(dex.name, venue.name, "chain {id} name mismatch");
                assert_eq!(dex.address, venue.address, "chain {id} address mismatch");
                assert_eq!(dex.chain_id, id);
                assert_eq!(dex.address.len(), 42, "chain {id} {} malformed", dex.name);
            }
        }
    }

    /// No address may repeat within a chain: two entries pointing at the same
    /// factory resolve the same pool, quote the same price, and can never show
    /// a spread.
    #[test]
    fn venue_addresses_are_unique_per_chain() {
        for id in CHAINS {
            let venues = get_venues(id);
            for (i, a) in venues.iter().enumerate() {
                for b in venues.iter().skip(i + 1) {
                    assert_ne!(
                        a.address.to_lowercase(),
                        b.address.to_lowercase(),
                        "chain {id}: {} and {} share a factory address",
                        a.name,
                        b.name
                    );
                }
            }
        }
    }

    /// `v3` is `Some` exactly when the protocol is V3, and every V3 venue must
    /// carry a pool signature plus at least one tier to probe.
    #[test]
    fn v3_venues_carry_pool_resolution_params() {
        for id in CHAINS {
            for v in get_venues(id) {
                match v.protocol {
                    Protocol::V3 => {
                        let p = v.v3.unwrap_or_else(|| {
                            panic!("chain {id} {}: V3 venue without params", v.name)
                        });
                        assert!(!p.fees.is_empty(), "chain {id} {}: no fee tiers", v.name);
                        assert!(
                            p.fees.iter().all(|f| *f > 0 && *f < (1 << 23)),
                            "chain {id} {}: a tier does not fit in int24/uint24",
                            v.name
                        );
                        assert!(
                            p.pool_sig == SIG_GET_POOL_UINT24 || p.pool_sig == SIG_GET_POOL_INT24,
                            "chain {id} {}: unknown pool signature {}",
                            v.name,
                            p.pool_sig
                        );
                    }
                    Protocol::V2 => assert!(
                        v.v3.is_none(),
                        "chain {id} {}: V2 venue must not carry V3 params",
                        v.name
                    ),
                }
            }
        }
    }

    /// The two V3 families resolve pools with different selectors. Giving a
    /// Slipstream factory the `uint24` form (or vice versa) returns the zero
    /// address, silently dropping every pool on that venue.
    #[test]
    fn slipstream_venues_use_the_int24_selector() {
        for id in CHAINS {
            for v in get_venues(id) {
                let Some(p) = v.v3 else { continue };
                if v.name.contains("Slipstream") {
                    assert_eq!(p.pool_sig, SIG_GET_POOL_INT24, "chain {id} {}", v.name);
                } else {
                    assert_eq!(p.pool_sig, SIG_GET_POOL_UINT24, "chain {id} {}", v.name);
                }
            }
        }
    }
}

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
    read_rpc_urls_for_chain(env_key, default, 0)
}

/// Read env-var URLs then merge with the built-in RPC catalog so every chain
/// has 15-27+ failover endpoints. Env-var URLs come first (operator-
/// controlled, possibly authenticated), then the curated free public catalog.
fn read_rpc_urls_for_chain(env_key: &str, default: &str, chain_id: u64) -> Vec<String> {
    let primary = std::env::var(env_key).unwrap_or_else(|_| default.to_string());
    let mut urls: Vec<String> = Vec::new();

    // Env-var URLs first (operator-controlled, possibly authenticated)
    let parts: Vec<&str> = primary
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if parts.len() > 1 {
        urls.extend(parts.iter().map(|s| s.to_string()));
    } else {
        urls.push(primary);
    }

    // Numbered fallbacks: KEY_1, KEY_2, ... KEY_5
    for i in 1..=5 {
        let fallback_key = format!("{}_{}", env_key, i);
        if let Ok(url) = std::env::var(&fallback_key) {
            let url = url.trim().to_string();
            if !url.is_empty() {
                urls.push(url);
            }
        }
    }

    // Merge in ALL endpoints from the built-in RPC catalog for this chain.
    if chain_id > 0 {
        let catalog = crate::rpc_catalog::all_default_endpoints(chain_id);
        urls.extend(catalog);
    }

    // Deduplicate while preserving order (env-var URLs stay at front).
    let mut seen = std::collections::HashSet::new();
    urls.retain(|u| seen.insert(u.clone()));

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
                rpc_urls: read_rpc_urls_for_chain("ETH_RPC_URL", "https://eth.merkle.io", 1),
                native_currency: "ETH".to_string(),
                explorer_url: "https://etherscan.io".to_string(),
            },
            ChainConfig {
                id: 42161,
                name: "Arbitrum".to_string(),
                rpc_url: std::env::var("ARB_RPC_URL")
                    .unwrap_or_else(|_| "https://arb1.arbitrum.io/rpc".to_string()),
                rpc_urls: read_rpc_urls_for_chain("ARB_RPC_URL", "https://arb1.arbitrum.io/rpc", 42161),
                native_currency: "ETH".to_string(),
                explorer_url: "https://arbiscan.io".to_string(),
            },
            ChainConfig {
                id: 10,
                name: "Optimism".to_string(),
                rpc_url: std::env::var("OP_RPC_URL")
                    .unwrap_or_else(|_| "https://mainnet.optimism.io".to_string()),
                rpc_urls: read_rpc_urls_for_chain("OP_RPC_URL", "https://mainnet.optimism.io", 10),
                native_currency: "ETH".to_string(),
                explorer_url: "https://optimistic.etherscan.io".to_string(),
            },
            ChainConfig {
                id: 137,
                name: "Polygon".to_string(),
                rpc_url: std::env::var("POLY_RPC_URL")
                    .unwrap_or_else(|_| "https://polygon-rpc.com".to_string()),
                rpc_urls: read_rpc_urls_for_chain("POLY_RPC_URL", "https://polygon-rpc.com", 137),
                native_currency: "MATIC".to_string(),
                explorer_url: "https://polygonscan.com".to_string(),
            },
            ChainConfig {
                id: 56,
                name: "BSC".to_string(),
                rpc_url: std::env::var("BSC_RPC_URL")
                    .unwrap_or_else(|_| "https://bsc-dataseed.binance.org".to_string()),
                rpc_urls: read_rpc_urls_for_chain("BSC_RPC_URL", "https://bsc-dataseed.binance.org", 56),
                native_currency: "BNB".to_string(),
                explorer_url: "https://bscscan.com".to_string(),
            },
            ChainConfig {
                id: 43114,
                name: "Avalanche".to_string(),
                rpc_url: std::env::var("AVAX_RPC_URL")
                    .unwrap_or_else(|_| "https://api.avax.network/ext/bc/C/rpc".to_string()),
                rpc_urls: read_rpc_urls_for_chain("AVAX_RPC_URL", "https://api.avax.network/ext/bc/C/rpc", 43114),
                native_currency: "AVAX".to_string(),
                explorer_url: "https://snowtrace.io".to_string(),
            },
            ChainConfig {
                id: 8453,
                name: "Base".to_string(),
                rpc_url: std::env::var("BASE_RPC_URL")
                    .unwrap_or_else(|_| "https://mainnet.base.org".to_string()),
                rpc_urls: read_rpc_urls_for_chain("BASE_RPC_URL", "https://mainnet.base.org", 8453),
                native_currency: "ETH".to_string(),
                explorer_url: "https://basescan.org".to_string(),
            },
            ChainConfig {
                id: 42220,
                name: "Celo".to_string(),
                rpc_url: std::env::var("CELO_RPC_URL")
                    .unwrap_or_else(|_| "https://forno.celo.org".to_string()),
                rpc_urls: read_rpc_urls_for_chain("CELO_RPC_URL", "https://forno.celo.org", 42220),
                native_currency: "CELO".to_string(),
                explorer_url: "https://celoscan.io".to_string(),
            },
            ChainConfig {
                id: 100,
                name: "Gnosis".to_string(),
                rpc_url: std::env::var("GNOSIS_RPC_URL")
                    .unwrap_or_else(|_| "https://rpc.gnosischain.com".to_string()),
                rpc_urls: read_rpc_urls_for_chain("GNOSIS_RPC_URL", "https://rpc.gnosischain.com", 100),
                native_currency: "xDAI".to_string(),
                explorer_url: "https://gnosisscan.io".to_string(),
            },
            ChainConfig {
                id: 59144,
                name: "Linea".to_string(),
                rpc_url: std::env::var("LINEA_RPC_URL")
                    .unwrap_or_else(|_| "https://rpc.linea.build".to_string()),
                rpc_urls: read_rpc_urls_for_chain("LINEA_RPC_URL", "https://rpc.linea.build", 59144),
                native_currency: "ETH".to_string(),
                explorer_url: "https://lineascan.build".to_string(),
            },
            // ─── High-throughput EVM chains (added for latency-sensitive arb) ───
            ChainConfig {
                id: 146,
                name: "Sonic".to_string(),
                rpc_url: std::env::var("SONIC_RPC_URL")
                    .unwrap_or_else(|_| "https://rpc.soniclabs.com".to_string()),
                rpc_urls: read_rpc_urls_for_chain("SONIC_RPC_URL", "https://rpc.soniclabs.com", 146),
                native_currency: "S".to_string(),
                explorer_url: "https://sonicscan.org".to_string(),
            },
            ChainConfig {
                id: 130,
                name: "Unichain".to_string(),
                rpc_url: std::env::var("UNICHAIN_RPC_URL")
                    .unwrap_or_else(|_| "https://mainnet.unichain.org".to_string()),
                rpc_urls: read_rpc_urls_for_chain("UNICHAIN_RPC_URL", "https://mainnet.unichain.org", 130),
                native_currency: "ETH".to_string(),
                explorer_url: "https://unichain.blockscout.com".to_string(),
            },
            ChainConfig {
                id: 534352,
                name: "Scroll".to_string(),
                rpc_url: std::env::var("SCROLL_RPC_URL")
                    .unwrap_or_else(|_| "https://rpc.scroll.io".to_string()),
                rpc_urls: read_rpc_urls_for_chain("SCROLL_RPC_URL", "https://rpc.scroll.io", 534352),
                native_currency: "ETH".to_string(),
                explorer_url: "https://scrollscan.com".to_string(),
            },
            ChainConfig {
                id: 324,
                name: "zkSync Era".to_string(),
                rpc_url: std::env::var("ZKSYNC_RPC_URL")
                    .unwrap_or_else(|_| "https://mainnet.era.zksync.io".to_string()),
                rpc_urls: read_rpc_urls_for_chain("ZKSYNC_RPC_URL", "https://mainnet.era.zksync.io", 324),
                native_currency: "ETH".to_string(),
                explorer_url: "https://explorer.zksync.io".to_string(),
            },
            ChainConfig {
                id: 5000,
                name: "Mantle".to_string(),
                rpc_url: std::env::var("MANTLE_RPC_URL")
                    .unwrap_or_else(|_| "https://rpc.mantle.xyz".to_string()),
                rpc_urls: read_rpc_urls_for_chain("MANTLE_RPC_URL", "https://rpc.mantle.xyz", 5000),
                native_currency: "MNT".to_string(),
                explorer_url: "https://mantlescan.xyz".to_string(),
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
        // Sonic's wrapped S — verified on-chain (decimals 18).
        146 => "0x039e2fB66102314Ce7b64Ce5Ce3E5183bc94aD38",
        // Unichain and Scroll use the standard WETH9 deployments.
        130 => "0x4200000000000000000000000000000000000006",
        534352 => "0x5300000000000000000000000000000000000004",
        // zkSync Era WETH9.
        324 => "0x5AEa5775959fBC2557Cc8789bC1bf90A239D9a91",
        // Mantle pools quote against bridged WETH (0xdEAd…1111, symbol "WETH",
        // verified on-chain) — the documented WMNT address carries no code.
        // NOTE: the USD rate env for chain 5000 must therefore be ETH's price.
        5000 => "0xdEAddEaDdeadDEadDEADDEAddEADDEAddead1111",
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
        // Bridged USDC.e on Sonic; native USDC on Unichain/Scroll/zkSync.
        146 => "0x29219dd400f2Bf60E5a23d13Be72B486D4038894",
        130 => "0x078D782b760474a361bDA0F3bCf0c1b71dDbc20B",
        534352 => "0x06eFdBFf2a14a7c8E15944D1F4A48F9F95F663A4",
        324 => "0x1d17CBcF0D6D143135aE902365D2E5e2A16538D4",
        5000 => "0x09Bc4E0D864854c6aFB6eB9A9cdF58aC190D0dF9",
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
        146 => "Sonic",
        130 => "Unichain",
        534352 => "Scroll",
        324 => "zkSync Era",
        5000 => "Mantle",
        _ => "Unknown",
    }
}

/// Resolve a token *symbol* to its contract address on `chain_id`.
///
/// The scanner needs an address per chain: the same symbol is a different
/// contract on every network, and passing one chain's address to another
/// resolves no pool at all. Resolution therefore went through
/// `RadarScanner::resolve_token_address`, which was a stub returning `Err`,
/// so every symbol-only scan failed outright.
///
/// This table is the fix: the canonical, widely-replicated assets per chain.
/// A symbol that is genuinely absent resolves to `""` and the caller skips
/// that chain rather than quoting a wrong address.
///
/// Provenance is the important caveat. These are the published canonical
/// addresses, but they are *not* verified on-chain by this build. A wrong
/// entry does not error - it reads some other contract and produces a
/// plausible-looking price, which is worse than reporting nothing.
pub fn resolve_token_symbol(symbol: &str, chain_id: u64) -> &'static str {
    let s = symbol.trim().to_ascii_uppercase();
    match (s.as_str(), chain_id) {
        // ---- native / wrapped native -------------------------------------
        ("ETH" | "WETH", 1) => "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2",
        ("ETH" | "WETH", 42161) => "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1",
        ("ETH" | "WETH", 10) => "0x4200000000000000000000000000000000000006",
        ("ETH" | "WETH", 8453) => "0x4200000000000000000000000000000000000006",
        ("ETH" | "WETH", 100) => "0xe91D153E0b41518A2Ce8Dd3D7944Fa863463a97d",
        ("ETH" | "WETH", 59144) => "0xe5d7c2a44ffddf6b295a15c148167daaaf5cf34f",
        ("WMATIC" | "MATIC", 137) => "0x0d500B1d8E8eF31E21C99d1Db9A6444d3ADf1270",
        ("WBNB" | "BNB", 56) => "0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c",
        ("WAVAX" | "AVAX", 43114) => "0xB31f66AA3C1e785363F0875A1B74E27b85FD66c7",
        ("CELO", 42220) => "0x471EcE3750Da237f93B8e339c536989b8978a438",
        ("WXDAI", 100) => "0xe91D153E0b41518A2Ce8Dd3D7944Fa863463a97d",

        // ---- stablecoins --------------------------------------------------
        ("USDC", 1) => "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
        ("USDC", 42161) => "0xaf88d065e77c8cC2239327C5EDb3A432268e5831",
        ("USDC", 10) => "0x0b2C639c533813f4Aa9D7837CAf62653d097Ff85",
        ("USDC", 137) => "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174",
        ("USDC", 56) => "0x8AC76a51cc950d9822D68b83fE1Ad97B32Cd580d",
        ("USDC", 43114) => "0xB97EF9Ef8734C71904D8002F8b6Bc66Dd9c48a6E",
        ("USDC", 8453) => "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913",
        ("USDC", 42220) => "0x48065fbbe25f71c9282ddf5e1cd6d6a887483d5e", // USDCe
        ("USDC", 100) => "0xddafbb505ad214d7b80b1f830fccc89b60fb7a83",   // bridged
        ("USDC", 59144) => "0x176211869cA2b568f2A7D4EE941E073a821EE1ff",
        ("USDT", 1) => "0xdAC17F958D2ee523a2206206994597C13D831ec7",
        ("USDT", 42161) => "0xFd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9",
        ("USDT", 10) => "0x94b008aA00579c1307B0EF2c499aD98a8ce58e58",
        ("USDT", 137) => "0xc2132D05D31c914a87C6611C10748AEb04B58e8F",
        ("USDT", 56) => "0x55d398326f99059fF775485246999027B3197955",
        ("USDT", 43114) => "0x9702230A8Ea53601f5cd2dc00fDBc13d4dF4A8c7",
        ("USDT", 8453) => "0xfde4C96c8593536E31F229EA8f37b2ADa2699bb2",

        // Gnosis bridged tokens (from Gnosis official bridge + OmniBridge)
        ("USDT", 100) => "0x4ECaBa5870353805a9F068101A40E0f32ed605C6",
        ("DAI", 100) => "0xe91D153E0b41518A2Ce8Dd3D7944Fa863463a97d",   // WXDAI = DAI on Gnosis
        ("WBTC", 100) => "0x8e5bBbb09Ed1ebdE8674Cda39A0c169401db4252",

        // Linea bridged tokens (from Linea canonical bridge)
        ("USDT", 59144) => "0xA219439258ca9da29E9Cc4cE5596924745e12B93",
        ("DAI", 59144) => "0x4AF15ec2A0BD43Db75dd04E62FAA3B8EF36b00d5",
        ("WBTC", 59144) => "0x3aAB2285ddcDdaD8edf438C1bAB47e1a9D05a9b4",

        ("DAI", 1) => "0x6B175474E89094C44Da98b954EedeAC495271d0F",
        ("DAI", 42161) => "0xDA10009cBd5D07dd0CeCc66161FC93D7c9000da1",
        ("DAI", 10) => "0xDA10009cBd5D07dd0CeCc66161FC93D7c9000da1",
        ("DAI", 137) => "0x8f3Cf7ad23Cd3CaDbD9735AFf958023239c6A063",
        ("DAI", 56) => "0x1AF3F329e8BE154074D8769D1FFa4eE058B1DBc3",
        ("DAI", 8453) => "0x50c5725949A6F0c72E6C4a641F24049A917DB0Cb",

        // ---- wrapped BTC ---------------------------------------------------
        ("WBTC", 1) => "0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599",
        ("WBTC", 42161) => "0x2f2a2543B76A4166549F7aaB2e75Bef0aefC5B0f",
        ("WBTC", 10) => "0x68f180fcCe6836688e9084f035309E29Bf0A2095",
        ("WBTC", 137) => "0x1BFD67037B42Cf73acF2047067bd4F2C47D9BfD6",
        ("WBTC", 56) => "0x7130d2A12B9BCbFAe4f2634d864A1Ee1Ce3Ead9c",
        ("WBTC", 43114) => "0x50b7545627a5162F82A992c33b87aDc75187B218",
        ("WBTC", 8453) => "0x0555E30Da8f98308eB51565D3f9DeA4765DcE1D6",

        // ---- LINK (Chainlink) — $12.5M Ethereum V3 TVL, $1M Arbitrum ------
        ("LINK", 1) => "0x514910771AF9Ca656af840dff83E8264EcF986CA",
        ("LINK", 42161) => "0xf97f4df75117a78c1A5a0DBb814Af92458539FB4",
        ("LINK", 10) => "0x350a791Bfc2C21F9Ed5d10980Dad2e2638ffa7f6",
        ("LINK", 137) => "0x53E0bca35eC356BD5ddDFebbD1Fc0fD03FaBad39",
        ("LINK", 56) => "0xF8A0BF9cF54Bb92F17374d9e9A321E6a111a51bD",
        ("LINK", 43114) => "0x5947BB275c521040051D82396f893D0d34a62C10",
        ("LINK", 8453) => "0x88Fb150BDc53A65fe94Dea0c9BA0a6dAf8C6e196",

        // ---- UNI (Uniswap governance) — $5.1M Ethereum V3 TVL -------------
        ("UNI", 1) => "0x1f9840a85d5aF5bf1D1762F925BDADdC4201F984",
        ("UNI", 42161) => "0xFa7F8980b0f1E64A2062791cc3b0871572f1F7f0",
        ("UNI", 10) => "0x6fd9d7AD17242c41f7131d257212c54A0e816691",
        ("UNI", 137) => "0xb33EaAd8d922B1083446DC23f610c2567fB5180f",
        ("UNI", 8453) => "0xc3De830EA07524a0761646a6a4e4be0e114a3C83",

        // ---- AAVE — deep pools on Ethereum/Arbitrum ------------------------
        ("AAVE", 1) => "0x7Fc66500c84A76Ad7e9c93437bFc5Ac33E2DDaE9",
        ("AAVE", 42161) => "0xba5DdD1f9d7F570dc94a51479a000E3BCE967196",
        ("AAVE", 10) => "0x76FB31fb4af56892A25e32cFC43De717950c9278",
        ("AAVE", 137) => "0xD6DF932A45C0f255f85145f286eA0b292B21C90B",
        ("AAVE", 8453) => "0x63706e401c06aC8513145B7687A14804d17f2804",

        // ---- LDO (Lido DAO) — $1M+ Ethereum, liquid on Arbitrum -----------
        ("LDO", 1) => "0x5A98FcBEA516Cf06857215779Fd812CA3beF1B32",
        ("LDO", 42161) => "0x13Ad51ed4F1B7e9Dc168d8a00cB3f4dDD85EfA60",
        ("LDO", 10) => "0xFdb794692724153d1488CcdBE0C56c0C3a69b3B0",
        ("LDO", 137) => "0xC3C7d422809852031b44ab29EEC9F1EfF2A58756",

        // ---- CRV (Curve DAO) — deep Curve ecosystem pools ------------------
        ("CRV", 1) => "0xD533a949740bb3306d119CC777fa900bA034cd52",
        ("CRV", 42161) => "0x11cDb42B0EB46D95f990BeDD4695A6e3fA034978",
        ("CRV", 10) => "0x0994206dfE8De6Ec6920FF4D779B0d950605Fb53",
        ("CRV", 137) => "0x172370d5Cd63279eFa6d502DAB29171933a610AF",

        // ---- ARB (Arbitrum governance) — deep on Arbitrum ------------------
        ("ARB", 42161) => "0x912CE59144191C1204E64559FE8253a0e49E6548",
        ("ARB", 1) => "0xB50721BCf8d664c30412Cfbc6cf7a15145234ad1",
        ("ARB", 8453) => "0x1DEBd73E752bEaF79865Fd6446b0c970EaE7732f",

        // ---- OP (Optimism governance) — deep on Optimism -------------------
        ("OP", 10) => "0x4200000000000000000000000000000000000042",

        // ---- PEPE — high-volume meme, deep V3 pools ----------------------
        ("PEPE", 1) => "0x6982508145454Ce325dDbE47a25d4ec3d2311933",
        ("PEPE", 42161) => "0x25d887Ce7a35172C62FeBFD67a1856F20FaEBB00",
        ("PEPE", 56) => "0x25d887Ce7a35172C62FeBFD67a1856F20FaEBB00",
        ("PEPE", 43114) => "0xa659d083b677d6bFFe1CB704E1473b896727BE6d",

        // ---- SHIB — high-volume meme, Ethereum V3 TVL ---------------------
        ("SHIB", 1) => "0x95aD61b0a150d79219dCF64E1E6Cc01f0B64C4cE",

        // ---- MKR (Maker / Sky) — deep governance token --------------------
        ("MKR", 1) => "0x9f8F72aA9304c8B593d555F12eF6589cC3A579A2",
        ("MKR", 137) => "0x6f7C932e7684666C9fd1d44527765433e01fF61d",

        // ---- GRT (The Graph) — indexed across 4 chains --------------------
        ("GRT", 1) => "0xc944E90C64B2c07662A292be6244BDf05Cda44a7",
        ("GRT", 42161) => "0x9623063377AD1B27544C965cCd7342f7EA7e88C7",
        ("GRT", 137) => "0x5fe2B58c013d7601147DcDd68C143A77499f5531",

        // ---- PENDLE — yield trading, multi-chain --------------------------
        ("PENDLE", 1) => "0x808507121B80c02388fAd14726482e061B8da827",
        ("PENDLE", 42161) => "0x0c880f6761F1af8d9Aa9C466984b80DAb9a8c9e8",
        ("PENDLE", 10) => "0xBC7B1Ff1c6989f006a1185318eD4E7b5796e66E1",
        ("PENDLE", 56) => "0xb3Ed0A426155B79B898849803E3B36552f7ED507",
        ("PENDLE", 8453) => "0xA99F6E6785da0F5d6fb42495Fe424BCE029EEB3e",

        // ---- ENA (Ethena) — stablecoin ecosystem --------------------------
        ("ENA", 1) => "0x57e114B691Db790C35207b2e685D4A43181e6061",
        ("ENA", 42161) => "0x58538e6A46e07434d7E7375Bc268D3cb839C0133",
        ("ENA", 8453) => "0x58538e6A46e07434d7E7375Bc268D3cb839C0133",
        ("ENA", 10) => "0x58538e6A46e07434d7E7375Bc268D3cb839C0133",

        // ---- WLD (Worldcoin) — deep on Optimism + Ethereum ----------------
        ("WLD", 1) => "0x163f8C2467924be0ae7B5347228CABF260318753",
        ("WLD", 10) => "0xdC6fF44d5d932Cbd77B52E5612Ba0529DC6226F1",

        // ---- MORPHO — DeFi lending governance -----------------------------
        ("MORPHO", 1) => "0x58D97B57BB95320F9a05dc918Aef65434969c2B2",
        ("MORPHO", 8453) => "0xBAa5CC21fd487B8Fcc2F632f3F4E8D37262a0842",
        ("MORPHO", 42161) => "0x40BD670a58238e6e230c430Bbb5ce6Ec0d40Df48",

        // ---- 1INCH — DEX aggregator governance, multi-chain ---------------
        ("1INCH", 1) => "0x111111111117dC0aa78b770fA6A738034120C302",
        ("1INCH", 42161) => "0x6314C31A7a1652cE482cFFe247E9CB7c3f4BB9aF",
        ("1INCH", 10) => "0xAd42D013ac31486B73b6b059e748172994736426",
        ("1INCH", 56) => "0x111111111117dC0aa78b770fA6A738034120C302",
        ("1INCH", 8453) => "0xc5feCc3a29Fb57B5024eEc8a2239d4621e111CBe",
        ("1INCH", 59144) => "0x10F04e61Bd6019D6C9B0Bf4907e805a64b92EE3e",

        // ---- FXS (Frax Share) — Frax ecosystem governance -----------------
        ("FXS", 1) => "0x3432B6A60D23Ca0dFCa7761B7ab56459D9C964D0",
        ("FXS", 42161) => "0x9d2F299715D94d8A7E6F5eAA8E654E8c74a988A7",
        ("FXS", 137) => "0x1a3aCf6D19267E2d3e7f898f42803e90C9219062",
        ("FXS", 56) => "0xe48A3d7d0Bc88D552f730B62c006bC925eadB9eE",

        // ---- YFI (Yearn Finance) — deep DeFi governance -------------------
        ("YFI", 1) => "0x0bc529c00C6401aEF6D220BE8C6Ea1667F6Ad93e",
        ("YFI", 42161) => "0x82e3A8F066a6989666b031d916c43672085b1582",
        ("YFI", 137) => "0xDA537104d6A5edd53c6fBba9A898708E465260b6",
        ("YFI", 10) => "0x9046D36440290FfdE54FE0DD84Db8b1CfEE9107b",
        ("YFI", 8453) => "0x9eAF8C1E34F05a589EDa6BAFdF391cf6AD3CB239",

        // ---- FLOKI — high-volume meme across ETH+BSC ---------------------
        ("FLOKI", 1) => "0xcf0C122c6b73ff809C693DB761e7BaeBe62b6a2E",
        ("FLOKI", 56) => "0xfb5B838b6cfEEdC2873aB27866079AC55363D37E",

        // ---- PYUSD (PayPal USD) — regulated stablecoin --------------------
        ("PYUSD", 1) => "0x6c3ea9036406852006290770BEdFcAbA0e23A0e8",
        ("PYUSD", 137) => "0x99Af3eEa856556646c98C8b9b2548fE815240750",
        ("PYUSD", 42161) => "0x46850Ad61c2B7d64D08C9C754F45254596696984",

        // ---- RENDER — GPU compute token -----------------------------------
        ("RENDER", 1) => "0x6De037ef9aD2725EB40118Bb1702EBb27e4Aeb24",

        // ---- FET (Fetch.ai) — AI token ------------------------------------
        ("FET", 1) => "0xaea46A60368A7bD060eec7DF8CBa43b7EF41Ad85",
        ("FET", 56) => "0x031b41e504677879370e9DBcF937283A8691Fa7f",

        // ---- CVX (Convex Finance) — Curve ecosystem -----------------------
        ("CVX", 1) => "0x4e3FBD56CD56c3e72c1403e103b45Db9da5B9D2B",

        // ---- IMX (Immutable X) — gaming/NFT L2 ----------------------------
        ("IMX", 1) => "0xF57e7e7C23978C3cAEC3C3548E3D615c346e79fF",

        // ---- ENS (Ethereum Name Service) ----------------------------------
        ("ENS", 1) => "0xC18360217D8F7Ab5e7c516566761Ea12Ce7F9D72",

        // ---- STRK (StarkNet) — L2 governance ------------------------------
        ("STRK", 1) => "0xCa14007Eff0dB1f8135f4C25B34De49AB0d42766",

        // ---- MNT (Mantle) — L2 governance ---------------------------------
        ("MNT", 1) => "0x3c3a81e81dc49A522A592e7622A7E711c06bf354",

        // ---- WSTETH (Wrapped stETH) — liquid staking ----------------------
        ("WSTETH", 1) => "0x7f39C581F595B53c5cb19bD0b3f8dA6c935E2Ca0",
        ("WSTETH", 42161) => "0x5979D7b546E38E414F7E9822514be443A4800529",
        ("WSTETH", 10) => "0x1F32b1c2345538c0c6f582fCB022739c4A194Ebb",
        ("WSTETH", 8453) => "0xc1CBa3fCea344f92D9239c8C0558C6DAc2a3564e",
        ("WSTETH", 137) => "0x03b54A6e9a984069379fae1a4fC4dBAE93B3bCCD",
        ("WSTETH", 59144) => "0xB5bedd42000b71FddE22D3eE8a79Bd49A568fC8F",
        ("WSTETH", 100) => "0x6C76971f98945AE98dD7d4DFcA8711ebea946eA6",

        // ---- Liquid staking / restaking — rich peg-spread surface ---------
        ("STETH", 1) => "0xae7ab96520DE3A18E5e111B5EaAb095312D7fE84",
        ("RETH", 1) => "0xae78736Cd615f374D3085123A210448E74Fc6393",
        ("RETH", 42161) => "0xEC70Dcb4A1EFa114bFF03bD271272032021E39cd",
        ("RETH", 10) => "0x9Bcef72be871e61ED4fBbc7630889bE7585CdD4B",
        ("RETH", 8453) => "0xB6fe221Fe9Eef5aBa221c348bA20A1Bf5e73624c",
        ("CBETH", 1) => "0xBe9895146f7AF43049ca1c1AE358B0541E497776",
        ("CBETH", 42161) => "0x11cDb42B0EB46D95f990BeDD4695A80e7fA3C77b",
        ("CBETH", 8453) => "0x2Ae3F1Ec7F1F5012CFEab0185bfc7aa3cf0DEc22",
        ("EZETH", 59144) => "0x2416092f143378750bb29b79eD961ab195CcEea5",
        ("WEETH", 59144) => "0x1Bf74C010E6320bab11e2e5A532b5AC15e0b8aA6",
        ("WRSETH", 59144) => "0xD2671165570f41BBB3B0097893300b6EB6102E6C",
        ("SAVAX", 43114) => "0x2b2C81e08f1Af8835a78Bb2A90AE924ACE0eA4bE",
        ("GGAVAX", 43114) => "0xA25EaF2906FA1a3a13EdAc9B9657108Af7B703e3",
        ("STMATIC", 137) => "0x3A58a54C066FdC0f2D55FC9C89F0415C92eBf3C4",
        ("MATICX", 137) => "0xfa68FB4628DFF1028CFEc22b4162FCcd0d45efb6",
        ("STCELO", 42220) => "0xC668583dcbDc9ae6FA3CE46462758188adfdfC24",
        ("SDAI", 100) => "0xaf204776c7245bF4147c2612BF6e5972Ee483701",

        // ---- Stablecoin variants — depeg spreads are the core arb ---------
        ("USDC_E", 42161) => "0xFF970A61A04b1cA14834A43f5dE4533eBDDB5CC8",
        ("USDC_E", 10) => "0x7F5c764cBc14f9669B88837ca1490cCa17c31607",
        ("USDC_E", 137) => "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174",
        ("USDC_E", 43114) => "0xA7D7079b0FEaD91F3e65f86E8915Cb59c1a4C664",
        ("USDT_E", 43114) => "0xc7198437980c041c805A1EDcbA50c1Ce5db95118",
        ("DAI_E", 43114) => "0xd586E7F844cEa2F87f50152665BCbc2C279D8d70",
        ("BUSD", 56) => "0xe9e7CEA3DedcA5984780Bafc599bD69ADd087D56",
        ("FDUSD", 56) => "0xc5f0f7b66764F6ec8C8Dff7BA683102295E16409",
        ("USDE", 1) => "0x4c9EDD5852cd905f086C759E8383e09bff1E68B3",
        ("USDE", 42161) => "0x5d3a1Ff2b6BAb83b63cd9AD0787074081a52ef34",
        ("USDE", 8453) => "0x5d3a1Ff2b6BAb83b63cd9AD0787074081a52ef34",
        ("USDE", 59144) => "0x5d3a1Ff2b6BAb83b63cd9AD0787074081a52ef34",
        ("SUSDE", 1) => "0x9D39A5DE30e57443BfF2A8307A4256c8797A3497",
        ("FRAX", 1) => "0x853d955aCEf822Db058EB8451b48d3d24B4f9819",
        ("FRAX", 42161) => "0x17FC002b466eEc40DaE837Fc4bE5c67993ddBd6F",
        ("FRAX", 10) => "0x2E3D870790dC77A83DD1d18184ACC7439A53f475",
        ("FRAX", 43114) => "0xD24C2Ad096400B6FBcd2ad8B24E7acBc21A1da64",
        ("LUSD", 1) => "0x5f98805A4E8be255a32880FDeC7F6728C6568bA0",
        ("LUSD", 42161) => "0x93b346b6BC2548dA6A5E7a98d78E21E87e1e616D",
        ("LUSD", 10) => "0xc40F949F8a4e094D1b49a23ea9241D289B7b2819",
        ("LUSD", 8453) => "0x368181499736d0c0CC614DBB145E2EC1AC86b8c6",
        ("DOLA", 42161) => "0x6A7661795C374c0bFC635934efAddFf3A7Ee23b6",
        ("DOLA", 10) => "0x8aE125E679382fc703b0A50038dd707e0221e4a0",
        ("DOLA", 8453) => "0x4621b7A9c75199271F773Ebd9A499dbd165c3191",
        ("MIM", 42161) => "0xFEa7a6a0B346362BF88A9e4A67916B6a73D0d597",
        ("MIM", 43114) => "0x130966628846BFd36ff31a822705796e8cb8C18D",
        ("MIM", 137) => "0x49a0400587A7F65072c87c4910449fDcC5c47242",
        ("SUSD", 10) => "0x8c6f28f2F1a3C87F0f938b96d27520d9751ec8d9",
        ("MAI", 10) => "0xdFA46478F9e5EA86d57387849598dbFB2e964b02",
        ("MAI", 137) => "0xa3Fa99A148fA48D14Ed51d610c367C61876997F1",
        ("HAI", 10) => "0x10398AbC267496E49106B07dd6BE13364D10dC71",
        ("EURE", 100) => "0xcB444e90D8198415266c6a2724b7900fb12FC56E",
        ("CUSD", 42220) => "0x765DE816845861e75A25fCA122bb6898B8B1282a",
        ("CEUR", 42220) => "0xD8763CBa276a3738E6DE85b4b3bF5FDed6D6cA73",
        ("CREAL", 42220) => "0xe8537a3d056DA446677B9E9d6c21dB704EaAb927",
        ("USDS", 1) => "0xdC035D45d973E3EC169d2276DDab16f1e407384F",
        ("USDS", 8453) => "0x820C137fa70C8691f0e44Dc420a5e53c168921Dc",
        ("USDT", 42220) => "0x48065fbBE25f71C9282ddf5e1cD6D6A887483D5e",

        // ---- DeFi governance / DEX tokens ---------------------------------
        ("COMP", 1) => "0xc00e94Cb662C3520282E6f5717214004A7f26888",
        ("SNX", 1) => "0xC011a73ee8576Fb46F5E1c5751cA3B9Fe0af2a6F",
        ("SNX", 10) => "0x8700dAec35aF8Ff88c16BdF0418774CB3D7599B4",
        ("SNX", 137) => "0x50B728D8D964fd00C2d0AAD81718b71311feF68a",
        ("BAL", 1) => "0xba100000625a3754423978a60c9317c58a424e3D",
        ("BAL", 42161) => "0x040d1EdC9569d4Bab2D15287Dc5A4F10F56a56B8",
        ("BAL", 10) => "0xFE8B128bA8C78aabC59d4c64cEE7fF28e9379921",
        ("BAL", 137) => "0x9a71012B13CA4d3D0Cdc72A177DF3ef03b0E76A3",
        ("BAL", 100) => "0x7eF541E2a22058048904fE5744f9c7E4C7AF6cCC",
        ("SUSHI", 1) => "0x6B3595068778DD592e39A122f4f5a5cF09C90fE2",
        ("SUSHI", 42161) => "0xd4d42F0b6DEF4CE0383636770eF773390d85c61A",
        ("SUSHI", 137) => "0x0b3F868E0BE5597D5DB7fEB59E1CADBb0fdDa50a",
        ("SUSHI", 43114) => "0x37B608519F91f70F2EeB0e5Ed9AF4061722e4F76",
        ("DYDX", 1) => "0x92D6C1e31e14520e676a687F0a93788B716Beff5",
        ("QNT", 1) => "0x4a220E6096B25EADb88358cb44068A3248254675",
        ("MANA", 1) => "0x0F5D2fB29fb7d3CFeE444a200298f468908cC942",
        ("MANA", 137) => "0xA1c57f48F0Deb89f569dFbE6E2B7f46D33606fD4",
        ("SAND", 1) => "0x3845badAde8e6dFF049820680d1F14bD3903a5d0",
        ("SAND", 137) => "0xBbba073C31bF03b8ACf7c28EF0738DeCF3695683",
        ("SAFE", 1) => "0x5aFE3855358E112B5647B952709E6165e1c1eAAe",
        ("SAFE", 100) => "0x4d18815D14fe5c3304e87B3FA18318baa5c23820",
        ("SKY", 1) => "0x56072C95FAA701256059aa122697B133aDEd9279",
        ("TRB", 1) => "0x88dF592F8eb5D7Bd38bFeF7dEb0fBc02cf3778a0",
        ("API3", 1) => "0x0b38210ea11411557c13457D4dA7dC6ea731B88a",
        ("ONDO", 1) => "0xfAbA6f8e4a5E8Ab82F62fe7C39859FA577269BE3",
        ("PAXG", 1) => "0x45804880De22913dAFE09f4980848ECE6EcbAf78",
        ("TBTC", 1) => "0x18084fbA666a33d37592fA2633fD49a74DD93a88",
        ("TBTC", 42161) => "0x6c84a8f1c29108F47a79964b5Fe888D4f4D959dE",
        ("POL", 1) => "0x455e53CBB86018Ac2B8092FdCd39d8444aFFC3F6",
        ("POL", 137) => "0x455e53CBB86018Ac2B8092FdCd39d8444aFFC3F6",
        ("AXL", 42161) => "0x23ee2343B892b1BB63503a4FAbc840E0e2C6810f",
        ("AXL", 56) => "0x8b1f4432F943c465A973FeDC6d7aa50Fc96f1f65",
        ("TIA", 42161) => "0xD56734d7f9979dD94FAE3d67C7a9280e71dBBd31",

        // ---- Arbitrum ecosystem -------------------------------------------
        ("GMX", 42161) => "0xfc5A1A6EB076a2C7aD06eD22C90d7E710E35ad0a",
        ("GMX", 43114) => "0x62edc0692BD897d2295872a9FFCac5425011c661",
        ("MAGIC", 42161) => "0x539bdE0d7Dbd336b79148AA742883198BBF60342",
        ("RDNT", 42161) => "0x3082CC23568eA640225c2467653dB90e9250AaA0",
        ("STG", 42161) => "0x6694340fc020c5E6B96567843da2df01b2CE1eb6",
        ("STG", 10) => "0x296F55F8Fb28E498B858d0CdDA06D955B2Cb3f97",
        ("STG", 137) => "0x2F6F07CDcf3588944Bf4C42aC74ff24bF56e7590",
        ("STG", 43114) => "0x2F6F07CDcf3588944Bf4C42aC74ff24bF56e7590",
        ("GRAIL", 42161) => "0x3d9907F9a368ad0a51Be60f7Da3b97cf940982D8",
        ("DPX", 42161) => "0x6C2C06790b3E3E3c38e12Ee22F8183b37a13EE55",
        ("SPELL", 42161) => "0x3E6648C5a70A150A88bCE65F4aD4d506Fe15d2AF",
        ("SYN", 42161) => "0x080F6AEd32Fc474DD5717105bDB5eC5721C1d3Ef",
        ("GNS", 42161) => "0x18c11FD286C5EC11c3b683Caa813B77f5163A122",
        ("GNS", 137) => "0xE5417Af564e4bFDA1c483642db72007871397896",
        ("WOO", 42161) => "0xcAFcD85D8ca7Ad1e1C6F82F651fA15E33AEfD07b",
        ("WOO", 137) => "0x1B815d120B3eF02039Ee11dC2d33DE7aA4a8C603",
        ("WOO", 10) => "0x871f2F2ff935FD1eD867842FF2a7bfD051A5E527",
        ("WOO", 56) => "0x4691937a7508860F876c9c0a2a617E7d9E945D4B",
        ("PLS", 42161) => "0x51318B7D00db7ACc4026C88c3952B66278B6A67F",
        ("WINR", 42161) => "0xD77B108d4f6cefaa0Cae9506A934e824BEccA46B",
        ("DMT", 42161) => "0x8B0E6f19Ee57089F7649A455D89D7bC6314D04e8",
        ("UNI", 56) => "0xBf5140A22578168FD562DCcF235E5D43A02ce9b1",
        ("AAVE", 56) => "0xfb6115445Bff7b52FeB98650C87f4493E107f802",
        // Bridged WETH variants — real tokens, but NOT the wrapped-native
        // quote leg (WAVAX / CELO), so they must not claim the "WETH" symbol.
        ("WETH_E", 43114) => "0x49D5c2BdFfac6CE2BFdB6640F4F80f226bc10bAB",
        ("WETH_E", 42220) => "0x122013fd7dF1C6F636a5bb8f03108E876548b455",
        ("WBTC", 42220) => "0xD629eb00dEced2a080B7EC630eF6aC117e614f1b",
        ("ETH", 56) => "0x2170Ed0880ac9A755fd29B2688956BD959F933F8",
        ("DAI", 43114) => "0xd586E7F844cEa2F87f50152665BCbc2C279D8d70",

        // ---- Optimism ecosystem -------------------------------------------
        ("VELO", 10) => "0x9560e827aF36c94D2Ac33a39bCE1Fe78631088Db",
        ("PERP", 10) => "0x9e1028F5F1D5eDE59748FFcee5532509976840E0",
        ("THALES", 10) => "0x217D47011b23BB961eB6D93cA9945B7501a5BB11",
        ("KWENTA", 10) => "0x920Cf626a271321C151D027030D5d08aF699456b",
        ("SONNE", 10) => "0x1DB2466d9F5e10D7090E7152B68d62703a2245F0",
        ("DHT", 10) => "0xAF9fE3B5cCDAe78188B1F8b9a49Da7ae9510F151",
        ("AELIN", 10) => "0x61BAADcF22d2565B0F471b291C475db5555e0b76",

        // ---- Base ecosystem — meme/AI tokens drive real volume ------------
        ("AERO", 8453) => "0x940181a94A35A4569E4529A3CDfB74e38FD98731",
        ("DEGEN", 8453) => "0x4ed4E862860beD51a9570b96d89aF5E1B0Efefed",
        ("BRETT", 8453) => "0x532f27101965dd16442E59d40670FaF5eBB142E4",
        ("TOSHI", 8453) => "0xAC1Bd2486aAf3B5C0fc3Fd868558b082a531B2B4",
        ("VIRTUAL", 8453) => "0x0b3e328455c4059EEb9e3f84b5543F74E24e7E1b",
        ("AIXBT", 8453) => "0x4F9Fd6Be4a90f2620860d680c0d4d5Fb53d1A825",
        ("CLANKER", 8453) => "0x1bc0c42215582d5A085795f4baDbaC3ff36d1Bcb",
        ("MOG", 8453) => "0x2Da56AcB9Ea78330f947bD57C54119DebdaB4035",
        ("HIGHER", 8453) => "0x0578d8A44db98B27BF358E9C8a6737fBf14198e4",
        ("MIGGLES", 8453) => "0xB1a03EdA10342529bBF8EB700a06C60441fEf25d",
        ("PRIME", 8453) => "0xFA980cEd6895AC314E7dE34Ef1bFAE90a5AdD21b",
        ("CBBTC", 1) => "0xcbB7C0000aB88B473b1f5aFd9ef808440eed33Bf",
        ("CBBTC", 8453) => "0xcbB7C0000aB88B473b1f5aFd9ef808440eed33Bf",
        ("WELL", 8453) => "0xA88594D404727625A9437C3f886C7643872296AE",

        // ---- Polygon ecosystem --------------------------------------------
        ("QUICK", 137) => "0x831753DD7087CaC61aB5644b308642cc1c33Dc13",
        ("GHST", 137) => "0x385Eeac5cB85A38A9a07A70c73e0a3271CfB54A7",
        ("OCEAN", 137) => "0x282d8efCe846A88B159800bd4130ad77443Fa1A1",
        ("DFYN", 137) => "0xC168E40227E4ebD8C1caAE80F7a55a4F0e6D66C5",
        ("TEL", 137) => "0xdF7837DE1F2Fa4631D716CF2502f8b230F1dcc32",

        // ---- BSC ecosystem — pegged majors are the deepest pools ----------
        ("CAKE", 56) => "0x0E09FaBB73Bd3Ade0a17ECC321fD13a19e81cE82",
        ("TWT", 56) => "0x4B0F1812e5Df2A09796481Ff14017e6005508003",
        ("XVS", 56) => "0xcF6BB5389c92Bdda8a3747Ddb454cB7a64626C63",
        ("DOGE", 56) => "0xbA2aE424d960c26247Dd6c32edC70B295c744C43",
        ("XRP", 56) => "0x1D2F0da169ceB9fC7B3143178cCa156BD176A682",
        ("ADA", 56) => "0x3EE2200Efb3400fAbB9AacF31297cBdD1d435D47",
        ("DOT", 56) => "0x7083609fCE4d1d8Dc0C979AAb8c869Ea2C873402",
        ("ATOM", 56) => "0x0Eb3a705fc54725037CC9e008bDede697f62F335",
        ("LTC", 56) => "0x4338665CBB7B2485A8855A139b75D5e34AB0DB94",
        ("TRX", 56) => "0x85EAC5Ac2F758618dFa09bDbe0cf174e7d574D5B",
        ("TON", 56) => "0x76A797A59Ba2C17726896976B7B3747BfD1d220f",
        ("MATIC", 56) => "0xCC42724C6683B7E57334c4E856f4c9965ED682bD",
        ("ANKR", 56) => "0xf307910A4c7bbc79691fD374889b36d8531B08e3",
        ("BSW", 56) => "0x965F527D9159dCe6288a2219DB51fc6Eef120dD1",
        ("ALPACA", 56) => "0x8F0528cE5eF7B51152A59745bEfDD91D97091d2F",
        ("DODO", 56) => "0x67ee3Cb086F8a16f34beE3ca5FAD36F7DbEBeEe5",
        ("BANANA", 56) => "0x603c7f932ED1fc6575303D8Fb018fDCBb0f39a95",
        ("CHESS", 56) => "0x20de22029ab63cf9A7Cf5fEB2b737Ca1eE4c62A6",
        ("C98", 56) => "0xaec945e04baf28b135fa7c640f624f8d90f1c3a6",
        ("SFP", 56) => "0xD41FDb03Ba84762dD66a0af1a6C8540FF1ba5dfb",
        ("CHR", 56) => "0x9FDc6ae99d28F8A90559d48016fF6Cfa06A19f91",
        ("EDU", 56) => "0xBdEAea03cA43a1c790FCFdA8fAe1c7f1772aE398",

        // ---- Avalanche ecosystem ------------------------------------------
        ("JOE", 43114) => "0x6e84a6216eA6dACC71eE8E6b0a5B7322EEbC0fDd",
        ("PNG", 43114) => "0x60781C2586D68229fde47564546784ab3fACA982",
        ("QI", 43114) => "0x8729438EB15e2C8B576fCc6AeCdA6A148776C0F5",
        ("PTP", 43114) => "0x22d4002028f537599bE9f666d1c4Fa138522f9c8",
        ("YAK", 43114) => "0x59414b3089ce2AF0010e7523Dea7E2b35d776ec7",
        ("COQ", 43114) => "0x420FcA0121DC28039145009570975747295f2329",
        ("KIMBO", 43114) => "0x184ff13B3EBCB25Be44e860163A5D8391Dd568c1",
        ("SNOB", 43114) => "0xC38f41A296A4493Ff429F1238e030924A1542e50",
        ("XAVA", 43114) => "0xd1c3f94DE7e5B45fa4EDBA47222a7e50B5a469A4",

        // ---- Linea ecosystem ------------------------------------------------
        ("FOXY", 59144) => "0x5FBDF89403270a1846F5ae7D113A989F850d1566",
        ("CROAK", 59144) => "0xaCb54d07cA167934F57F829BeE2cC665e1A5eFBF",
        ("LYNX", 59144) => "0x1a51b19CE03dbE0Cb44C1528E34a7EDD7771E9Af",
        ("MENDI", 59144) => "0x43E8809ea7486baA3E4be59a922A2e7D7cEa4A0E",

        // ---- Gnosis ecosystem ------------------------------------------------
        ("GNO", 100) => "0x9C58BAcC331c9aa871AFD802DB6379a98e80CEdb",
        ("COW", 100) => "0x177127622c4A00F3d409B75571e12cB3c8973d3c",
        ("OLAS", 100) => "0xcE11e14225575945b8E6Dc0D4F2dD4C570f79d9f",
        ("HNY", 100) => "0x71850b7E9Ee3f13Ab46d67167341E4bDc905Eef9",
        ("FOX", 100) => "0x21a42669643f45Bc0e086b8Fc2ed70c23D67509d",

        // ---- Celo ecosystem --------------------------------------------------
        ("UBE", 42220) => "0x00Be915B9dCf56a3CBE739D9B9c202ca692409EC",

        // ---- Sonic (146) — addresses verified on-chain ------------------------
        // wS is the wrapped native; USDC.e/USDT.e are the canonical bridged
        // stables; bridged WETH carries no code at the formerly documented
        // address, so it is intentionally absent.
        ("ETH" | "WETH" | "WS", 146) => "0x039e2fB66102314Ce7b64Ce5Ce3E5183bc94aD38",
        ("USDC" | "USDC_E", 146) => "0x29219dd400f2Bf60E5a23d13Be72B486D4038894",
        ("USDT" | "USDT_E", 146) => "0x6047828dc181963ba44974801FF68e538dA5eaF9",
        // Sonic ecosystem majors (canonical bridged/native deployments).
        ("WBTC" | "WBTC_E", 146) => "0x0555E30da8f98308EdB960aa94C0Db47230d2B9c",
        ("SCUSD", 146) => "0xd3DCe716f3eF535C5Ff8d041c1A41C3bd89b97aE",
        ("STS", 146) => "0xE5DA20F15420aD15DE0fa650600aFc998bbE3955",
        ("OS", 146) => "0xb1e25689D55734FD3ffFc939c4C3Eb52DFf8A794",
        ("SHADOW", 146) => "0x3333b97138D4b086720b5aE8A7844b1345a33333",
        ("BEETS", 146) => "0x2D0E0814E62D80056181f5cd932274405966e4F0",
        ("GOGLZ", 146) => "0x9fDbC3f8Abc05Fa8f3Ad3C17D2F806c1230c4564",
        ("SWPX", 146) => "0xA04BC7140c26fc9BB1F36B1A604C7A5a88fb5E70",
        // Additional tokens with live on-chain bytecode (verified getCode).
        ("WETH_B" | "BRIDGED_ETH", 146) => "0x50c42dEAcD8Fc9773493ED674b675bE577f2634b",
        ("SCETH", 146) => "0x3bcE5CB273F0F148010BbEa2470e7b5df84C7812",
        ("ANON", 146) => "0x79BBF4508B1391af3A0F4B30bb5FC4aa9ab0E07C",
        ("BRUSH", 146) => "0xE51EE9868C1f0d6cd968A8B8C8376Dc2991Bfe44",

        // ---- Unichain (130) ---------------------------------------------------
        ("ETH" | "WETH", 130) => "0x4200000000000000000000000000000000000006",
        ("USDC", 130) => "0x078D782b760474a361bDA0F3bCf0c1b71dDbc20B",
        ("USDT", 130) => "0x9151434b16b9763660701914891fA906F660EcC5",
        ("UNI", 130) => "0x8f187aA05619a017077f5308904739877ce9eA21",
        ("WBTC", 130) => "0x927B51f251480a681271180DA4de28D44EC4AfB8",

        // ---- Scroll (534352) --------------------------------------------------
        ("ETH" | "WETH", 534352) => "0x5300000000000000000000000000000000000004",
        ("USDC", 534352) => "0x06eFdBFf2a14a7c8E15944D1F4A48F9F95F663A4",
        ("USDT", 534352) => "0xf55BEC9cafDbE8730f096Aa55dad6D22d44099Df",
        ("WSTETH", 534352) => "0xf610A9dfB7C89644979b4A0f27063E9e7d7Cda32",
        ("RETH", 534352) => "0x53878B874283351D26d206FA971aBaCE0f12E422",
        ("WRSETH", 534352) => "0xa25b25548B4C98B0c7d3d27dcA5D5ca743d68b7F",
        ("SCR", 534352) => "0xd29687c813D741E2F938F4aC377128810E217b1b",
        ("WBTC", 534352) => "0x3C1BCa5a656e69edCD0D4E36BEbb3FcDAcA60Cf1",
        ("SOLVBTC", 534352) => "0x3bA89d490AB1C0c9CCaB33821bBcF9640F71BFbA",
        ("PENCIL", 534352) => "0x4Cf16d25a15c4f517B9B60EEc2eF308f8247cfee",
        ("NURI", 534352) => "0xAAAE8378809BB8815c08D3A59bBe31613db57Fc6",
        ("HOPI", 534352) => "0x5b577135c9De91cFC5247A28e92Eb4aBeDcB3aA8",
        ("STONE", 534352) => "0x80137510979822322193FC997d400D08A6C747bf",
        ("WEETH", 534352) => "0x01f0a31698C4d065659b9bdc21B3610292a1c506",

        // ---- zkSync Era (324) -------------------------------------------------
        ("ETH" | "WETH", 324) => "0x5AEa5775959fBC2557Cc8789bC1bf90A239D9a91",
        ("USDC", 324) => "0x1d17CBcF0D6D143135aE902365D2E5e2A16538D4",
        ("USDT", 324) => "0x493257fD37EDB34451f62EDf8D2a0C418852bA4C",
        ("USDC_E", 324) => "0x3355df6D4c9C3035724Fd0e3914dE96A5a83aaf4",
        ("WBTC", 324) => "0xBBeB516fb02a01611cBBE0453Fe3c580D7281011",
        ("ZK", 324) => "0x5A7d6b2F92C77FAD6CCaBd7EE0624E64907Eaf3E",
        ("WUSDM", 324) => "0xA900cbE7739c96D2B153a273953620A701d5442b",
        ("ZZ", 324) => "0x1ab721f531Cab4c777d536AEC8f5a90b21E8ecCf",
        ("SIS", 324) => "0xdd9f72afED3631a6C85b5369D84875e6c42f1827",
        ("WSTETH", 324) => "0x703b52F2b28fEbcB60E1372858AF5b18849FE867",
        ("RETH", 324) => "0x32Fd44bB869620C0BC993528c2a1Be3b1a50D6b8",
        ("MAV", 324) => "0x787c09494Ec8Bcb24DcAf8659E7d5D69979eE508",
        ("FIRE", 324) => "0x8f05f20D6Fa7ec33B0140D0e097DeCc97a801a42",
        // Verified getCode on mainnet.era.zksync.io.
        ("HOLD", 324) => "0xed4040fD47629e7c8FBB7DA76bb50B3e7695F0f2",
        ("MUTE", 324) => "0x0e97C7a0F8B2C9885C8ac9fC6136e829CbC21d42",
        ("DERI", 324) => "0x140D5bc5b62d6cB492B1A475127F50d531023803",

        // ---- Mantle (5000) ----------------------------------------------------
        // Pools quote in bridged WETH; MNT itself lives on the predeploy.
        ("ETH" | "WETH", 5000) => "0xdEAddEaDdeadDEadDEADDEAddEADDEAddead1111",
        ("MNT", 5000) => "0xDeadDeAddeAddEAddeadDeaDdeAdDeaDDeAD0000",
        ("USDT", 5000) => "0x201EBa5CC46D216Ce6DC03F6a759e8E766e956aE",
        ("USDC", 5000) => "0x09Bc4E0D864854c6aFB6eB9A9cdF58aC190D0dF9",
        ("USDE", 5000) => "0x5d3a1Ff2b6BAb83b63cd9AD0787074081a52ef34",
        ("SUSDE", 5000) => "0x211Cc4DD073734dA055fbF44a2b4667d5E5FE5d2",
        ("FBTC", 5000) => "0xC96dE26018A54D51c097160568752c4E3BD6C364",
        ("METH", 5000) => "0xcDA86A272531e8640cD7F1a92c01839911B90bb0",
        ("CMETH", 5000) => "0xE6829d9a7eE3040e1276Fa75293Bde931859e8fA",
        ("USDT0", 5000) => "0x779Ded0c9e1022225f8E0630b35a9b54bE713736",
        ("USDY", 5000) => "0x5bE26527e817998A7206475496fDE1E68957c5A6",
        ("AXLUSDC", 5000) => "0xEB466342C4d449BC9f53A865D5Cb90586f405215",
        // Canonical WMNT — getCode verified (6642 B). Earlier "no code" reads
        // came from a rate-limited endpoint, not the chain.
        ("WMNT", 5000) => "0x78c1b0C915c4FAA5FffA6CAbf0219DA63d7f4cb8",

        _ => "",
    }
}
/// The protocol that determines how a pool is resolved and priced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// Constant product: `getPair(a,b)` then read `getReserves()`.
    V2,
    /// Concentrated liquidity: `getPool(a,b,fee)` then read `slot0`/`liquidity`.
    V3,
    /// Solidly family (Velodrome V2, Aerodrome classic): the factory keys
    /// pools by a `stable` flag — `getPool(a,b,bool)` — while the pool itself
    /// still answers `getReserves()`, so state reads are V2-shaped.
    Solidly,
}

impl Protocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Protocol::V2 => "v2",
            Protocol::V3 => "v3",
            Protocol::Solidly => "solidly",
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

/// `getPool(address,address,bool)` — Solidly family (Velodrome V2,
/// Aerodrome). `bool` ABI-encodes as the same left-padded 32-byte 0/1 word
/// the uint24 encoder produces, so `factory_get_pool_call_data` handles it.
pub const SIG_GET_POOL_BOOL: &str = "getPool(address,address,bool)";

/// Solidly `fees` carry the stable flag: 0 = volatile pool, 1 = stable pool.
pub const SOLIDLY_STABLE_FLAGS: &[u32] = &[0, 1];

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
    /// `Some` exactly when `protocol` is [`Protocol::V3`] or
    /// [`Protocol::Solidly`] — both resolve pools through a keyed factory call.
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
/// Solidly family (Velodrome V2, Aerodrome classic): `bool` stable flag.
const V3_SOLIDLY: Option<V3Params> = Some(V3Params {
    fees: SOLIDLY_STABLE_FLAGS,
    pool_sig: SIG_GET_POOL_BOOL,
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
            Venue {
                name: "SushiSwap V2",
                address: "0xC0AEe478e3658e2610c5F7A4A2E1777cE9e4f2Ac",
                protocol: Protocol::V2,
                v3: V2_ONLY,
            },
            Venue {
                name: "PancakeSwap V3",
                address: "0x0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865",
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
            // Camelot V2 — major Arbitrum-native DEX
            Venue {
                name: "Camelot V2",
                address: "0x6EcCab422D763aC031210895C81787E87B43A652",
                protocol: Protocol::V2,
                v3: V2_ONLY,
            },
            // SushiSwap V3 — separate V3 factory on Arbitrum
            Venue {
                name: "SushiSwap V3",
                address: "0x1af415a1EbA07a4986a52B6f2e7dE7003D82231e",
                protocol: Protocol::V3,
                v3: V3_UNI,
            },
            // PancakeSwap V3 — canonical CREATE2 deployment, same as BSC/ETH
            Venue {
                name: "PancakeSwap V3",
                address: "0x0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865",
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
            // Velodrome Slipstream — largest DEX on Optimism by TVL
            Venue {
                name: "Velodrome Slipstream",
                address: "0xCc0bDDB707055e04e497aB22a59c2aF4391cd12F",
                protocol: Protocol::V3,
                v3: V3_SLIPSTREAM,
            },
            // Velodrome V2 (Solidly) — constant-product sibling of Slipstream;
            // pools are keyed by a stable flag, not a fee tier.
            Venue {
                name: "Velodrome V2",
                address: "0xF1046053aa5682b4F9a81b5481394DA16BE5FF5a",
                protocol: Protocol::Solidly,
                v3: V3_SOLIDLY,
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
            // QuickSwap V2 — dominant Polygon DEX, standard getPair factory.
            // (QuickSwap V3 is Algebra: it exposes poolByPair(a,b), not
            // getPool(a,b,fee), so every resolution call reverted.)
            Venue {
                name: "QuickSwap V2",
                address: "0x5757371414417b8C6CAad45bAeF941aBc7d3Ab32",
                protocol: Protocol::V2,
                v3: V2_ONLY,
            },
            // SushiSwap V2 on Polygon.
            Venue {
                name: "SushiSwap V2",
                address: "0xc35DADB65012eC5796536bD9864eD8773aBc74C4",
                protocol: Protocol::V2,
                v3: V2_ONLY,
            },
            Venue {
                name: "SushiSwap V3",
                address: "0x917933899c6a5F8E37F31E19f92CdBFF7e8FF0e2",
                protocol: Protocol::V3,
                v3: V3_UNI,
            },
            Venue {
                name: "ApeSwap V2",
                address: "0xCf083Be4164828f00cAE704EC15a36D711491284",
                protocol: Protocol::V2,
                v3: V2_ONLY,
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
            // PancakeSwap V2 — dominant DEX on BSC
            Venue {
                name: "PancakeSwap V2",
                address: "0xcA143Ce32Fe78f1f7019d7d551a6402fC5350c73",
                protocol: Protocol::V2,
                v3: V2_ONLY,
            },
            // PancakeSwap V3
            Venue {
                name: "PancakeSwap V3",
                address: "0x0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865",
                protocol: Protocol::V3,
                v3: V3_UNI,
            },
            // On-chain verified: the BSC ApeSwap factory is 0x0841BD0B… — the
            // 0xCf083Be… address is ApeSwap's *Polygon* deployment.
            Venue {
                name: "ApeSwap V2",
                address: "0x0841BD0B734E4F5853f0dD8d7Ea041c241fb0Da6",
                protocol: Protocol::V2,
                v3: V2_ONLY,
            },
            Venue {
                name: "BiSwap V2",
                address: "0x858E3312ed3A876947EA49d572A7C42DE08af7EE",
                protocol: Protocol::V2,
                v3: V2_ONLY,
            },
            Venue {
                name: "BakerySwap V2",
                address: "0x01bF7C66c6BD861915CdaaE475042d3c4BaE16A7",
                protocol: Protocol::V2,
                v3: V2_ONLY,
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
            // TraderJoe V1 — the dominant Avalanche V2 factory
            Venue {
                name: "TraderJoe V1",
                address: "0x9Ad6C38BE94206cA50bb0d90783181662f0CFC10",
                protocol: Protocol::V2,
                v3: V2_ONLY,
            },
            Venue {
                name: "Pangolin V2",
                address: "0xefa94DE7a4656D787667C749f7E1223D71E9FD88",
                protocol: Protocol::V2,
                v3: V2_ONLY,
            },
            Venue {
                name: "SushiSwap V2",
                address: "0xc35DADB65012eC5796536bD9864eD8773aBc74C4",
                protocol: Protocol::V2,
                v3: V2_ONLY,
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
            // Aerodrome Slipstream — largest DEX on Base by TVL
            Venue {
                name: "Aerodrome Slipstream",
                address: "0x5e7BB104d84c7CB9B682AaC2F3d509f5F406809A",
                protocol: Protocol::V3,
                v3: V3_SLIPSTREAM,
            },
            // SushiSwap V3 on Base
            Venue {
                name: "SushiSwap V3",
                address: "0xc35DADB65012eC5796536bD9864eD8773aBc74C4",
                protocol: Protocol::V3,
                v3: V3_UNI,
            },
            // Aerodrome classic (Solidly) — Base's largest AMM by volume;
            // `getPool(a,b,stable)` keyed pools behind the same factory that
            // also answers the Slipstream-style call for CL pools.
            Venue {
                name: "Aerodrome",
                address: "0x420DD381b31aEf6683db6B902084cB0FFECe40Ab",
                protocol: Protocol::Solidly,
                v3: V3_SOLIDLY,
            },
            Venue {
                name: "PancakeSwap V3",
                address: "0x0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865",
                protocol: Protocol::V3,
                v3: V3_UNI,
            },
            Venue {
                name: "BaseSwap V2",
                address: "0xFDa619b6d20975be80A10332cD39b9a4b0FAa8BB",
                protocol: Protocol::V2,
                v3: V2_ONLY,
            },
        ],
        42220 => &[
            // The Uniswap-deployed V2 factory on Celo holds no pairs, so the
            // chain is covered by two V3-style venues plus Ubeswap's V2.
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
            // Ubeswap — Celo's native V2 fork, the chain's dominant classic AMM
            Venue {
                name: "Ubeswap V2",
                address: "0x62d5b84bE28a183aBB507E125B384122D2C25fAE",
                protocol: Protocol::V2,
                v3: V2_ONLY,
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
            Venue {
                name: "SushiSwap V2",
                address: "0xc35DADB65012eC5796536bD9864eD8773aBc74C4",
                protocol: Protocol::V2,
                v3: V2_ONLY,
            },
            Venue {
                name: "Swapr V2",
                address: "0x5D48C95AdfFD4B40c1AAADc4e08fc44117E02117",
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
            // PancakeSwap on Linea — canonical V3 factory plus its V2 sibling
            Venue {
                name: "PancakeSwap V3",
                address: "0x0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865",
                protocol: Protocol::V3,
                v3: V3_UNI,
            },
            Venue {
                name: "PancakeSwap V2",
                address: "0x02a84c1b3BBD7401a5f7fa98a384EBC70bB5749E",
                protocol: Protocol::V2,
                v3: V2_ONLY,
            },
        ],
        // Sonic — all three verified live by probing factory.getPool for
        // wS/USDC.e (real pool addresses returned).
        146 => &[
            Venue {
                name: "Uniswap V3",
                address: "0xcb2436774C3e191c85056d248EF4260ce5f27A9D",
                protocol: Protocol::V3,
                v3: V3_UNI,
            },
            // Wagmi — UniV3 fork, factory derived from a live pool's
            // factory() and verified via getPool @3000/@10000.
            Venue {
                name: "Wagmi V3",
                address: "0x56cfc796bc88c9c7e1b38c2b0af9b7120b079aef",
                protocol: Protocol::V3,
                v3: V3_UNI,
            },
            // SpookySwap's Sonic deployment is V3-style: getPool(a,b,fee)
            // returned real pools @500/@3000.
            Venue {
                name: "SpookySwap V3",
                address: "0x3d91b700252e0e3ee7805d12e048a988ab69c8ad",
                protocol: Protocol::V3,
                v3: V3_UNI,
            },
        ],
        // Unichain — canonical Uniswap deployments (Uniswap's own chain).
        130 => &[
            Venue {
                name: "Uniswap V3",
                address: "0x1F98400000000000000000000000000000000003",
                protocol: Protocol::V3,
                v3: V3_UNI,
            },
            Venue {
                name: "Uniswap V2",
                address: "0x8909Dc15e40173Ff4699343b6eB8132c65e18eC6",
                protocol: Protocol::V2,
                v3: V2_ONLY,
            },
        ],
        // Scroll — all three verified live via factory.getPool on WETH/USDC
        // (real pool addresses returned @ multiple fee tiers).
        534352 => &[
            Venue {
                name: "Uniswap V3",
                address: "0x70C62C8b8e801124A4Aa81ce07b637A3e83cb919",
                protocol: Protocol::V3,
                v3: V3_UNI,
            },
            // KyberSwap Elastic — UniV3-fork signature, getPool/100 → live
            // pool 0x8518d5d6….
            Venue {
                name: "KyberSwap Elastic",
                address: "0xC7a590291e07B9fe9E64b86c58fD8fC764308C4A",
                protocol: Protocol::V3,
                v3: V3_UNI,
            },
            // Nuri CL (Ramses-family) — getPool/100,500,10000 → live pools.
            Venue {
                name: "Nuri CL",
                address: "0xAAA32926fcE6bE95ea2c51cB4Fcb60836D320C42",
                protocol: Protocol::V3,
                v3: V3_UNI,
            },
        ],
        // zkSync Era — both verified live with real WETH/USDC pools.
        324 => &[
            Venue {
                name: "PancakeSwap V3",
                address: "0x1BB72E0CbbEA93c08f535fc7856E0338D7F7a8aB",
                protocol: Protocol::V3,
                v3: V3_UNI,
            },
            Venue {
                name: "Uniswap V3",
                address: "0x8FdA5a7a8dCA67BBcDd10F02Fa0649A937215422",
                protocol: Protocol::V3,
                v3: V3_UNI,
            },
        ],
        // Mantle — Agni Finance (UniV3 fork) verified live: USDC/USDT pools
        // at fees 100/500/10000, WETH/USDT at 100/500/10000. Canonical
        // Uniswap V3 deployment also verified: getPool(WETH,USDT,500) →
        // live pool 0x076eb72e….
        5000 => &[
            Venue {
                name: "Agni Finance V3",
                address: "0x25780dc8Fc3cfBD75F33bFDAB65e969b603b2035",
                protocol: Protocol::V3,
                v3: V3_UNI,
            },
            Venue {
                name: "Uniswap V3",
                address: "0x0d922Fb1Bc191F64970ac40376643808b4B74Df9",
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

    const CHAINS: [u64; 15] = [
        1, 42161, 10, 137, 56, 43114, 8453, 42220, 100, 59144, 146, 130, 534352, 324, 5000,
    ];

    #[test]
    fn all_fifteen_chains_are_configured() {
        let configured: Vec<u64> = get_chains().iter().map(|c| c.id).collect();
        for id in CHAINS {
            assert!(
                configured.contains(&id),
                "chain {id} missing from get_chains()"
            );
        }
        assert_eq!(configured.len(), 15, "expected exactly 15 chains");
    }

    /// Every resolved token must be a syntactically valid EVM address.
    ///
    /// A malformed entry would be silently skipped at parse time inside the
    /// scanner, so the token would just never quote. Catching it here turns a
    /// silent "no liquidity" into a build failure.
    #[test]
    fn resolved_tokens_are_well_formed_addresses() {
        for id in CHAINS {
            for sym in [
                "USDC", "USDT", "DAI", "WETH", "ETH", "WBTC", "WMATIC", "WBNB", "WAVAX", "CELO",
                "WXDAI", "LINK", "UNI", "AAVE", "LDO", "CRV", "ARB", "OP",
            ] {
                let a = resolve_token_symbol(sym, id);
                if a.is_empty() {
                    continue; // not carried on this chain: allowed
                }
                assert_eq!(
                    a.len(),
                    42,
                    "chain {id} {sym}: address is not 0x + 40 hex chars: {a}"
                );
                assert!(a.starts_with("0x"), "chain {id} {sym}: missing 0x");
                assert!(
                    a[2..].chars().all(|c| c.is_ascii_hexdigit()),
                    "chain {id} {sym}: non-hex characters in {a}"
                );
            }
        }
    }

    /// The same symbol must not resolve to one address on two different chains.
    ///
    /// That mistake is invisible downstream - the scanner would quote a
    /// different chain's token under the requested name - so it is asserted
    /// directly.
    #[test]
    fn same_symbol_differs_across_chains() {
        let usdc_eth = resolve_token_symbol("USDC", 1);
        let usdc_base = resolve_token_symbol("USDC", 8453);
        let usdc_arb = resolve_token_symbol("USDC", 42161);
        assert!(!usdc_eth.is_empty() && !usdc_base.is_empty() && !usdc_arb.is_empty());
        assert_ne!(usdc_eth, usdc_base, "USDC must differ: Ethereum vs Base");
        assert_ne!(usdc_eth, usdc_arb, "USDC must differ: Ethereum vs Arbitrum");
    }

    /// Symbols are case-insensitive, and an unknown symbol resolves to empty
    /// (the caller then skips the chain) rather than to a plausible address.
    #[test]
    fn symbol_lookup_is_case_insensitive_and_fails_closed() {
        assert_eq!(
            resolve_token_symbol("usdc", 1),
            resolve_token_symbol("USDC", 1)
        );
        assert_eq!(
            resolve_token_symbol("  weth  ", 1),
            resolve_token_symbol("WETH", 1)
        );
        assert_eq!(resolve_token_symbol("NOT_A_REAL_TOKEN", 1), "");
        assert_eq!(resolve_token_symbol("USDC", 999_999), "");
    }

    /// Wrapped-native resolution must agree with `get_wrapped_native`, because
    /// the scanner prices a token *against* the wrapped native and a mismatch
    /// would make it look for pools that cannot exist.
    #[test]
    fn wrapped_native_symbols_match_the_quote_leg() {
        for id in CHAINS {
            let w = get_wrapped_native(id);
            if w.is_empty() {
                continue;
            }
            let resolved = resolve_token_symbol("WETH", id);
            if !resolved.is_empty() {
                assert_eq!(
                    resolved.to_lowercase(),
                    w.to_lowercase(),
                    "chain {id}: WETH symbol and quote leg disagree"
                );
            }
        }
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
        // The Uniswap-deployed V2 factory on Celo holds no pairs, so Celo is
        // served by Ubeswap (its native V2 fork) plus its V3 venues.
        assert!(
            has_v2_factory(42220),
            "Celo should resolve V2 through Ubeswap"
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
                    Protocol::Solidly => {
                        let p = v.v3.unwrap_or_else(|| {
                            panic!("chain {id} {}: Solidly venue without params", v.name)
                        });
                        assert_eq!(
                            p.pool_sig, SIG_GET_POOL_BOOL,
                            "chain {id} {}: Solidly venue must use getPool(a,b,bool)",
                            v.name
                        );
                        // Flags, not fees: 0 = volatile pool, 1 = stable pool.
                        assert_eq!(
                            p.fees, SOLIDLY_STABLE_FLAGS,
                            "chain {id} {}: Solidly venue must probe volatile+stable",
                            v.name
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
                if v.protocol == Protocol::Solidly {
                    assert_eq!(p.pool_sig, SIG_GET_POOL_BOOL, "chain {id} {}", v.name);
                } else if v.name.contains("Slipstream") {
                    assert_eq!(p.pool_sig, SIG_GET_POOL_INT24, "chain {id} {}", v.name);
                } else {
                    assert_eq!(p.pool_sig, SIG_GET_POOL_UINT24, "chain {id} {}", v.name);
                }
            }
        }
    }
}

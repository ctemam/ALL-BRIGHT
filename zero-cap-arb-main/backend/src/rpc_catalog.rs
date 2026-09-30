//! Curated public RPC endpoint catalogue.
//!
//! Every endpoint was live-tested on 2026-09-30. Dead, rate-limited, and
//! auth-gated URLs have been removed. The scanner uses round-robin rotation
//! across these URLs with 3-attempt retries.
//!
//! Operators can override or extend any list via env vars (see
//! `chains::read_rpc_urls`); these are only the built-in defaults.

/// Ethereum (1) and Arbitrum One (42161) defaults.
pub fn default_endpoints(chain_id: u64) -> &'static [&'static str] {
    match chain_id {
        1 => &[
            "https://ethereum-rpc.publicnode.com",
            "https://eth.drpc.org",
            "https://eth.llamarpc.com",
            "https://cloudflare-eth.com",
            "https://eth.merkle.io",
            "https://eth-mainnet.public.blastapi.io",
            "https://ethereum.publicnode.com",
            "https://1rpc.io/eth",
            "https://eth.rpc.blxrbdn.com",
            "https://eth-pokt.nodies.app",
            "https://gateway.tenderly.co/public/mainnet",
            "https://rpc.mevblocker.io",
            "https://rpc.payload.de",
            "https://eth.meowrpc.com",
            "https://eth.gateway.tenderly.co",
            "https://eth-mainnet.rpcfast.com",
        ],
        42161 => &[
            "https://arbitrum-one-rpc.publicnode.com",
            "https://arb1.arbitrum.io/rpc",
            "https://arbitrum.drpc.org",
            "https://arbitrum.gateway.tenderly.co",
            "https://arbitrum.meowrpc.com",
            "https://arb-pokt.nodies.app",
            "https://arbitrum-one.publicnode.com",
            "https://arbitrum-rpc.publicnode.com",
            "https://1rpc.io/arb",
        ],
        _ => &[],
    }
}

/// Optimism (10) and Polygon (137) defaults.
pub fn default_endpoints_more(chain_id: u64) -> &'static [&'static str] {
    match chain_id {
        10 => &[
            "https://optimism-rpc.publicnode.com",
            "https://mainnet.optimism.io",
            "https://optimism.drpc.org",
            "https://optimism.gateway.tenderly.co",
            "https://1rpc.io/op",
            "https://op-pokt.nodies.app",
        ],
        137 => &[
            "https://polygon-bor-rpc.publicnode.com",
            "https://polygon.drpc.org",
            "https://polygon.gateway.tenderly.co",
            "https://1rpc.io/matic",
        ],
        _ => &[],
    }
}

/// BNB Smart Chain (56) and Avalanche C-Chain (43114) defaults.
pub fn default_endpoints_l2(chain_id: u64) -> &'static [&'static str] {
    match chain_id {
        56 => &[
            "https://bsc-rpc.publicnode.com",
            "https://bsc-dataseed1.binance.org",
            "https://bsc-dataseed2.binance.org",
            "https://bsc-dataseed1.defibit.io",
            "https://bsc-dataseed1.ninicoin.io",
            "https://bsc.publicnode.com",
            "https://1rpc.io/bnb",
            "https://bsc-mainnet.public.blastapi.io",
            "https://bsc-dataseed2.defibit.io",
        ],
        43114 => &[
            "https://avalanche-c-chain-rpc.publicnode.com",
            "https://api.avax.network/ext/bc/C/rpc",
            "https://avalanche.drpc.org",
            "https://avalanche.gateway.tenderly.co",
            "https://1rpc.io/avax/c",
        ],
        _ => &[],
    }
}

/// Every default endpoint for a chain, across the split catalogues.
pub fn all_default_endpoints(chain_id: u64) -> Vec<String> {
    let mut out: Vec<String> = default_endpoints(chain_id)
        .iter()
        .map(|s| s.to_string())
        .collect();
    out.extend(
        default_endpoints_more(chain_id)
            .iter()
            .map(|s| s.to_string()),
    );
    out.extend(default_endpoints_l2(chain_id).iter().map(|s| s.to_string()));
    out.extend(
        default_endpoints_chains13(chain_id)
            .iter()
            .map(|s| s.to_string()),
    );
    out
}

/// Total number of built-in endpoints across every chain.
pub fn total_default_endpoints() -> usize {
    [1u64, 42161, 10, 137, 56, 43114, 8453, 42220, 100, 59144]
        .iter()
        .map(|c| all_default_endpoints(*c).len())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHAINS: [u64; 10] = [1, 42161, 10, 137, 56, 43114, 8453, 42220, 100, 59144];

    #[test]
    fn total_exceeds_60_endpoints() {
        let total = total_default_endpoints();
        assert!(total >= 60, "only {total} endpoints registered");
    }

    #[test]
    fn every_chain_has_at_least_4_endpoints() {
        for chain in CHAINS {
            let n = all_default_endpoints(chain).len();
            assert!(n >= 4, "chain {chain} only has {n} endpoints");
        }
    }

    #[test]
    fn all_endpoints_are_https() {
        for chain in CHAINS {
            for url in all_default_endpoints(chain) {
                assert!(url.starts_with("https://"), "not https: {url}");
            }
        }
    }

    /// Duplicates waste pool slots.
    #[test]
    fn no_duplicates_within_a_chain() {
        for chain in CHAINS {
            let mut seen = std::collections::HashSet::new();
            for url in all_default_endpoints(chain) {
                assert!(seen.insert(url.clone()), "chain {chain} repeats {url}");
            }
        }
    }

    #[test]
    fn unknown_chain_returns_empty() {
        assert!(all_default_endpoints(999).is_empty());
    }
}

/// Base (8453), Celo (42220), Gnosis (100) and Linea (59144) defaults.
pub fn default_endpoints_chains13(chain_id: u64) -> &'static [&'static str] {
    match chain_id {
        8453 => &[
            "https://mainnet.base.org",
            "https://base-rpc.publicnode.com",
            "https://base.drpc.org",
            "https://base.meowrpc.com",
            "https://1rpc.io/base",
            "https://base-mainnet.public.blastapi.io",
            "https://base.gateway.tenderly.co",
            "https://base-pokt.nodies.app",
        ],
        42220 => &[
            "https://forno.celo.org",
            "https://celo-rpc.publicnode.com",
            "https://celo.gateway.tenderly.co",
            "https://1rpc.io/celo",
        ],
        100 => &[
            "https://rpc.gnosischain.com",
            "https://gnosis-rpc.publicnode.com",
            "https://gnosis.drpc.org",
            "https://gnosis.gateway.tenderly.co",
            "https://1rpc.io/gnosis",
        ],
        59144 => &[
            "https://rpc.linea.build",
            "https://linea-rpc.publicnode.com",
            "https://linea.drpc.org",
            "https://linea.gateway.tenderly.co",
            "https://1rpc.io/linea",
        ],
        _ => &[],
    }
}

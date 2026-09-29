//! Curated public RPC endpoint catalogue.
//!
//! The scanner talks to free, unauthenticated endpoints. Two properties make
//! that workable, and both are enforced in `rpc_pool` rather than here:
//!
//!   * **Endpoints go stale.** Hosts get rate-limited, geo-blocked, or shut
//!     down. A long list is not a liability *because* every endpoint is
//!     health-tracked, 429-cooldowned and quarantined, with failover on
//!     transport errors.
//!   * **Region matters.** Entries are ordered EU-hosted-provider-first, which
//!     matters when `DEPLOYMENT_REGION=frankfurt`.
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
            "https://rpc.ankr.com/eth",
            "https://cloudflare-eth.com",
            "https://eth.merkle.io",
            "https://eth-mainnet.public.blastapi.io",
            "https://ethereum.publicnode.com",
            "https://1rpc.io/eth",
            "https://eth.rpc.blxrbdn.com",
            "https://eth.blockrazor.xyz",
            "https://eth-pokt.nodies.app",
            "https://eth.rpc.subquery.network/public",
            "https://api.securerpc.com/v1",
            "https://gateway.tenderly.co/public/mainnet",
            "https://rpc.mevblocker.io",
            "https://rpc.payload.de",
            "https://core.gashawk.io/rpc",
            "https://eth-mainnet.gateway.pokt.network/v1/lb",
            "https://rpc.builder0x69.io",
            "https://eth.rpc.gateway.fm",
            "https://eth.api.onfinality.io/public",
            "https://eth.gateway.tenderly.co",
            "https://ethereum.rpc.subquery.network/public",
            "https://eth.blockpi.network/v1/rpc/public",
            "https://eth-mainnet.rpcfast.com",
            "https://eth.meowrpc.com",
        ],
        42161 => &[
            "https://arbitrum-one-rpc.publicnode.com",
            "https://arb1.arbitrum.io/rpc",
            "https://arbitrum.drpc.org",
            "https://arbitrum-one.llamarpc.com",
            "https://arbitrum-one-rpc.gateway.pokt.network/v1/lb",
            "https://arbitrum-one.public.blastapi.io",
            "https://arbitrum.gateway.tenderly.co",
            "https://1rpc.io/arb",
            "https://arbitrum.blockrazor.xyz",
            "https://arbitrum-pokt.nodies.app",
            "https://arbitrum.rpc.subquery.network/public",
            "https://api.securerpc.com/v1",
            "https://arb1.rpc.blockpi.network/v1/rpc/public",
            "https://arbitrum.meowrpc.com",
            "https://rpc.arb1.arbitrum.gateway.fm",
            "https://arbitrum.api.onfinality.io/public",
            "https://arbitrum.rpc.gateway.fm",
            "https://arb-pokt.nodies.app",
            "https://core.gashawk.io/rpc/arb",
            "https://arb.gateway.tenderly.co",
            "https://arbitrum.rpcbuilder.io",
            "https://arb1.gateway.pokt.network",
            "https://arbitrum-rpc.publicnode.com",
            "https://arbitrum.io/rpc",
            "https://arbitrum-one.publicnode.com",
            "https://arbitrum-mainnet.public.blastapi.io",
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
            "https://optimism.llamarpc.com",
            "https://optimism-mainnet.gateway.pokt.network/v1/lb",
            "https://optimism.public.blastapi.io",
            "https://optimism.gateway.tenderly.co",
            "https://1rpc.io/op",
            "https://optimism.blockrazor.xyz",
            "https://optimism-pokt.nodies.app",
            "https://optimism.rpc.subquery.network/public",
            "https://api.securerpc.com/v1",
            "https://optimism.meowrpc.com",
            "https://optimism.api.onfinality.io/public",
            "https://optimism.rpc.gateway.fm",
            "https://core.gashawk.io/rpc/op",
            "https://optimism.blockpi.network/v1/rpc/public",
            "https://op-pokt.nodies.app",
            "https://optimism.gateway.pokt.network",
            "https://op.gateway.tenderly.co",
            "https://optimism.rpcbuilder.io",
            "https://opt-rpc.publicnode.com",
            "https://optimism-rpc.gateway.pokt.network/v1/lb",
            "https://rpc.optimism.gateway.fm",
            "https://optimism-rpc.gateway.fm",
            "https://op.gateway.pokt.network",
            "https://optimism-mainnet.publicnode.com",
        ],
        137 => &[
            "https://polygon-bor-rpc.publicnode.com",
            "https://polygon-rpc.com",
            "https://polygon.drpc.org",
            "https://polygon.llamarpc.com",
            "https://polygon-mainnet.public.blastapi.io",
            "https://polygon.gateway.tenderly.co",
            "https://1rpc.io/matic",
            "https://polygon.blockrazor.xyz",
            "https://polygon-pokt.nodies.app",
            "https://polygon.rpc.subquery.network/public",
            "https://api.securerpc.com/v1",
            "https://polygon.meowrpc.com",
            "https://polygon.api.onfinality.io/public",
            "https://polygon.rpc.gateway.fm",
            "https://core.gashawk.io/rpc/matic",
            "https://polygon.blockpi.network/v1/rpc/public",
            "https://matic-pokt.nodies.app",
            "https://polygon.gateway.pokt.network",
            "https://polygon-bor-rpc.gateway.pokt.network/v1/lb",
            "https://polygon.gateway.fm",
            "https://rpc-mainnet.matic.quiknode.pro",
            "https://matic.rpc.subquery.network/public",
            "https://rpc.matic.gateway.fm",
            "https://polygon-rpc.gateway.pokt.network/v1/lb",
            "https://maticsdk.com",
        ],
        _ => &[],
    }
}

/// BNB Smart Chain (56) and Avalanche C-Chain (43114) defaults.
pub fn default_endpoints_l2(chain_id: u64) -> &'static [&'static str] {
    match chain_id {
        56 => &[
            "https://bsc-rpc.publicnode.com",
            "https://bsc-dataseed.binance.org",
            "https://bsc-dataseed1.defibit.io",
            "https://bsc-dataseed1.ninicoin.io",
            "https://bsc.drpc.org",
            "https://bsc.llamarpc.com",
            "https://bsc.meowrpc.com",
            "https://bsc.publicnode.com",
            "https://bsc-dataseed2.binance.org",
            "https://bsc-dataseed3.binance.org",
            "https://bsc-dataseed4.binance.org",
            "https://1rpc.io/bnb",
            "https://bsc.blockrazor.xyz",
            "https://bsc-pokt.nodies.app",
            "https://bsc.rpc.subquery.network/public",
            "https://api.securerpc.com/v1",
            "https://bsc.api.onfinality.io/public",
            "https://bsc.rpc.gateway.fm",
            "https://core.gashawk.io/rpc/bsc",
            "https://bsc.blockpi.network/v1/rpc/public",
            "https://bsc-dataseed2.defibit.io",
            "https://bsc-dataseed3.defibit.io",
            "https://bsc-dataseed4.defibit.io",
            "https://bnb.rpc.subquery.network/public",
            "https://bsc.gateway.tenderly.co",
            "https://bsc.gateway.pokt.network",
            "https://bsc-mainnet.public.blastapi.io",
        ],
        43114 => &[
            "https://avalanche-c-chain-rpc.publicnode.com",
            "https://api.avax.network/ext/bc/C/rpc",
            "https://avalanche.drpc.org",
            "https://avalanche.llamarpc.com",
            "https://avalanche-c-chain.blastapi.io",
            "https://avalanche.gateway.tenderly.co",
            "https://1rpc.io/avax",
            "https://avalanche.blockrazor.xyz",
            "https://avalanche-pokt.nodies.app",
            "https://avalanche.rpc.subquery.network/public",
            "https://api.securerpc.com/v1",
            "https://avalanche.meowrpc.com",
            "https://avalanche.api.onfinality.io/public",
            "https://avalanche.rpc.gateway.fm",
            "https://core.gashawk.io/rpc/avax",
            "https://avalanche.blockpi.network/v1/rpc/public",
            "https://avalanche-c-chain-rpc.gateway.pokt.network/v1/lb",
            "https://avalanche.gateway.pokt.network",
            "https://avax-mainnet.public.blastapi.io",
            "https://avalanche-c-chain.gateway.pokt.network",
            "https://avalanche-c-chain.drpc.org",
            "https://avalanche-c-chain.llamarpc.com",
            "https://avax-pokt.nodies.app",
            "https://avalanche-c-chain.meowrpc.com",
            "https://avax.blockrazor.xyz",
            "https://rpc.avax.gateway.fm",
            "https://avalanche-c-chain.rpc.gateway.fm",
            "https://avax.gateway.tenderly.co",
            "https://avalanche-c-chain.publicnode.com",
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
    fn total_exceeds_150_endpoints() {
        let total = total_default_endpoints();
        assert!(total >= 150, "only {total} endpoints registered");
    }

    #[test]
    fn every_chain_has_at_least_18_endpoints() {
        for chain in CHAINS {
            let n = all_default_endpoints(chain).len();
            assert!(n >= 18, "chain {chain} only has {n} endpoints");
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

    /// Duplicates waste pool slots. The pool dedupes on register, but shipping
    /// obvious repeats hides a real mistake in the list.
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
            "https://base.llamarpc.com",
            "https://base.meowrpc.com",
            "https://1rpc.io/base",
            "https://base-mainnet.public.blastapi.io",
            "https://base.gateway.tenderly.co",
            "https://base.blockrazor.xyz",
            "https://base-pokt.nodies.app",
            "https://base.rpc.subquery.network/public",
            "https://api.securerpc.com/v1",
            "https://base.api.onfinality.io/public",
            "https://base.rpc.gateway.fm",
            "https://core.gashawk.io/rpc/base",
            "https://base.blockpi.network/v1/rpc/public",
            "https://base-mainnet.gateway.pokt.network/v1/lb",
            "https://base.gateway.pokt.network",
            "https://developer-access-mainnet.base.org",
            "https://base-mainnet.rpcfast.com",
            "https://base.therpc.io",
            "https://base-mainnet.publicnode.com",
            "https://base-rpc.gateway.pokt.network/v1/lb",
            "https://base.voyager.online",
            "https://base.llamarpc.com/",
            "https://base.blastapi.io",
            "https://base-mainnet.rpc.gateway.fm",
        ],
        42220 => &[
            "https://forno.celo.org",
            "https://celo-rpc.publicnode.com",
            "https://celo.drpc.org",
            "https://celo.llamarpc.com",
            "https://1rpc.io/celo",
            "https://celo-mainnet.public.blastapi.io",
            "https://celo.gateway.tenderly.co",
            "https://celo-pokt.nodies.app",
            "https://api.securerpc.com/v1",
            "https://celo.api.onfinality.io/public",
            "https://celo.rpc.gateway.fm",
            "https://celo.blockpi.network/v1/rpc/public",
            "https://celo-mainnet.gateway.pokt.network/v1/lb",
            "https://celo.gateway.pokt.network",
            "https://forno-alfajores.celo-testnet.org",
            "https://celo-mainnet.publicnode.com",
            "https://celo.meowrpc.com",
            "https://rpc.celo.gateway.fm",
            "https://celo.blockrazor.xyz",
            "https://celo.drpc.org/",
        ],
        100 => &[
            "https://rpc.gnosischain.com",
            "https://gnosis-rpc.publicnode.com",
            "https://gnosis.drpc.org",
            "https://gnosis.llamarpc.com",
            "https://1rpc.io/gnosis",
            "https://rpc.gnosis.gateway.fm",
            "https://gnosis.gateway.tenderly.co",
            "https://gnosis-pokt.nodies.app",
            "https://api.securerpc.com/v1",
            "https://gnosis.api.onfinality.io/public",
            "https://gnosis.blockpi.network/v1/rpc/public",
            "https://gnosis-mainnet.gateway.pokt.network/v1/lb",
            "https://gnosis.gateway.pokt.network",
            "https://gnosis-rpc.gateway.pokt.network/v1/lb",
            "https://gnosis.meowrpc.com",
            "https://gnosis.blockrazor.xyz",
            "https://gnosischain-rpc.publicnode.com",
            "https://rpc.ankr.com/gnosis",
            "https://gnosis.public.blastapi.io",
            "https://gnosischain.publicnode.com",
        ],
        59144 => &[
            "https://rpc.linea.build",
            "https://linea-rpc.publicnode.com",
            "https://linea.drpc.org",
            "https://linea.llamarpc.com",
            "https://1rpc.io/linea",
            "https://linea.gateway.tenderly.co",
            "https://linea-pokt.nodies.app",
            "https://api.securerpc.com/v1",
            "https://linea.api.onfinality.io/public",
            "https://linea.blockpi.network/v1/rpc/public",
            "https://linea-mainnet.gateway.pokt.network/v1/lb",
            "https://linea.gateway.pokt.network",
            "https://linea.meowrpc.com",
            "https://linea.blockrazor.xyz",
            "https://linea-mainnet.public.blastapi.io",
            "https://linea-mainnet.publicnode.com",
            "https://rpc.linea.gateway.fm",
            "https://linea-rpc.gateway.pokt.network/v1/lb",
        ],
        _ => &[],
    }
}

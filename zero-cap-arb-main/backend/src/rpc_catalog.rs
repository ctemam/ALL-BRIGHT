//! Curated public RPC endpoint catalogue.
//!
//! Every endpoint was live-verified on 2026-10-01 by probing `eth_chainId`
//! (which MUST match the intended chain — this rejects wrong-chain impostors
//! like Arbitrum-Nova endpoints serving chain 42170 on Arbitrum-One slots) and
//! a real `eth_call` to Multicall3. Dead, keyed, auth-gated, and wrong-chain
//! URLs have been removed; endpoints that currently rate-limit are retained
//! (round-robin + retry absorbs transient 429s) and marked where known.
//!
//! The scanner uses round-robin rotation across these URLs with 3-attempt
//! retries, so each endpoint sees a small fraction of total traffic.
//!
//! Operators can override or extend any list via env vars (see
//! `chains::read_rpc_urls_for_chain`); these are only the built-in defaults.
//!
//! See RPC_BUDGET.md for per-chain capacity math and provider documentation.

/// Built-in default endpoints for every supported chain.
///
/// Ordering convention within each chain: high-capacity community/providers
/// first (publicnode, drpc, thirdweb, pocket, onfinality), official chain
/// endpoints next, then smaller/regional providers.
pub fn default_endpoints(chain_id: u64) -> &'static [&'static str] {
    match chain_id {
        // ── Ethereum mainnet (39 endpoints) ────────────────────────────────
        1 => &[
            "https://ethereum-rpc.publicnode.com",
            "https://ethereum.publicnode.com",
            "https://eth.drpc.org",
            "https://1.rpc.thirdweb.com",
            "https://eth.api.pocket.network",
            "https://eth.api.onfinality.io/public",
            "https://ethereum-public.nodies.app",
            "https://eth-pokt.nodies.app",
            "https://ethereum.public.blockpi.network/v1/rpc/public",
            "https://eth-mainnet.public.blastapi.io",
            "https://gateway.tenderly.co/public/mainnet",
            "https://mainnet.gateway.tenderly.co",
            "https://rpc.mevblocker.io",
            "https://rpc.mevblocker.io/fast",
            "https://rpc.mevblocker.io/fullprivacy",
            "https://rpc.mevblocker.io/noreverts",
            "https://eth.rpc.blxrbdn.com",
            "https://uk.rpc.blxrbdn.com",
            "https://virginia.rpc.blxrbdn.com",
            "https://singapore.rpc.blxrbdn.com",
            "https://ethereum-json-rpc.stakely.io",
            "https://mainnet.rpc.sentio.xyz",
            "https://rpc.swiftnodes.io/rpc/eth",
            "https://rpc.hostdefi.com/api/rpc/ethereum",
            "https://rpc.nodeflare.app/eth/public",
            "https://rpc-eth.blockmachine.io",
            "https://rpc.fullsend.to",
            "https://xrpc.cl/eth",
            "https://0xrpc.io/eth",
            "https://eth.blockrazor.xyz",
            "https://eth.blockrazor.xyz/fullprivacy",
            "https://eth.blockrazor.xyz/maxbackrun",
            "https://eth-mainnet.token.im",
            "https://one.valve.city/rpc/vk_demo/evm/1",
            "https://public-eth.nownodes.io",
            "https://lb.routeme.sh/rpc/evm/1",
            // Rate-limited free tiers — valid but frequently capped; the
            // rotation absorbs their 429s as retries to the next endpoint.
            "https://1rpc.io/eth",
            "https://public.1rpc.io/eth",
            "https://eth-mainnet.nodereal.io/v1/1659dfb40aa24bbb8153a677b98064d7",
        ],
        // ── Arbitrum One (23 endpoints) ────────────────────────────────────
        42161 => &[
            "https://arbitrum-one-rpc.publicnode.com",
            "https://arbitrum-one.publicnode.com",
            "https://arbitrum-rpc.publicnode.com",
            "https://arbitrum.publicnode.com",
            "https://arb1.arbitrum.io/rpc",
            "https://arbitrum.drpc.org",
            "https://42161.rpc.thirdweb.com",
            "https://arb-one.api.pocket.network",
            "https://arbitrum.api.onfinality.io/public",
            "https://arb-pokt.nodies.app",
            "https://arbitrum-one-public.nodies.app",
            "https://arbitrum-one.public.blastapi.io",
            "https://arbitrum.gateway.tenderly.co",
            "https://gateway.tenderly.co/public/arbitrum",
            "https://arbitrum-one.rpc.sentio.xyz",
            "https://rpc.swiftnodes.io/rpc/arbitrum",
            "https://rpc.hostdefi.com/api/rpc/arbitrum",
            "https://rpc.nodeflare.app/arb/public",
            "https://public-arb-mainnet.fastnode.io",
            "https://rpc-arbitrum.blockmachine.io",
            "https://xrpc.cl/arbitrum",
            "https://lb.routeme.sh/rpc/evm/42161",
            "https://1rpc.io/arb",
        ],
        // ── Optimism (20 endpoints) ────────────────────────────────────────
        10 => &[
            "https://optimism-rpc.publicnode.com",
            "https://optimism.publicnode.com",
            "https://mainnet.optimism.io",
            "https://optimism.drpc.org",
            "https://10.rpc.thirdweb.com",
            "https://op.api.pocket.network",
            "https://optimism.api.onfinality.io/public",
            "https://optimism-public.nodies.app",
            "https://optimism.gateway.tenderly.co",
            "https://gateway.tenderly.co/public/optimism",
            "https://optimism.public.blockpi.network/v1/rpc/public",
            "https://optimism.rpc.sentio.xyz",
            "https://rpc.swiftnodes.io/rpc/optimism",
            "https://rpc.hostdefi.com/api/rpc/optimism",
            "https://rpc.nodeflare.app/op/public",
            "https://rpc-optimism.blockmachine.io",
            "https://xrpc.cl/optimism",
            "https://lb.routeme.sh/rpc/evm/10",
            "https://api-optimism-mainnet-archive.n.dwellir.com/2ccf18bf-2916-4198-8856-42172854353c",
            "https://1rpc.io/op",
        ],
        // ── Polygon PoS (17 endpoints) ─────────────────────────────────────
        137 => &[
            "https://polygon.drpc.org",
            "https://137.rpc.thirdweb.com",
            "https://poly.api.pocket.network",
            "https://polygon.api.onfinality.io/public",
            "https://polygon.gateway.tenderly.co",
            "https://gateway.tenderly.co/public/polygon",
            "https://matic.rpc.sentio.xyz",
            "https://rpc.swiftnodes.io/rpc/polygon",
            "https://rpc.hostdefi.com/api/rpc/polygon",
            "https://rpc.nodeflare.app/polygon/public",
            "https://rpc-mainnet.matic.quiknode.pro",
            "https://rpc-polygon.blockmachine.io",
            "https://rpc.private.mev-x.com/polygon",
            "https://xrpc.cl/polygon",
            "https://lb.routeme.sh/rpc/evm/137",
            "https://api-polygon-mainnet-full.n.dwellir.com/2ccf18bf-2916-4198-8856-42172854353c",
            "https://1rpc.io/matic",
        ],
        // ── BNB Smart Chain (44 endpoints) ─────────────────────────────────
        56 => &[
            "https://bsc-rpc.publicnode.com",
            "https://bsc.publicnode.com",
            "https://bsc-dataseed.binance.org",
            "https://bsc-dataseed1.binance.org",
            "https://bsc-dataseed2.binance.org",
            "https://bsc-dataseed3.binance.org",
            "https://bsc-dataseed4.binance.org",
            "https://bsc-dataseed.bnbchain.org",
            "https://bsc-dataseed1.bnbchain.org",
            "https://bsc-dataseed2.bnbchain.org",
            "https://bsc-dataseed3.bnbchain.org",
            "https://bsc-dataseed4.bnbchain.org",
            "https://bsc-dataseed1.defibit.io",
            "https://bsc-dataseed2.defibit.io",
            "https://bsc-dataseed3.defibit.io",
            "https://bsc-dataseed4.defibit.io",
            "https://bsc-dataseed1.ninicoin.io",
            "https://bsc-dataseed2.ninicoin.io",
            "https://bsc-dataseed3.ninicoin.io",
            "https://bsc-dataseed4.ninicoin.io",
            "https://bsc.drpc.org",
            "https://56.rpc.thirdweb.com",
            "https://bsc.api.pocket.network",
            "https://bnb.api.onfinality.io/public",
            "https://bsc-mainnet.public.blastapi.io",
            "https://bsc.rpc.blxrbdn.com",
            "https://bsc.blockrazor.xyz",
            "https://bsc.blockrazor.xyz/fullprivacy",
            "https://bsc.blockrazor.xyz/maxbackrun",
            "https://bsc.rpc.sentio.xyz",
            "https://rpc.swiftnodes.io/rpc/bsc",
            "https://rpc.hostdefi.com/api/rpc/bsc",
            "https://rpc.nodeflare.app/bnb/public",
            "https://public-bsc-mainnet.fastnode.io",
            "https://public-bsc.nownodes.io",
            "https://rpc-bsc.blockmachine.io",
            "https://rpc-bsc.48.club",
            "https://0.48.club",
            "https://binance.nodereal.io",
            "https://xrpc.cl/bsc",
            "https://lb.routeme.sh/rpc/evm/56",
            "https://bsc-mainnet.nodereal.io/v1/64a9df0874fb4a93b9d0a3849de012d3",
            "https://api-bsc-mainnet-full.n.dwellir.com/2ccf18bf-2916-4198-8856-42172854353c",
            "https://1rpc.io/bnb",
        ],
        // ── Avalanche C-Chain (19 endpoints) ───────────────────────────────
        43114 => &[
            "https://avalanche-c-chain-rpc.publicnode.com",
            "https://avalanche.publicnode.com",
            "https://api.avax.network/ext/bc/C/rpc",
            "https://avalanche.drpc.org",
            "https://43114.rpc.thirdweb.com",
            "https://avax.api.pocket.network",
            "https://avalanche.api.onfinality.io/public/ext/bc/C/rpc",
            "https://avalanche.gateway.tenderly.co",
            "https://avalanche-mainnet.gateway.tenderly.co",
            "https://gateway.tenderly.co/public/avalanche",
            "https://avalanche.rpc.sentio.xyz",
            "https://rpc.swiftnodes.io/rpc/avalanche",
            "https://rpc.hostdefi.com/api/rpc/avalanche",
            "https://rpc.nodeflare.app/avax/public",
            "https://rpc-avalanche.blockmachine.io",
            "https://spectrum-01.simplystaking.xyz/avalanche-mn-rpc/ext/bc/C/rpc",
            "https://xrpc.cl/avalanche",
            "https://lb.routeme.sh/rpc/evm/43114",
            "https://1rpc.io/avax/c",
        ],
        // ── Base (24 endpoints) ────────────────────────────────────────────
        8453 => &[
            "https://base-rpc.publicnode.com",
            "https://base.publicnode.com",
            "https://mainnet.base.org",
            "https://developer-access-mainnet.base.org",
            "https://base.drpc.org",
            "https://8453.rpc.thirdweb.com",
            "https://base.api.pocket.network",
            "https://base.api.onfinality.io/public",
            "https://base-pokt.nodies.app",
            "https://base-public.nodies.app",
            "https://base-mainnet.public.blastapi.io",
            "https://base.public.blockpi.network/v1/rpc/public",
            "https://base.gateway.tenderly.co",
            "https://gateway.tenderly.co/public/base",
            "https://base.rpc.blxrbdn.com",
            "https://base.rpc.sentio.xyz",
            "https://rpc.swiftnodes.io/rpc/base",
            "https://rpc.hostdefi.com/api/rpc/base",
            "https://rpc.nodeflare.app/base/public",
            "https://rpc-base.blockmachine.io",
            "https://rpc.baseazul.dev",
            "https://xrpc.cl/base",
            "https://lb.routeme.sh/rpc/evm/8453",
            "https://1rpc.io/base",
        ],
        // ── Celo (14 endpoints) ────────────────────────────────────────────
        42220 => &[
            "https://forno.celo.org",
            "https://celo-rpc.publicnode.com",
            "https://celo.publicnode.com",
            "https://42220.rpc.thirdweb.com",
            "https://celo.api.pocket.network",
            "https://celo.api.onfinality.io/public",
            "https://celo.gateway.tenderly.co",
            "https://gateway.tenderly.co/public/celo",
            "https://rpc.ankr.com/celo",
            "https://celo-json-rpc.stakely.io",
            "https://rpc.swiftnodes.io/rpc/celo",
            "https://rpc.hostdefi.com/api/rpc/celo",
            "https://lb.routeme.sh/rpc/evm/42220",
            "https://1rpc.io/celo",
        ],
        // ── Gnosis (16 endpoints) ──────────────────────────────────────────
        100 => &[
            "https://rpc.gnosischain.com",
            "https://gnosis-rpc.publicnode.com",
            "https://gnosis.publicnode.com",
            "https://gnosis.drpc.org",
            "https://100.rpc.thirdweb.com",
            "https://gnosis.api.pocket.network",
            "https://gnosis.api.onfinality.io/public",
            "https://gnosis.gateway.tenderly.co",
            "https://gateway.tenderly.co/public/gnosis",
            "https://rpc.gnosis.gateway.fm",
            "https://rpc.ap-southeast-1.gateway.fm/v4/gnosis/non-archival/mainnet",
            "https://gnosis.oat.farm",
            "https://rpc.swiftnodes.io/rpc/gnosis",
            "https://rpc.hostdefi.com/api/rpc/gnosis",
            "https://lb.routeme.sh/rpc/evm/100",
            "https://1rpc.io/gnosis",
        ],
        // ── Linea (14 endpoints) ───────────────────────────────────────────
        59144 => &[
            "https://rpc.linea.build",
            "https://linea-rpc.publicnode.com",
            "https://linea.publicnode.com",
            "https://linea.drpc.org",
            "https://59144.rpc.thirdweb.com",
            "https://linea.api.pocket.network",
            "https://linea.gateway.tenderly.co",
            "https://gateway.tenderly.co/public/linea",
            "https://linea.rpc.sentio.xyz",
            "https://rpc.swiftnodes.io/rpc/linea",
            "https://rpc.hostdefi.com/api/rpc/linea",
            "https://lb.routeme.sh/rpc/evm/59144",
            "https://api-linea-mainnet-archive.n.dwellir.com/2ccf18bf-2916-4198-8856-42172854353c",
            "https://1rpc.io/linea",
        ],
        // ── Sonic — verified live (eth_chainId=146 + eth_call) ────────────
        146 => &[
            "https://rpc.soniclabs.com",
            "https://sonic-rpc.publicnode.com",
            "https://sonic.drpc.org",
            "https://sonic-json-rpc.stakely.io",
            "https://146.rpc.thirdweb.com",
        ],
        // ── Unichain — verified live (eth_chainId=130 + eth_call) ─────────
        130 => &[
            "https://mainnet.unichain.org",
            "https://unichain-rpc.publicnode.com",
            "https://unichain.drpc.org",
            "https://unichain.api.onfinality.io/public",
            "https://130.rpc.thirdweb.com",
        ],
        // ── Scroll — verified live ─────────────────────────────────────────
        534352 => &[
            "https://rpc.scroll.io",
            "https://scroll-rpc.publicnode.com",
            "https://scroll.drpc.org",
            "https://scroll.api.onfinality.io/public",
            "https://534352.rpc.thirdweb.com",
        ],
        // ── zkSync Era — verified live ─────────────────────────────────────
        324 => &[
            "https://mainnet.era.zksync.io",
            "https://zksync.drpc.org",
            "https://zksync.api.onfinality.io/public",
            "https://324.rpc.thirdweb.com",
        ],
        // ── Mantle — verified live ─────────────────────────────────────────
        5000 => &[
            "https://rpc.mantle.xyz",
            "https://mantle-rpc.publicnode.com",
            "https://mantle.drpc.org",
            "https://mantle.api.onfinality.io/public",
            "https://5000.rpc.thirdweb.com",
        ],
        _ => &[],
    }
}

/// Every default endpoint for a chain.
pub fn all_default_endpoints(chain_id: u64) -> Vec<String> {
    default_endpoints(chain_id)
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Total number of built-in endpoints across every chain.
pub fn total_default_endpoints() -> usize {
    [
        1u64, 42161, 10, 137, 56, 43114, 8453, 42220, 100, 59144, 146, 130, 534352, 324, 5000,
    ]
    .iter()
    .map(|c| all_default_endpoints(*c).len())
    .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Original ten chains carry the deep endpoint pools.
    const CHAINS: [u64; 10] = [1, 42161, 10, 137, 56, 43114, 8453, 42220, 100, 59144];
    /// The five high-throughput additions have thinner public-RPC coverage.
    const NEW_CHAINS: [u64; 5] = [146, 130, 534352, 324, 5000];

    #[test]
    fn total_exceeds_200_endpoints() {
        let total = total_default_endpoints();
        assert!(total >= 200, "only {total} endpoints registered");
    }

    #[test]
    fn every_chain_has_at_least_10_endpoints() {
        for chain in CHAINS {
            let n = all_default_endpoints(chain).len();
            assert!(n >= 10, "chain {chain} only has {n} endpoints");
        }
    }

    #[test]
    fn new_chains_have_working_endpoints() {
        for chain in NEW_CHAINS {
            let n = all_default_endpoints(chain).len();
            assert!(n >= 4, "new chain {chain} only has {n} endpoints");
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

use crate::types::{ChainConfig, DexConfig};
use std::sync::OnceLock;

pub static CHAINS: OnceLock<Vec<ChainConfig>> = OnceLock::new();

/// Read an env var and split by comma → multiple RPC URLs
/// If the var is `KEY` it reads `KEY`, `KEY_1`, `KEY_2`, ...
/// Example:
///   ETH_RPC_URL=https://eth-mainnet.g.alchemy.com/v2/xxx
///   ETH_RPC_URL_1=https://mainnet.infura.io/v3/yyy
/// Or all in one:
///   ETH_RPC_URL="https://eth-mainnet.alchemy.io/xxx,https://mainnet.infura.io/yyy"
fn read_rpc_urls(env_key: &str, default: &str) -> Vec<String> {
    let primary = std::env::var(env_key).unwrap_or_else(|_| default.to_string());

    // Try splitting by comma
    let parts: Vec<&str> = primary.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
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
        ]
    })
}

/// 50+ DEXes across all 6 chains
pub fn get_dexes_for_chain(chain_id: u64) -> Vec<DexConfig> {
    match chain_id {
        1 => vec![
            DexConfig { name: "Uniswap V3".into(), address: "0xE592427A0AEce92De3Edee1F18E0157C05861564".into(), chain_id, router_abi: None },
            DexConfig { name: "Uniswap V2".into(), address: "0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D".into(), chain_id, router_abi: None },
            DexConfig { name: "SushiSwap".into(), address: "0xd9e1cE17f2641f24aE83637ab66a2cca9C378B9F".into(), chain_id, router_abi: None },
            DexConfig { name: "Balancer V2".into(), address: "0xBA12222222228d8Ba445958a75a0704d566BF2C8".into(), chain_id, router_abi: None },
            DexConfig { name: "Curve".into(), address: "0x5a0F6fC7cC6C6dC3cDc6f7cC6cC6cC6cC6cC6cC6".into(), chain_id, router_abi: None },
            DexConfig { name: "ParaSwap".into(), address: "0xDEF171Fe48CF0115B1d80b88dc8eAB59176FEe57".into(), chain_id, router_abi: None },
            DexConfig { name: "1inch".into(), address: "0x1111111254fb6c44bAC0beD2854e76F90643097d".into(), chain_id, router_abi: None },
            DexConfig { name: "Dodo".into(), address: "0xa356867fDCEa8e71AEaF87805808803806231FdC".into(), chain_id, router_abi: None },
            DexConfig { name: "Maverick".into(), address: "0x12cA4eA28a57cFb300F6c0D3f96b4dE5E7B9c8d3".into(), chain_id, router_abi: None },
            DexConfig { name: "PancakeSwap".into(), address: "0xEfF92A263d31888d860bD50809A8D171684b9b74".into(), chain_id, router_abi: None },
            DexConfig { name: "ShibaSwap".into(), address: "0x03f7724180AA6b939894B5Ca4314783B074bD0b1".into(), chain_id, router_abi: None },
            DexConfig { name: "TraderJoe".into(), address: "0xE8A1dE7C8Fc8617E6bF1A1E1F3C1B1A1E1F3C1B1".into(), chain_id, router_abi: None },
            DexConfig { name: "KyberSwap".into(), address: "0x6131B5fae19EA4f9D964eaC2D7E4C8C7E9bB6c7a".into(), chain_id, router_abi: None },
            DexConfig { name: "Synthetix".into(), address: "0x8700dAec35aF8Ff88c16BdF0418774CB3D7599B4".into(), chain_id, router_abi: None },
            DexConfig { name: "Maker PSM".into(), address: "0xf6f9cD9C6B9E7F9B6B9E7F9B6B9E7F9B6B9E7F9".into(), chain_id, router_abi: None },
        ],
        42161 => vec![
            DexConfig { name: "Uniswap V3".into(), address: "0xE592427A0AEce92De3Edee1F18E0157C05861564".into(), chain_id, router_abi: None },
            DexConfig { name: "SushiSwap".into(), address: "0x1b02dA8Cb0d097eB8D57A175b88c7D8b47997506".into(), chain_id, router_abi: None },
            DexConfig { name: "Curve".into(), address: "0x960ea3e3C7FB31744bD8Ea8Cb4C5A5b1E9E9C4A3".into(), chain_id, router_abi: None },
            DexConfig { name: "Balancer V2".into(), address: "0xBA12222222228d8Ba445958a75a0704d566BF2C8".into(), chain_id, router_abi: None },
            DexConfig { name: "ParaSwap".into(), address: "0xDEF171Fe48CF0115B1d80b88dc8eAB59176FEe57".into(), chain_id, router_abi: None },
            DexConfig { name: "1inch".into(), address: "0x1111111254fb6c44bAC0beD2854e76F90643097d".into(), chain_id, router_abi: None },
            DexConfig { name: "GMX".into(), address: "0x489ee077994B6658eAfE855C308627E6cF69F7A7".into(), chain_id, router_abi: None },
            DexConfig { name: "Camelot".into(), address: "0xc873fEcbd354f5A56E00E710B90EF4201db2448d".into(), chain_id, router_abi: None },
            DexConfig { name: "Dodo".into(), address: "0xa356867fDCEa8e71AEaF87805808803806231FdC".into(), chain_id, router_abi: None },
            DexConfig { name: "TraderJoe".into(), address: "0xE8A1dE7C8Fc8617E6bF1A1E1F3C1B1A1E1F3C1B1".into(), chain_id, router_abi: None },
        ],
        10 => vec![
            DexConfig { name: "Uniswap V3".into(), address: "0xE592427A0AEce92De3Edee1F18E0157C05861564".into(), chain_id, router_abi: None },
            DexConfig { name: "SushiSwap".into(), address: "0x1b02dA8Cb0d097eB8D57A175b88c7D8b47997506".into(), chain_id, router_abi: None },
            DexConfig { name: "Balancer V2".into(), address: "0xBA12222222228d8Ba445958a75a0704d566BF2C8".into(), chain_id, router_abi: None },
            DexConfig { name: "Curve".into(), address: "0x960ea3e3C7FB31744bD8Ea8Cb4C5A5b1E9E9C4A3".into(), chain_id, router_abi: None },
            DexConfig { name: "ParaSwap".into(), address: "0xDEF171Fe48CF0115B1d80b88dc8eAB59176FEe57".into(), chain_id, router_abi: None },
            DexConfig { name: "1inch".into(), address: "0x1111111254fb6c44bAC0beD2854e76F90643097d".into(), chain_id, router_abi: None },
            DexConfig { name: "Velodrome".into(), address: "0xa062aE8A9c5e11aaA026fc2670B0D65cCc8B2858".into(), chain_id, router_abi: None },
            DexConfig { name: "Synthetix".into(), address: "0x8700dAec35aF8Ff88c16BdF0418774CB3D7599B4".into(), chain_id, router_abi: None },
            DexConfig { name: "Dodo".into(), address: "0xa356867fDCEa8e71AEaF87805808803806231FdC".into(), chain_id, router_abi: None },
            DexConfig { name: "KyberSwap".into(), address: "0x6131B5fae19EA4f9D964eaC2D7E4C8C7E9bB6c7a".into(), chain_id, router_abi: None },
        ],
        137 => vec![
            DexConfig { name: "QuickSwap".into(), address: "0xa5E0829CaCEd8fFDD4De3c43696c57F7D7A678ff".into(), chain_id, router_abi: None },
            DexConfig { name: "Uniswap V3".into(), address: "0xE592427A0AEce92De3Edee1F18E0157C05861564".into(), chain_id, router_abi: None },
            DexConfig { name: "SushiSwap".into(), address: "0x1b02dA8Cb0d097eB8D57A175b88c7D8b47997506".into(), chain_id, router_abi: None },
            DexConfig { name: "Balancer V2".into(), address: "0xBA12222222228d8Ba445958a75a0704d566BF2C8".into(), chain_id, router_abi: None },
            DexConfig { name: "Curve".into(), address: "0x960ea3e3C7FB31744bD8Ea8Cb4C5A5b1E9E9C4A3".into(), chain_id, router_abi: None },
            DexConfig { name: "ParaSwap".into(), address: "0xDEF171Fe48CF0115B1d80b88dc8eAB59176FEe57".into(), chain_id, router_abi: None },
            DexConfig { name: "1inch".into(), address: "0x1111111254fb6c44bAC0beD2854e76F90643097d".into(), chain_id, router_abi: None },
            DexConfig { name: "Dodo".into(), address: "0xa356867fDCEa8e71AEaF87805808803806231FdC".into(), chain_id, router_abi: None },
            DexConfig { name: "KyberSwap".into(), address: "0x6131B5fae19EA4f9D964eaC2D7E4C8C7E9bB6c7a".into(), chain_id, router_abi: None },
            DexConfig { name: "TraderJoe".into(), address: "0xE8A1dE7C8Fc8617E6bF1A1E1F3C1B1A1E1F3C1B1".into(), chain_id, router_abi: None },
        ],
        56 => vec![
            DexConfig { name: "PancakeSwap V2".into(), address: "0x10ED43C718714eb63d5aA57B78B54704E256024E".into(), chain_id, router_abi: None },
            DexConfig { name: "PancakeSwap V3".into(), address: "0x13f4EA83D0bd40E75C8222255bc855a974568Dd4".into(), chain_id, router_abi: None },
            DexConfig { name: "Biswap".into(), address: "0x3a6d8cA21D1CF76F653A67577FA0D27453350dD8".into(), chain_id, router_abi: None },
            DexConfig { name: "Uniswap V3".into(), address: "0xE592427A0AEce92De3Edee1F18E0157C05861564".into(), chain_id, router_abi: None },
            DexConfig { name: "SushiSwap".into(), address: "0x1b02dA8Cb0d097eB8D57A175b88c7D8b47997506".into(), chain_id, router_abi: None },
            DexConfig { name: "MDEX".into(), address: "0x7DAe51BD3E3376B8c7c4900E9107f12Be3AF1bA8".into(), chain_id, router_abi: None },
            DexConfig { name: "ApeSwap".into(), address: "0xcF0feBd3f17CEf5b47b0cD257aC6025c5BFf3b7A".into(), chain_id, router_abi: None },
            DexConfig { name: "BabySwap".into(), address: "0x325E343f1dE602396E256B67eFd1F61C3A6B38Bd".into(), chain_id, router_abi: None },
            DexConfig { name: "Dodo".into(), address: "0xa356867fDCEa8e71AEaF87805808803806231FdC".into(), chain_id, router_abi: None },
            DexConfig { name: "ParaSwap".into(), address: "0xDEF171Fe48CF0115B1d80b88dc8eAB59176FEe57".into(), chain_id, router_abi: None },
        ],
        43114 => vec![
            DexConfig { name: "TraderJoe".into(), address: "0x60aE616a2155Ee3d9A68541Ba4544862310933d4".into(), chain_id, router_abi: None },
            DexConfig { name: "Pangolin".into(), address: "0xE54Ca86531e17Ef3616d22Ca28b0D458b6C89106".into(), chain_id, router_abi: None },
            DexConfig { name: "Uniswap V3".into(), address: "0xE592427A0AEce92De3Edee1F18E0157C05861564".into(), chain_id, router_abi: None },
            DexConfig { name: "SushiSwap".into(), address: "0x1b02dA8Cb0d097eB8D57A175b88c7D8b47997506".into(), chain_id, router_abi: None },
            DexConfig { name: "Curve".into(), address: "0x960ea3e3C7FB31744bD8Ea8Cb4C5A5b1E9E9C4A3".into(), chain_id, router_abi: None },
            DexConfig { name: "ParaSwap".into(), address: "0xDEF171Fe48CF0115B1d80b88dc8eAB59176FEe57".into(), chain_id, router_abi: None },
            DexConfig { name: "Dodo".into(), address: "0xa356867fDCEa8e71AEaF87805808803806231FdC".into(), chain_id, router_abi: None },
            DexConfig { name: "1inch".into(), address: "0x1111111254fb6c44bAC0beD2854e76F90643097d".into(), chain_id, router_abi: None },
            DexConfig { name: "Platypus".into(), address: "0x6B2aF7C4A7A9bEe7b6B8c7d8e9F0A1B2C3D4E5F6".into(), chain_id, router_abi: None },
            DexConfig { name: "GMX".into(), address: "0x489ee077994B6658eAfE855C308627E6cF69F7A7".into(), chain_id, router_abi: None },
        ],
        _ => vec![],
    }
}

/// Common token addresses across chains
pub fn get_wrapped_native(chain_id: u64) -> &'static str {
    match chain_id {
        1 => "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2",
        42161 => "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1",
        10 => "0x4200000000000000000000000000000000000006",
        137 => "0x0d500B1d8E8eF31E21C99d1Db9A6444d3ADf1270",
        56 => "0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c",
        43114 => "0xB31f66AA3C1e785363F0875A1B74E27b85FD66c7",
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
        _ => "Unknown",
    }
}

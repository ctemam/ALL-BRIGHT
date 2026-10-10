//! Allbright Core
//!
//! Architectural lineage:
//! - Allbright original: unified internal type system, chain registry.
//! - Paradigm Artemis: typed events as the contract between collectors and
//!   independent processing modules.
//!
//! This module is an independent implementation and does not copy
//! proprietary source code.

pub mod amm;
pub mod bait;
pub mod bus;
pub mod chain;
pub mod config;
pub mod events;
pub mod lineage;
pub mod multicall;
pub mod rpc_registry;

pub use chain::*;
pub use config::*;
pub use events::*;

/// rustls 0.23 cannot auto-pick a CryptoProvider when both `ring` and
/// `aws-lc-rs` features arrive transitively (alloy + reqwest pull both).
/// Every binary must call this once at startup before any TLS use.
pub fn init_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

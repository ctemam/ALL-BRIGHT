//! Allbright Execution Layer — library surface.
//!
//! Exposes the relay, calldata and AA modules so auxiliary binaries
//! (e.g. `deploy-executor`) share the same verified code paths as the
//! live executor daemon.

pub mod aa;
pub mod calldata;
pub mod relay;
pub mod simgate;

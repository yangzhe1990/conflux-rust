use core::ledger::{DEFAULT_LEDGER_CACHE_SIZE, MIN_LEDGER_CACHE_MB};
use std::cmp::max;

/// Configuration for application cache sizes.
/// All	values are represented in MB.
#[derive(Debug, PartialEq)]
pub struct CacheConfig {
    /// Size of blockchain cache.
    blockchain: usize,
}

impl Default for CacheConfig {
    fn default() -> Self { CacheConfig::new(DEFAULT_LEDGER_CACHE_SIZE) }
}

impl CacheConfig {
    /// Creates new cache config with gitven details.
    pub fn new(blockchain: usize) -> Self { CacheConfig { blockchain } }

    /// Size of the blockchain cache.
    pub fn blockchain(&self) -> usize {
        max(self.blockchain, MIN_LEDGER_CACHE_MB)
    }
}

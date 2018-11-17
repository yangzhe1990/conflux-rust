/// Configuration for application cache sizes.
/// All	values are represented in MB.
#[derive(Debug, PartialEq)]
pub struct CacheConfig {
    /// Size of blockchain cache.
    blockchain: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        CacheConfig::new(8)
    }
}

impl CacheConfig {
    /// Creates new cache config with gitven details.
    pub fn new(blockchain: usize) -> Self {
        CacheConfig { blockchain }
    }

    /// Size of the blockchain cache.
    #[allow(dead_code)]
    pub fn blockchain(&self) -> usize {
        self.blockchain
    }
}

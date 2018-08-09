use encoded;
use ethereum_types::{H256, U256};
use header::Header;
use network::PeerId;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
pub use types::*;

/// Familial details concerning a block
#[derive(Debug, Clone)]
pub struct BlockDetails {
    /// Block number
    pub number: BlockNumber,
    /// Total difficulty of the block and all its parents
    pub total_difficulty: U256,
    /// Parent block hash
    pub parent: H256,
    /// List of children block hashes
    pub children: Vec<H256>,
    /// Whether the block is considered finalized
    pub is_finalized: bool,
    /// Additional block metadata
    pub metadata: Option<Vec<u8>>,
}

/// Contains information on a best block that is specific to the consensus engine.
///
/// For GHOST fork-choice rule it would typically describe the block with highest
/// combined difficulty (usually the block with the highest block number).
///
/// Sometimes refered as 'latest block'.
pub struct BestBlock {
    /// Best block decoded header.
    pub header: Header,
    /// Best block uncompressed bytes.
    pub block: encoded::Block,
    /// Best block total difficulty.
    pub total_difficulty: U256,
}

pub struct Ledger {
    // All locks must be captured in the order declared here.
    best_block: RwLock<BestBlock>,

    pub block_headers: RwLock<HashMap<H256, encoded::Header>>,
    block_bodies: RwLock<HashMap<H256, encoded::Body>>,
    /// maintain the main chain blocks
    block_hashes: RwLock<HashMap<BlockNumber, H256>>,
    block_details: RwLock<HashMap<H256, BlockDetails>>,
}

pub type SharedLedger = Arc<Ledger>;

impl Ledger {
    pub fn new() -> Self {
        Ledger {
            best_block: RwLock::new(BestBlock {
                header: Default::default(),
                block: encoded::Block::new(Vec::new()),
                total_difficulty: 0.into(),
            }),
            block_headers: RwLock::new(HashMap::new()),
            block_bodies: RwLock::new(HashMap::new()),
            block_hashes: RwLock::new(HashMap::new()),
            block_details: RwLock::new(HashMap::new()),
        }
    }

    pub fn new_shared() -> SharedLedger {
        Arc::new(Self::new())
    }

    /// Get the hash of given block's number.
    fn block_hash_of_number(&self, index: BlockNumber) -> Option<H256> {
        if let Some(v) = self.block_hashes.read().get(&index) {
            Some(v.clone())
        } else {
            None
        }
    }

    pub fn block_hash(&self, id: BlockId) -> Option<H256> {
        match id {
            BlockId::Hash(hash) => Some(hash),
            BlockId::Number(number) => self.block_hash_of_number(number),
            BlockId::Earliest => self.block_hash_of_number(0),
            BlockId::Latest => Some(self.best_block_hash()),
        }
    }

    /// Get best block hash.
    pub fn best_block_hash(&self) -> H256 {
        self.best_block.read().header.hash()
    }

    /// Get block header data
    pub fn block_header_data(&self, hash: &H256) -> Option<encoded::Header> {
        let read = self.block_headers.read();
        if let Some(v) = read.get(hash) {
            return Some(v.clone());
        } else {
            None
        }
    }

    /// Returns reference to genesis hash.
    fn genesis_hash(&self) -> H256 {
        self.block_hash_of_number(0)
            .expect("Genesis hash should always exist")
    }

    /// Returns general ledger information
    pub fn ledger_info(&self) -> LedgerInfo {
        let best_block = self.best_block.read();
        let genesis_hash = self.genesis_hash();
        LedgerInfo {
            genesis_hash,
            total_difficulty: best_block.total_difficulty,
            best_block_hash: best_block.header.hash(),
            best_block_number: best_block.header.number(),
        }
    }

    pub fn block_header_exists(&self, hash: &H256) -> bool {
        self.block_headers.read().contains_key(hash)
    }

    pub fn block_header(&self, id: BlockId) -> Option<encoded::Header> {
        self.block_hash(id)
            .and_then(|hash| self.block_header_data(&hash))
    }
}

impl Default for Ledger {
    // FIXME: Fix this default trait as the initial state of the ledger
    fn default() -> Self { Self::new() }
}

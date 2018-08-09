use super::super::encoded;
use ethereum_types::{H256, U256};
use parking_lot::RwLock;
use std::collections::HashMap;
pub use types::*;

pub struct ConfluxLedger {
    /// Blockchain difficulty.
    pub total_difficulty: U256,
    /// Genesis block hash.
    pub genesis_hash: H256,
    /// Best blockchain block hash.
    pub best_block_hash: H256,
    /// Best ledger block number.
    pub best_block_number: BlockNumber,

    block_headers: RwLock<HashMap<H256, encoded::Header>>,
    block_bodies: RwLock<HashMap<H256, encoded::Body>>,
    block_hashes: RwLock<HashMap<BlockNumber, H256>>,
}

impl ConfluxLedger {
    /// Get the hash of given block's number.
    pub fn block_hash(&self, index: BlockNumber) -> Option<H256> {
        if let Some(v) = self.block_hashes.read().get(&index) {
            Some(v.clone())
        } else {
            None
        }
    }

    /// Get best block hash.
    pub fn best_block_hash(&self) -> H256 { self.best_block_hash }

    /// Get block header data
    pub fn block_header_data(&self, hash: &H256) -> Option<encoded::Header> {
        let read = self.block_headers.read();
        if let Some(v) = read.get(hash) {
            return Some(v.clone());
        } else {
            None
        }
    }

    /// Returns general ledger information
    pub fn ledger_info(&self) -> LedgerInfo {
        LedgerInfo {
            genesis_hash: self.genesis_hash,
            total_difficulty: self.total_difficulty,
            best_block_hash: self.best_block_hash,
            best_block_number: self.best_block_number,
        }
    }
}

impl Default for ConfluxLedger {
    // FIXME: Fix this default trait as the initial state of the ledger
    fn default() -> ConfluxLedger {
        ConfluxLedger {
            total_difficulty: 0.into(),
            genesis_hash: 0.into(),
            best_block_hash: 0.into(),
            best_block_number: 0,

            block_headers: RwLock::new(HashMap::new()),
            block_bodies: RwLock::new(HashMap::new()),
            block_hashes: RwLock::new(HashMap::new()),
        }
    }
}

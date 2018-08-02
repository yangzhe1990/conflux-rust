extern crate ethereum_types;

use ethereum_types::{H256};

/// Type for block number.
pub type BlockNumber = u64;

/// Information about the ledger gathered together.
pub struct LedgerInfo {
    /// Best ledger block number.
    pub best_block_number: BlockNumber,
}

/// Uniquely identifies block.
#[derive(Debug, PartialEq, Copy, Clone, Hash, Eq)]
pub enum BlockId {
    /// Block's sha3.
    Hash(H256),
    ///
    Number(BlockNumber),
    /// Earliest block (genesis).
    Earliest,
    /// Latest mined block.
    Latest,
}

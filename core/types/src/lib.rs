extern crate ethereum_types;

use ethereum_types::{H256, U256};

/// Type for block number.
pub type BlockNumber = u64;

/// Information about the ledger gathered together.
#[derive(Clone, Debug)]
pub struct LedgerInfo {
    /// Blockchain difficulty.
	pub total_difficulty: U256,
    /// Genesis block hash.
	pub genesis_hash: H256,
	/// Best blockchain block hash.
	pub best_block_hash: H256,
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

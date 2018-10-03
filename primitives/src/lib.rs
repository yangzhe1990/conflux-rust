extern crate ethcore_bytes as bytes;
extern crate ethereum_types;
extern crate ethkey;
extern crate keccak_hash as hash;
extern crate rlp;

pub use block_header::{BlockHeader, BlockHeaderBuilder};
use ethereum_types::H256;
pub use transaction::{SignedTransaction, Transaction, TransactionWithSignature};

/// Type for block number.
pub type BlockNumber = u64;

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

pub mod transaction;
pub mod block_header;

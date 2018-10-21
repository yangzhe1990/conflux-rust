extern crate ethcore_bytes as bytes;
extern crate ethereum_types;
extern crate ethkey;
extern crate keccak_hash as hash;
extern crate rlp;

pub type BlockNumber = u64;
pub type EpochNumber = u64;

pub mod block;
pub mod block_header;
pub mod transaction;
pub mod account;

pub use block::Block;
pub use block_header::{BlockHeader, BlockHeaderBuilder};
pub use transaction::{
    SignedTransaction, Transaction, TransactionWithSignature,
};

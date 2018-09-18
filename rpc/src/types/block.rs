use serde::Serialize;
use types::{H160, H256, U256};

#[derive(Debug, Serialize)]
pub struct Block {
    pub hash: H256,
    pub parent_hash: H256,
    pub author: H160,
    pub state_merkle_root: H256,
    pub transactions_merkle_root: H256,
    pub timestamp: U256,
}

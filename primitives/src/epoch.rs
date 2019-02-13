use ethereum_types::{H256, U64};

pub type EpochId = H256;

/// Uniquely identifies epoch.
pub enum EpochNumber {
    /// Epoch number within canon blockchain.
    Number(U64),
    /// Earliest block (genesis).
    Earliest,
    /// Latest mined block.
    Latest,
}

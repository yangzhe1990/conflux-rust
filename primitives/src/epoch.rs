use ethereum_types::{H256, U64};

pub type EpochId = H256;

/// Uniquely identifies epoch.
#[derive(Debug, Clone)]
pub enum EpochNumber {
    /// Epoch number within canon blockchain.
    Number(U64),
    /// Earliest block (genesis).
    Earliest,
    /// Latest mined block.
    LatestMined,
    /// Latest block with state.
    LatestState,
}

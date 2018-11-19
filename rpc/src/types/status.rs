use types::H256;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// Hash of the block
    pub best_hash: H256,
    /// The number of epochs
    pub epoch_number: usize,
    /// The number of blocks
    pub block_number: usize,
    /// The number of pending transactions
    pub pending_tx_number: usize,
}
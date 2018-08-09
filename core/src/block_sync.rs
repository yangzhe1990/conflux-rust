use rlp::{self, Rlp};

#[derive(Eq, PartialEq, Debug)]
pub enum BlockSyncError {
    /// Synced block is rejected as invalid. Peer should be dropped.
    Invalid,
    /// Synced block is not needed.
    Useless,
}

impl From<rlp::DecoderError> for BlockSyncError {
    fn from(_: rlp::DecoderError) -> BlockSyncError { BlockSyncError::Invalid }
}

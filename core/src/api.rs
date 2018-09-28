use network::ProtocolId;
use primitives::BlockNumber;

pub const CONFLUX_PROTOCOL: ProtocolId = *b"cfx";

pub trait LedgerExecutor: Send + Sync {
    fn execute_up_to(&self, number: BlockNumber);
}

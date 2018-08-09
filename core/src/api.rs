use ledger::LedgerRef;
use network::ProtocolId;
use types::BlockNumber;

pub const CONFLUX_PROTOCOL: ProtocolId = *b"cfx";

/// Represents what has to be handled by actor listening to chain events
pub trait LedgerCore: Send + Sync {
    /// fires when chain has new blocks.
    fn new_blocks(&self) {
        // does nothing by default
    }

    /// fires when chain achieves active mode
    fn start(&mut self) {
        // does nothing by default
    }

    /// fires when chain achieves passive mode
    fn stop(&self) {
        // does nothing by default
    }

    /// fires when chain broadcasts a message
    fn broadcast(&self) {}

    /// fires when new transactions are received from a peer
    fn transactions_received(&self) {
        // does nothing by default
    }
}

pub trait LedgerExecutor: Send + Sync {
    fn on_new_best_block(&self, number: BlockNumber, ledger: LedgerRef);
}

use network::{NetworkContext};
use core::LedgerEngineInterface;

/// Wraps `NetworkContext` and the ledger engine interface
pub struct SyncIoContext<'s> {
    network: &'s NetworkContext,
    ledger: &'s LedgerEngineInterface,
}

impl<'s> SyncIoContext<'s> {
    /// Creates a new instance from the `NetworkContext` and the ledger engine interface reference.
    pub fn new(network: &'s NetworkContext, ledger: &'s LedgerEngineInterface) -> SyncIoContext<'s> {
        SyncIoContext {
            network: network,
            ledger: ledger,
        }
    }
}

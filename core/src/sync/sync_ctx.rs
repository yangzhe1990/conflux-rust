use execution_engine::ExecutionEngine;
use ledger::Ledger;
use network::{Error, NetworkContext, PeerId};

/// Wraps `NetworkContext` and the ledger engine interface
pub struct SyncContext<'s> {
    network: &'s NetworkContext,
    ledger: &'s Ledger,
    execution_engine: &'s ExecutionEngine,
}

impl<'s> SyncContext<'s> {
    /// Creates a new instance from the `NetworkContext` and the ledger engine interface reference.
    pub fn new(
        network: &'s NetworkContext, ledger: &'s Ledger,
        execution_engine: &'s ExecutionEngine,
    ) -> SyncContext<'s>
    {
        SyncContext {
            network: network,
            ledger: ledger,
            execution_engine: execution_engine,
        }
    }

    pub fn ledger(&self) -> &Ledger { self.ledger }

    pub fn execution_engine(&self) -> &ExecutionEngine { self.execution_engine }

    pub fn disconnect_peer(&mut self, peer_id: PeerId) {
        self.network.disconnect_peer(peer_id);
    }

    pub fn is_expired(&self) -> bool { false }

    pub fn disable_peer(&mut self, _peer_id: PeerId) {
        // FIXME: Implement this!
    }

    pub fn send(
        &mut self, peer_id: PeerId, data: Vec<u8>,
    ) -> Result<(), Error> {
        self.network.send(peer_id, data)
    }
}

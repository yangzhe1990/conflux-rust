use super::PacketId;
use ledger::Ledger;
use network::{Error, NetworkContext, PeerId};

/// Wraps `NetworkContext` and the ledger engine interface
pub struct SyncContext<'s> {
    network: &'s NetworkContext,
    ledger: &'s Ledger,
}

impl<'s> SyncContext<'s> {
    /// Creates a new instance from the `NetworkContext` and the ledger engine interface reference.
    pub fn new(
        network: &'s NetworkContext, ledger: &'s Ledger,
    ) -> SyncContext<'s> {
        SyncContext {
            network: network,
            ledger: ledger,
        }
    }

    pub fn ledger(&self) -> &Ledger { return self.ledger; }

    pub fn disconnect_peer(&mut self, peer_id: PeerId) {
        self.network.disconnect_peer(peer_id);
    }

    pub fn is_expired(&self) -> bool { false }

    pub fn disable_peer(&mut self, peer_id: PeerId) {}

    pub fn send(
        &mut self, peer_id: PeerId, data: Vec<u8>,
    ) -> Result<(), Error> {
        self.network.send(peer_id, data)
    }
}

use super::PacketId;
use ledger::Ledger;
use network::{Error, NetworkContext, PeerId};

/// IO interface for the syncing handler.
pub trait SyncIo {
    /// Respond to current request with a packet. Can be called from an IO handler for incoming packet.
    fn respond(
        &mut self, packet_id: PacketId, data: Vec<u8>,
    ) -> Result<(), Error>;
    /// Get the ledger
    fn ledger(&self) -> &Ledger;
    /// Disconnect peer
    fn disconnect_peer(&mut self, peer_id: PeerId);
    /// Check if the session is expired
    fn is_expired(&self) -> bool;
    /// Disable a peer
    fn disable_peer(&mut self, peer_id: PeerId);
}

/// Wraps `NetworkContext` and the ledger engine interface
pub struct SyncIoContext<'s> {
    network: &'s NetworkContext,
    ledger: &'s Ledger,
}

impl<'s> SyncIoContext<'s> {
    /// Creates a new instance from the `NetworkContext` and the ledger engine interface reference.
    pub fn new(
        network: &'s NetworkContext, ledger: &'s Ledger,
    ) -> SyncIoContext<'s> {
        SyncIoContext {
            network: network,
            ledger: ledger,
        }
    }
}

impl<'s> SyncIo for SyncIoContext<'s> {
    fn respond(
        &mut self, packet_id: PacketId, data: Vec<u8>,
    ) -> Result<(), Error> {
        Ok(())
    }

    fn ledger(&self) -> &Ledger { return self.ledger; }

    fn disconnect_peer(&mut self, peer_id: PeerId) {
        self.network.disconnect_peer(peer_id);
    }

    fn is_expired(&self) -> bool { false }

    fn disable_peer(&mut self, peer_id: PeerId) {}
}

use super::{PacketId};
use network::{PeerId, NetworkContext, Error};
use core::LedgerEngineInterface;

/// IO interface for the syncing handler.
pub trait SyncIo {
    /// Respond to current request with a packet. Can be called from an IO handler for incoming packet.
    fn respond(&mut self, packet_id: PacketId, data: Vec<u8>) -> Result<(), Error>;
    /// Get the ledger
    fn ledger(&self) -> &LedgerEngineInterface;
    /// Disconnect peer
    fn disconnect_peer(&mut self, peer_id: PeerId);
}

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

impl<'s> SyncIo for SyncIoContext<'s> {
    fn respond(&mut self, packet_id: PacketId, data: Vec<u8>) -> Result<(), Error>{
        Ok(())
    }

    fn ledger(&self) -> &LedgerEngineInterface {
        return self.ledger;
    }

    fn disconnect_peer(&mut self, peer_id: PeerId) {
        self.network.disconnect_peer(peer_id);
    }
}

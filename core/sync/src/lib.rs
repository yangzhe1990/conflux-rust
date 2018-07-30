mod api;

pub use api::*;

use network::{NetworkProtocolHandler, NetworkContext, 
              PeerId, ProtocolId};
use io::{TimerToken};

/// Conflux network sync engine
pub struct ConfluxSync {
    /// Network service
    network: NetworkService,
    /// Main protocol handler
    sync_handler: Arc<SyncProtocolHandler>,
    /// The main subprotocol name
    subprotocol_name: [u8; 3],
}

impl ConfluxSync {

}

struct SyncProtocolHandler {
}

impl NetworkProtocolHandler for SyncProtocolHandler {
    fn initialize(&self, io: &NetworkContext) {
    }

	fn read(&self, io: &NetworkContext, peer: &PeerId, packet_id: u8, data: &[u8]) {
    }

	fn connected(&self, io: &NetworkContext, peer: &PeerId) {
    }

	fn disconnected(&self, io: &NetworkContext, peer: &PeerId) {
    }

	fn timeout(&self, io: &NetworkContext, timer: TimerToken) {
    }
}

impl ChainNotify for ConfluxSync {
    fn new_blocks(
        &self,
    ) {
    }

    fn start(&self) {
    }

    fn stop(&self) {
    }

    fn broadcast(&self,) {}

    fn transactions_received(&self,
    ) {
    }
}


extern crate network;
extern crate ethcore_io as io;
use std::sync::{Arc};
#[macro_use] 
extern crate log;

mod api;

pub use api::*;

use network::{NetworkService, NetworkProtocolHandler, NetworkContext, NetworkConfiguration,
              PeerId, ProtocolId, Error};
use io::{TimerToken};

/// Sync configuration
#[derive(Debug, Clone, Copy)]
pub struct SyncConfig {
    /// Main "cfx" subprotocol name.
    pub subprotocol_name: [u8; 3],
}

/// ConfluxSync initialization parameters.
pub struct Params {
    /// Configuration.
    pub config: SyncConfig,
    /// Network layer configuration.
    pub network_config: NetworkConfiguration,
}

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
    /// Create and register protocol with the network service
    pub fn new(params: Params) -> Result<Arc<ConfluxSync>, Error> {
        let service = NetworkService::new(&params.network_config)?;
        let sync = Arc::new(ConfluxSync {
            network: service,
            sync_handler: Arc::new(SyncProtocolHandler {
                
            }),
            subprotocol_name: params.config.subprotocol_name,
        });
        Ok(sync)
    }
}

struct SyncProtocolHandler {
}

impl NetworkProtocolHandler for SyncProtocolHandler {
    fn initialize(&self, io: &NetworkContext) {
    }

    fn on_message(
        &self, io: &NetworkContext, peer: PeerId, msg_id: u8, data: Vec<u8>,
    ) {
    }

    fn on_peer_connected(&self, io: &NetworkContext, peer: PeerId) {
    }

	fn on_peer_disconnected(&self, io: &NetworkContext, peer: PeerId) {
    }

	fn on_timeout(&self, io: &NetworkContext, timer: TimerToken) {
    }
}

impl ChainNotify for ConfluxSync {
    fn new_blocks(
        &self,
    ) {
    }

    fn start(&self) {
        match self.network.start() {
            Err(err) => {
                warn!("Error starting network");
            },
            _ => {},
        }
    }

    fn stop(&self) {
    }

    fn broadcast(&self,) {}

    fn transactions_received(&self,
    ) {
    }
}


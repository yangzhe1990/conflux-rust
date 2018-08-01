extern crate ethereum_types;
extern crate network;
extern crate ethcore_io as io;
extern crate parking_lot;
extern crate core;
extern crate rlp;

#[macro_use] 
extern crate log;

use parking_lot::RwLock;
use std::sync::{Arc};
mod api;
mod dag;
mod sync_ctx;

pub use api::*;
pub use dag::{DagSync};

use network::{NetworkService, NetworkProtocolHandler, NetworkContext, NetworkConfiguration,
              PeerId, ProtocolId, Error};
use io::{TimerToken};
use ethereum_types::H256;
use core::LedgerEngineInterface;
use sync_ctx::SyncIoContext;

/// Sync configuration
#[derive(Debug, Clone, Copy)]
pub struct SyncConfig {
    /// Main "cfx" subprotocol name.
    pub subprotocol_name: [u8; 3],
}

impl Default for SyncConfig {
    fn default() -> SyncConfig {
        SyncConfig {
            subprotocol_name: CONFLUX_PROTOCOL,
        }
    }
}

/// ConfluxSync initialization parameters.
pub struct Params {
    /// Configuration.
    pub config: SyncConfig,
    /// Network layer configuration.
    pub network_config: NetworkConfiguration,
    /// Ledger interface
    pub ledger: Arc<LedgerEngineInterface>,
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
        let dag_sync = DagSync::new();
        let service = NetworkService::new(&params.network_config)?;

        let sync = Arc::new(ConfluxSync {
            network: service,
            sync_handler: Arc::new(SyncProtocolHandler {
                ledger: params.ledger,
                sync: RwLock::new(dag_sync),
            }),
            subprotocol_name: params.config.subprotocol_name,
        });
        Ok(sync)
    }
}

struct SyncProtocolHandler {
    /// Shared ledger interface.
    ledger: Arc<LedgerEngineInterface>,
    /// Sync strategy
    sync: RwLock<DagSync>,
}

impl NetworkProtocolHandler for SyncProtocolHandler {
    fn initialize(&self, io: &NetworkContext) {
    }

    fn on_message(
        &self, io: &NetworkContext, peer: PeerId, msg_id: u8, data: &[u8],
    ) {
        DagSync::dispatch_packet(&self.sync, &mut SyncIoContext::new(io, &*self.ledger), peer, msg_id, data);
    }

    fn on_peer_connected(&self, io: &NetworkContext, peer: PeerId) {
        trace!("sync::connected");
    }

	fn on_peer_disconnected(&self, io: &NetworkContext, peer: PeerId) {
        trace!("sync::disconnected");
    }

	fn on_timeout(&self, io: &NetworkContext, timer: TimerToken) {
        trace!("sync::timeout");
    }
}

impl LedgerNotify for ConfluxSync {
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

        // TODO:register_protocol
    }

    fn stop(&self) {
    }

    fn broadcast(&self,) {}

    fn transactions_received(&self,
    ) {
    }
}


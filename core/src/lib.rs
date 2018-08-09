extern crate common_types as types;
extern crate ethereum_types;
extern crate io;
extern crate keccak_hash as hash;
extern crate network;
extern crate parity_bytes as bytes;
extern crate parking_lot;
extern crate rlp;

#[macro_use]
extern crate log;

mod api;
mod block_sync;
mod dag;
pub mod encoded;
pub mod ledger;
mod sync_ctx;
pub mod header;

use ledger::LedgerRef;
use parking_lot::RwLock;
use std::sync::Arc;
use ledger::Ledger;
use rlp::{Rlp, RlpStream};

pub use api::*;
pub use dag::*;

use ethereum_types::H256;
use io::TimerToken;
use network::{
    Error, NetworkConfiguration, NetworkContext, NetworkProtocolHandler,
    NetworkService, PeerId, ProtocolId,
};
use sync_ctx::SyncIoContext;

/// Protocol handler level packet id
pub type PacketId = u8;

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
    pub ledger: LedgerRef,
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
    pub fn new(params: Params) -> Self {
        let dag_sync = SyncState::new();
        let service = NetworkService::new(params.network_config);

        ConfluxSync {
            network: service,
            sync_handler: Arc::new(SyncProtocolHandler {
                ledger: params.ledger,
                sync: RwLock::new(dag_sync),
            }),
            subprotocol_name: params.config.subprotocol_name,
        }
    }
}

struct SyncProtocolHandler {
    /// Shared ledger interface.
    ledger: LedgerRef,
    /// Sync strategy
    sync: RwLock<SyncState>,
}

impl NetworkProtocolHandler for SyncProtocolHandler {
    fn initialize(&self, io: &NetworkContext) {}

    fn on_message(&self, io: &NetworkContext, peer: PeerId, data: &[u8]) {
        let mut packet_id: u8 = 0;
        let rlp = Rlp::new(data);
        let result = rlp.val_at(0);
        match result {
            Err(e) => {
                debug!(target: "PacketId decode error", "{:?}", e);
                return;
            },
            Ok(res) => {
                packet_id = res;
            }
        }
        SyncState::dispatch_packet(&self.sync, &mut SyncIoContext::new(io, &*self.ledger), peer, packet_id, rlp);
    }

    fn on_peer_connected(&self, io: &NetworkContext, peer: PeerId) {
        self.sync.write().on_peer_connected(
            &mut SyncIoContext::new(io, &*self.ledger),
            peer,
        );
        trace!("sync::connected");
    }

    fn on_peer_disconnected(&self, io: &NetworkContext, peer: PeerId) {
        trace!("sync::disconnected");
    }

    fn on_timeout(&self, io: &NetworkContext, timer: TimerToken) {
        trace!("sync::timeout");
    }
}

impl LedgerCore for ConfluxSync {
    fn new_blocks(&self) {}

    fn start(&mut self) {
        match self.network.start() {
            Err(err) => {
                warn!("Error starting network");
            }
            _ => {}
        }

        self.network
            .register_protocol(
                self.sync_handler.clone(),
                self.subprotocol_name,
                &[CONFLUX_PROTOCOL_VERSION_1],
            )
            .unwrap_or_else(|e| {
                warn!("Error registering conflux protocol: {:?}", e)
            });
    }

    fn stop(&self) {}

    fn broadcast(&self) {}

    fn transactions_received(&self) {}
}

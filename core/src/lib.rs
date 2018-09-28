extern crate primitives;
extern crate ethereum_types;
extern crate ethkey;
extern crate io;
extern crate keccak_hash as hash;
extern crate network;
extern crate ethcore_bytes as bytes;
extern crate parking_lot;
extern crate rlp;
extern crate secret_store;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
extern crate rand;

mod api;
pub mod block;
pub mod block_header;
mod block_sync;
pub mod encoded;
pub mod error;
pub mod execution_engine;
mod ledger;
pub mod state;
mod sync;
pub mod transaction_pool;

use network::NodeId;
use parking_lot::RwLock;
use rlp::Rlp;
use std::net::SocketAddr;
use std::sync::Arc;

pub use api::*;
pub use execution_engine::{ExecutionEngine, ExecutionEngineRef};
pub use ledger::{Ledger, LedgerRef};
pub use network::PeerInfo;
pub use state::TEST_ADDRESS;
pub use sync::*;

use ethereum_types::{H256, U256};
use io::TimerToken;
use network::{
    Error, NetworkConfiguration, NetworkContext, NetworkProtocolHandler,
    NetworkService, PeerId,
};
use sync_ctx::SyncContext;

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
pub struct SyncParams {
    /// Configuration.
    pub config: SyncConfig,
    /// Network layer configuration.
    pub network_config: NetworkConfiguration,
    /// Ledger
    pub ledger: LedgerRef,
    /// Execution engine
    pub execution_engine: ExecutionEngineRef,
}

pub type SyncEngineRef = Arc<SyncEngine>;

/// Conflux network sync engine
pub struct SyncEngine {
    /// Network service
    network: NetworkService,
    /// Main protocol handler
    sync_handler: Arc<SyncProtocolHandler>,
    /// The main subprotocol name
    subprotocol_name: [u8; 3],
}

impl SyncEngine {
    /// Create and register protocol with the network service
    pub fn new(params: SyncParams) -> Self {
        let sync_state = SyncState::new();
        let service = NetworkService::new(params.network_config);

        SyncEngine {
            network: service,
            sync_handler: Arc::new(SyncProtocolHandler {
                ledger: params.ledger,
                execution_engine: params.execution_engine,
                sync: RwLock::new(sync_state),
            }),
            subprotocol_name: params.config.subprotocol_name,
        }
    }

    pub fn start(&mut self) {
        match self.network.start() {
            Err(_err) => {
                warn!("Error starting network");
            }
            _ => {}
        }

        self.network
            .register_protocol(
                self.sync_handler.clone(),
                self.subprotocol_name,
                &[CONFLUX_PROTOCOL_VERSION_1],
            ).unwrap_or_else(|e| {
                warn!("Error registering conflux protocol: {:?}", e)
            });
    }

    pub fn new_blocks(&self, blocks: &[H256], total_difficulties: &[U256]) {
        self.network.with_context(self.subprotocol_name, |context| {
            let mut sync = self.sync_handler.sync.write();
            sync.new_chain_blocks(
                &mut SyncContext::new(
                    context,
                    &*self.sync_handler.ledger,
                    &*self.sync_handler.execution_engine,
                ),
                blocks,
                total_difficulties,
            );
        });
    }

    pub fn add_peer(&self, addr: SocketAddr) -> Result<NodeId, Error> {
        self.network.add_peer(addr)
    }

    pub fn drop_peer(&self, id: NodeId) -> Result<(), Error> {
        self.network.drop_peer(id)
    }

    pub fn get_peer_info(&self) -> Vec<PeerInfo> {
        self.network.get_peer_info().unwrap()
    }
}

struct SyncProtocolHandler {
    /// Shared ledger
    ledger: LedgerRef,
    /// Shared execution engine,
    execution_engine: ExecutionEngineRef,
    /// Sync strategy
    sync: RwLock<SyncState>,
}

impl NetworkProtocolHandler for SyncProtocolHandler {
    fn initialize(&self, _io: &NetworkContext) {}

    fn on_message(&self, io: &NetworkContext, peer: PeerId, data: &[u8]) {
        let packet_id: u8;
        let rlp = Rlp::new(data);
        let result = rlp.val_at(0);
        match result {
            Err(e) => {
                debug!(target: "PacketId decode error", "{:?}", e);
                return;
            }
            Ok(res) => {
                packet_id = res;
            }
        }
        debug!(target: "sync", "on_message(), peer {:}, packet_id {:}", peer, packet_id);
        SyncState::dispatch_packet(
            &self.sync,
            &mut SyncContext::new(io, &*self.ledger, &*self.execution_engine),
            peer,
            packet_id,
            rlp,
        );
    }

    fn on_peer_connected(&self, io: &NetworkContext, peer: PeerId) {
        trace!(target: "sync", "on_peer_connected {:?}", peer);
        self.sync.write().on_peer_connected(
            &mut SyncContext::new(io, &*self.ledger, &*self.execution_engine),
            peer,
        );
    }

    fn on_peer_disconnected(&self, _io: &NetworkContext, peer: PeerId) {
        trace!(target: "sync", "on_peer_disconnected {:?}", peer);
    }

    fn on_timeout(&self, _io: &NetworkContext, _timer: TimerToken) {
        trace!(target: "sync", "on_timeout()");
    }
}

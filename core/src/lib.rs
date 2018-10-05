extern crate ethcore_bytes as bytes;
extern crate ethereum_types;
extern crate ethkey;
extern crate io;
extern crate keccak_hash as hash;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
extern crate message;
extern crate network;
extern crate parking_lot;
extern crate primitives;
extern crate rand;
extern crate rlp;
extern crate secret_store;

pub use api::*;
use ethereum_types::{H256, U256};
pub use execution_engine::{ExecutionEngine, ExecutionEngineRef};
use io::TimerToken;
pub use ledger::{Ledger, LedgerRef};
use network::node_table::NodeEntry;
pub use network::PeerInfo;
use network::{
    Error, NetworkConfiguration, NetworkContext, NetworkProtocolHandler,
    NetworkService, PeerId,
};
use parking_lot::RwLock;
use rlp::Rlp;
pub use state::TEST_ADDRESS;
use std::net::SocketAddr;
use std::sync::Arc;
pub use sync::*;
use sync_ctx::SyncContext;
use message::MsgId;

mod api;
mod block_sync;
pub mod encoded;
pub mod error;
pub mod execution_engine;
mod ledger;
pub mod state;
mod sync;
pub mod transaction_pool;

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
            )
            .unwrap_or_else(|e| {
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

    pub fn add_peer(&self, node: NodeEntry) -> Result<(), Error> {
        self.network.add_peer(node)
    }

    pub fn drop_peer(&self, node: NodeEntry) -> Result<(), Error> {
        self.network.drop_peer(node)
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
        let msg_id = data[0];
        let rlp = Rlp::new(&data[1..]);
        debug!(target: "sync", "on_message(), peer {:}, msg_id {:}", peer, msg_id);
        SyncState::dispatch_message(
            &self.sync,
            &mut SyncContext::new(io, &*self.ledger, &*self.execution_engine),
            peer,
            msg_id.into(),
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

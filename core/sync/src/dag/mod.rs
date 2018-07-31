mod sync_handler;

use self::sync_handler::SyncHandler;
use parking_lot::RwLock;
use sync_ctx::SyncIoContext;
use network::{PeerId};

/// Conflux DAG sync handler.
pub struct DagSync {
}

impl DagSync {
    /// Create a new instance of syncing strategy.
    pub fn new() -> DagSync {
        let sync = DagSync {
        };

        sync
    }

    /// Dispatch incoming requests and responses
    pub fn dispatch_packet(sync: &RwLock<DagSync>, io: &mut SyncIoContext, peer: PeerId, packet_id: u8, data: &[u8]) {
        SyncHandler::dispatch_packet(sync, io, peer, packet_id, data)
    }
}

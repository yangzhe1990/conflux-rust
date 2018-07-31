use super::{
    DagSync,
};

use parking_lot::RwLock;
use sync_ctx::SyncIoContext;
use network::{PeerId};

pub struct SyncHandler;

impl SyncHandler {
    /// Dispatch incoming requests and responses
    pub fn dispatch_packet(sync: &RwLock<DagSync>, io: &mut SyncIoContext, peer: PeerId, packet_id: u8, data: &[u8]) {
    }
}

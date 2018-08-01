mod sync_handler;

use self::sync_handler::SyncHandler;
use parking_lot::RwLock;
use sync_ctx::SyncIo;
use network::{PeerId, MsgId};
use rlp::{Rlp, RlpStream, DecoderError};

const STATUS_PACKET: u8 = 0x00;
pub const GET_BLOCK_HEADERS_PACKET: u8 = 0x01;
pub const GET_BLOCK_BODIES_PACKET: u8 = 0x02;

pub type PacketDecodeError = DecoderError;
pub type RlpResponseResult = Result<Option<(MsgId, RlpStream)>, PacketDecodeError>;

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
    pub fn dispatch_packet(sync: &RwLock<DagSync>, io: &mut SyncIo, peer: PeerId, packet_id: u8, data: &[u8]) {
        SyncHandler::dispatch_packet(sync, io, peer, packet_id, data)
    }

    /// Handle incoming packet from peer which does not require response
    /// Require write lock on DagSync
    pub fn on_packet(&mut self, io: &mut SyncIo, peer: PeerId, packet_id: u8, data: &[u8]) {
        debug!(target: "sync", "{} -> Dispatching packet: {}", peer, packet_id);
        SyncHandler::on_packet(self, io, peer, packet_id, data);
    }
}

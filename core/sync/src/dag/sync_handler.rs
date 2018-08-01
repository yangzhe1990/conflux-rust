use super::{
    PacketId,
    DagSync,
    GET_BLOCK_BODIES_PACKET,
    GET_BLOCK_HEADERS_PACKET,
    RlpResponseResult,
    PacketDecodeError,
};

use parking_lot::RwLock;
use sync_ctx::SyncIo;
use network::{PeerId, Error};
use rlp::{Rlp, RlpStream};

pub struct SyncHandler;

impl SyncHandler {
    /// Dispatch incoming requests and responses
    pub fn dispatch_packet(sync: &RwLock<DagSync>, io: &mut SyncIo, peer: PeerId, packet_id: PacketId, data: &[u8]) {
        let rlp = Rlp::new(data);
        let result = match packet_id {
            GET_BLOCK_BODIES_PACKET => SyncHandler::return_rlp(io, &rlp, peer,
                SyncHandler::return_block_bodies,
                |e| format!("Error sending block bodies: {:?}", e)),
            GET_BLOCK_HEADERS_PACKET => SyncHandler::return_rlp(io, &rlp, peer,
                SyncHandler::return_block_headers,
                |e| format!("Error sending block headers: {:?}", e)),
            _ => {
                sync.write().on_packet(io, peer, packet_id, data);
                Ok(())
            }
        };
    }

    fn return_rlp<FRlp, FError>(io: &mut SyncIo, rlp: &Rlp, peer: PeerId, rlp_func: FRlp, error_func: FError) -> Result<(), PacketDecodeError>
        where FRlp : Fn(&SyncIo, &Rlp, PeerId) -> RlpResponseResult,
            FError : FnOnce(Error) -> String
    {
        let response = rlp_func(io, rlp, peer);
        match response {
            Err(e) => Err(e),
            Ok(Some((packet_id, rlp_stream))) => {
                io.respond(packet_id, rlp_stream.out()).unwrap_or_else(
                    |e| debug!(target: "sync", "{:?}", error_func(e)));
                Ok(())
            },
            _ => Ok(())
        }
    }

    /// Respond to GetBlockBodies request
    fn return_block_bodies(io: &SyncIo, r: &Rlp, peer_id: PeerId) -> RlpResponseResult {
        Ok(None)
    }
    
    /// Respond to GetBlockHeaders request
    fn return_block_headers(io: &SyncIo, r: &Rlp, peer_id: PeerId) -> RlpResponseResult {
        Ok(None)
    }

    /// Handle incoming packet from peer which does not require response
    pub fn on_packet(sync: &mut DagSync, io: &mut SyncIo, peer: PeerId, packet_id: PacketId, data: &[u8]) {
    }
}

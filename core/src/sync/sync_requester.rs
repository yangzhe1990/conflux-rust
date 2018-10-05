use bytes::Bytes;
use ethereum_types::H256;
use message::*;
use network::PeerId;
use rlp::RlpStream;
use std::time::Instant;
use sync_ctx::SyncContext;

use super::{
    PeerAsking, SyncState, GET_BLOCK_BODIES_PACKET, GET_BLOCK_HEADERS_PACKET,
};

/// The Conflux Sync Requester: requesting data to other peers
pub struct SyncRequester;

impl SyncRequester {
    /// Request headers from a peer by block hash
    pub fn request_headers_by_hash(
        sync: &mut SyncState, io: &mut SyncContext, peer_id: PeerId, h: &H256,
        count: u64, skip: u64, reverse: bool,
    )
    {
        trace!(target: "sync", "{} <- GetBlockHeaders: {} entries starting from {}", peer_id, count, h);
        SyncRequester::send_request(
            sync,
            io,
            peer_id,
            PeerAsking::BlockHeaders,
            Box::new(
                GetBlockHeaders {
                    block_id: BlockId::Hash(h.clone()),
                    max_headers: count as usize,
                    skip: skip as usize,
                    reverse: reverse
                }
            ),
        );
        let peer = sync.peers.get_mut(&peer_id).expect("peer_id may originate either from on_packet, where it is already validated or from enumerating self.peers. qed");
        peer.asking_hash = Some(h.clone());
    }

    /// Generic request sender
    fn send_request(
        sync: &mut SyncState, io: &mut SyncContext, peer_id: PeerId,
        asking: PeerAsking, msg: Box<Message>,
    )
    {
        if let Some(ref mut peer) = sync.peers.get_mut(&peer_id) {
            if peer.asking != PeerAsking::Nothing {
                warn!(target:"sync", "Asking {:?} while requesting {:?}", peer.asking, asking);
            }
            peer.asking = asking;
            peer.ask_time = Instant::now();
            let mut data = Bytes::new();
            data.push(msg.msg_id().into());
            data.extend_from_slice(&msg.rlp_bytes());
            let result = io.send(peer_id, data);
            if let Err(e) = result {
                debug!(target:"sync", "Error sending request: {:?}", e);
                io.disconnect_peer(peer_id);
            }
        }
    }

    pub fn fetch_block_bodies(
        sync: &mut SyncState, io: &mut SyncContext, peer_id: PeerId,
        hashes: Vec<H256>,
    )
    {
        trace!(target: "sync", "{} <- GetBlockBodies: {} entries starting from {:?}", peer_id,
               hashes.len(), hashes.first());
        SyncRequester::send_request(
            sync,
            io,
            peer_id,
            PeerAsking::BlockBodies,
            Box::new(GetBlockBodies {
                hashes: hashes.clone(),
            }),
        );
        let peer = sync.peers.get_mut(&peer_id).expect("peer_id may originate either from on_packet, where it is already validated or from enumerating self.peers. qed");
        peer.asking_blocks = hashes;
    }
}

use rlp::RlpStream;
use std::time::Instant;
use sync_ctx::SyncIo;
use network::{PeerId};
use ethereum_types::{H256};
use super::super::{PacketId};
use bytes::Bytes;

use super::{
    DagSync,
    PeerAsking,
    GET_BLOCK_HEADERS_PACKET,
};

/// The Conflux Sync Requester: requesting data to other peers
pub struct SyncRequester;

impl SyncRequester {
    /// Request headers from a peer by block hash
	pub fn request_headers_by_hash(sync: &mut DagSync, io: &mut SyncIo, peer_id: PeerId, h: &H256, count: u64, skip: u64, reverse: bool) {
		trace!(target: "sync", "{} <- GetBlockHeaders: {} entries starting from {}", peer_id, count, h);
		let mut rlp = RlpStream::new_list(5);
        rlp.append(&(GET_BLOCK_HEADERS_PACKET as u32));
		rlp.append(h);
		rlp.append(&count);
		rlp.append(&skip);
		rlp.append(&if reverse {1u32} else {0u32});
		SyncRequester::send_request(sync, io, peer_id, PeerAsking::BlockHeaders, rlp.out());
		let peer = sync.peers.get_mut(&peer_id).expect("peer_id may originate either from on_packet, where it is already validated or from enumerating self.peers. qed");
		peer.asking_hash = Some(h.clone());
	}

    /// Generic request sender
	fn send_request(sync: &mut DagSync, io: &mut SyncIo, peer_id: PeerId, asking: PeerAsking, packet: Bytes) {
		if let Some(ref mut peer) = sync.peers.get_mut(&peer_id) {
			if peer.asking != PeerAsking::Nothing {
				warn!(target:"sync", "Asking {:?} while requesting {:?}", peer.asking, asking);
			}
			peer.asking = asking;
			peer.ask_time = Instant::now();
			let result = io.send(peer_id, packet);
			if let Err(e) = result {
				debug!(target:"sync", "Error sending request: {:?}", e);
				io.disconnect_peer(peer_id);
			}
		}
	}
}

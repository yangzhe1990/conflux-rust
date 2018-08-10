use super::{
	PacketDecodeError, PacketId, PeerAsking, PeerInfo, RlpResponseResult,
	SyncState, BLOCK_HEADERS_PACKET, GET_BLOCK_BODIES_PACKET,
	GET_BLOCK_HEADERS_PACKET, MAX_HEADERS_TO_SEND, STATUS_PACKET,
};

use block_sync::BlockSyncError;
use bytes::Bytes;
use ethereum_types::H256;
use network::{Error, PeerId};
use parking_lot::RwLock;
use rlp::{Rlp, RlpStream};
use std::cmp;
use std::time::Instant;
use sync_ctx::SyncContext;
use types::*;

pub struct SyncHandler;

impl SyncHandler {
	/// Dispatch incoming requests and responses
	pub fn dispatch_packet(
		sync: &RwLock<SyncState>, io: &mut SyncContext, peer: PeerId,
		packet_id: PacketId, rlp: Rlp,
	)
	{
		let result = match packet_id {
			GET_BLOCK_BODIES_PACKET => SyncHandler::return_rlp(
				io,
				&rlp,
				peer,
				SyncHandler::return_block_bodies,
				|e| format!("Error sending block bodies: {:?}", e),
			),
			GET_BLOCK_HEADERS_PACKET => SyncHandler::return_rlp(
				io,
				&rlp,
				peer,
				SyncHandler::return_block_headers,
				|e| format!("Error sending block headers: {:?}", e),
			),
			_ => {
				sync.write().on_packet(io, peer, packet_id, &rlp);
				Ok(())
			}
		};
	}

	fn return_rlp<FRlp, FError>(
		io: &mut SyncContext, rlp: &Rlp, peer: PeerId, rlp_func: FRlp,
		error_func: FError,
	) -> Result<(), PacketDecodeError>
	where
		FRlp: Fn(&SyncContext, &Rlp, PeerId) -> RlpResponseResult,
		FError: FnOnce(Error) -> String,
	{
		let response = rlp_func(io, rlp, peer);
		match response {
			Err(e) => Err(e),
			Ok(Some(rlp_stream)) => {
				io.send(peer, rlp_stream.out()).unwrap_or_else(
					|e| debug!(target: "sync", "{:?}", error_func(e)),
				);
				Ok(())
			}
			_ => Ok(()),
		}
	}

	/// Respond to GetBlockBodies request
	fn return_block_bodies(
		io: &SyncContext, r: &Rlp, peer_id: PeerId,
	) -> RlpResponseResult {
		Ok(None)
	}

	/// Respond to GetBlockHeaders request
	fn return_block_headers(
		io: &SyncContext, r: &Rlp, peer_id: PeerId,
	) -> RlpResponseResult {
		// Packet layout:
		// [ block: { P , B_32 }, maxHeaders: P, skip: P, reverse: P in { 0 , 1 } ]
		let max_headers: usize = r.val_at(2)?;
		let skip: usize = r.val_at(3)?;
		let reverse: bool = r.val_at(4)?;
		let last = io.ledger().ledger_info().best_block_number;
		let number = if r.at(1)?.size() == 32 {
			// id is a hash
			let hash: H256 = r.val_at(1)?;
			trace!(target: "sync", "{} -> GetBlockHeaders (hash: {}, max: {}, skip: {}, reverse:{})", peer_id, hash, max_headers, skip, reverse);
			match io.ledger().block_header(BlockId::Hash(hash)) {
				Some(hdr) => {
					let number = hdr.number().into();
					debug_assert_eq!(hdr.hash(), hash);

					if max_headers == 1
						|| io.ledger().block_hash(BlockId::Number(number))
							!= Some(hash)
					{
						// Non canonical header or single header requested
						// TODO: handle single-step reverse hashchains of non-canon hashes
						trace!(target:"sync", "Returning single header: {:?}", hash);
						let mut rlp = RlpStream::new_list(2);
						rlp.append(&(BLOCK_HEADERS_PACKET as u32));
						rlp.append_raw(&hdr.rlp(), 1);
						return Ok(Some(rlp));
					}
					number
				}
				None => {
					let mut rlp = RlpStream::new_list(1);
					rlp.append(&(BLOCK_HEADERS_PACKET as u32));
					return Ok(Some(rlp)); //no such header, return nothing
				}
			}
		} else {
			let number = r.val_at::<BlockNumber>(0)?;
			trace!(target: "sync", "{} -> GetBlockHeaders (number: {}, max: {}, skip: {}, reverse:{})", peer_id, number, max_headers, skip, reverse);
			number
		};

		let mut number = if reverse {
			cmp::min(last, number)
		} else {
			cmp::max(0, number)
		};
		let max_count = cmp::min(MAX_HEADERS_TO_SEND, max_headers);
		let mut count = 0;
		let mut data = Bytes::new();
		let inc = skip.saturating_add(1) as BlockNumber;
		//let overlay = io.chain_overlay().read();

		// We are checking the `overlay` as well since it's where the ForkBlock
		// header is cached : so peers can confirm we are on the right fork,
		// even if we are not synced until the fork block
		//while (number <= last || overlay.contains_key(&number)) && count < max_count {
		while number <= last && count < max_count {
			//if let Some(hdr) = overlay.get(&number) {
			if false {
				//trace!(target: "sync", "{}: Returning cached fork header", peer_id);
				//data.extend_from_slice(hdr);
				//count += 1;
			} else if let Some(hdr) =
				io.ledger().block_header(BlockId::Number(number))
			{
				data.append(&mut hdr.rlp());
				count += 1;
			} else {
				// No required block.
				break;
			}
			if reverse {
				if number <= inc || number == 0 {
					break;
				}
				number = number.saturating_sub(inc);
			} else {
				number = number.saturating_add(inc);
			}
		}
		let mut rlp = RlpStream::new_list((count + 1) as usize);
		rlp.append(&(BLOCK_HEADERS_PACKET as u32));
		rlp.append_raw(&data, count as usize);
		trace!(target: "sync", "{} -> GetBlockHeaders: returned {} entries", peer_id, count);
		Ok(Some(rlp))
	}

	/// Handle incoming packet from peer which does not require response
	pub fn on_packet(
		sync: &mut SyncState, io: &mut SyncContext, peer: PeerId,
		packet_id: PacketId, rlp: &Rlp,
	)
	{
		if packet_id != STATUS_PACKET && !sync.peers.contains_key(&peer) {
			debug!(target:"sync", "Unexpected packet {} from unregistered peer: {}", packet_id, peer);
			return;
		}

		let result = match packet_id {
			STATUS_PACKET => SyncHandler::on_peer_status(sync, io, peer, rlp),
			_ => {
				debug!(target: "sync", "{}: Unknown packet {}", peer, packet_id);
				Ok(())
			}
		};

		match result {
			Err(BlockSyncError::Invalid) => {
				debug!(target:"sync", "{} -> Invalid packet {}", peer, packet_id);
				io.disable_peer(peer);
				sync.deactivate_peer(io, peer);
			}
			Err(BlockSyncError::Useless) => {
				sync.deactivate_peer(io, peer);
			}
			Ok(()) => {}
		}
	}

	/// Called when a new peer is connected
	pub fn on_peer_connected(
		sync: &mut SyncState, io: &mut SyncContext, peer: PeerId,
	) {
		trace!(target: "sync", "== Connected {}", peer);
		if let Err(e) = sync.send_status(io, peer) {
			debug!(target:"sync", "Error sending status request: {:?}", e);
			io.disconnect_peer(peer);
		} else {
			sync.handshaking_peers.insert(peer, Instant::now());
		}
	}

	/// Called by peer to report status
	fn on_peer_status(
		sync: &mut SyncState, io: &mut SyncContext, peer_id: PeerId, r: &Rlp,
	) -> Result<(), BlockSyncError> {
		sync.handshaking_peers.remove(&peer_id);
		let protocol_version: u8 = r.val_at(1)?;
		let peer = PeerInfo {
			protocol_version: protocol_version,
			difficulty: Some(r.val_at(2)?),
			latest_hash: r.val_at(3)?,
			genesis: r.val_at(4)?,
			asking: PeerAsking::Nothing,
			asking_blocks: Vec::new(),
			asking_hash: None,
			ask_time: Instant::now(),
			expired: false,
		};

		trace!(target: "sync", "New peer {} (protocol: {}, difficulty: {:?}, latest:{}, genesis:{})",
			peer_id, peer.protocol_version, peer.difficulty, peer.latest_hash, peer.genesis);
		if io.is_expired() {
			trace!(target: "sync", "Status packet from expired session {}", peer_id);
			return Ok(());
		}

		if sync.peers.contains_key(&peer_id) {
			debug!(target: "sync", "Unexpected status packet from {}", peer_id);
			return Ok(());
		}
		let ledger_info = io.ledger().ledger_info();
		if peer.genesis != ledger_info.genesis_hash {
			trace!(target: "sync", "Peer {} genesis hash mismatch (ours: {}, theirs: {})", peer_id, ledger_info.genesis_hash, peer.genesis);
			return Err(BlockSyncError::Invalid);
		}

		if sync.sync_start_time.is_none() {
			sync.sync_start_time = Some(Instant::now());
		}

		sync.peers.insert(peer_id.clone(), peer);
		// Don't activate peer immediatelly when searching for common block.
		// Let the current sync round complete first.
		sync.active_peers.insert(peer_id.clone());
		debug!(target: "sync", "Connected {}", peer_id);

		sync.peer_status_changed(io, peer_id);
		Ok(())
	}

    fn on_peer_block_headers(sync: &mut SyncState, io: &mut SyncContext, peer_id: PeerId, r: &Rlp) -> Result<(), BlockSyncError> {
        Ok(())
    }
}

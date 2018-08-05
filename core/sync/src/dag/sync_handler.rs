use super::{
    PacketId,
    DagSync,
    MAX_HEADERS_TO_SEND,
    GET_BLOCK_BODIES_PACKET,
    GET_BLOCK_HEADERS_PACKET,
    BLOCK_HEADERS_PACKET,
    RlpResponseResult,
    PacketDecodeError,
};

use parking_lot::RwLock;
use sync_ctx::SyncIo;
use network::{PeerId, Error};
use rlp::{Rlp, RlpStream};
use bytes::Bytes;
use types::*;
use std::cmp;
use ethereum_types::{H256};
use std::time::{Instant};

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
        // Packet layout:
		// [ block: { P , B_32 }, maxHeaders: P, skip: P, reverse: P in { 0 , 1 } ]
		let max_headers: usize = r.val_at(1)?;
		let skip: usize = r.val_at(2)?;
		let reverse: bool = r.val_at(3)?;
		let last = io.ledger().ledger_info().best_block_number;
		let number = if r.at(0)?.size() == 32 {
			// id is a hash
			let hash: H256 = r.val_at(0)?;
			trace!(target: "sync", "{} -> GetBlockHeaders (hash: {}, max: {}, skip: {}, reverse:{})", peer_id, hash, max_headers, skip, reverse);
			match io.ledger().block_header(BlockId::Hash(hash)) {
				Some(hdr) => {
					let number = hdr.number().into();
					debug_assert_eq!(hdr.hash(), hash);

					if max_headers == 1 || io.ledger().block_hash(BlockId::Number(number)) != Some(hash) {
						// Non canonical header or single header requested
						// TODO: handle single-step reverse hashchains of non-canon hashes
						trace!(target:"sync", "Returning single header: {:?}", hash);
						let mut rlp = RlpStream::new_list(1);
						rlp.append_raw(&hdr.into_inner(), 1);
						return Ok(Some((BLOCK_HEADERS_PACKET, rlp)));
					}
					number
				}
				None => return Ok(Some((BLOCK_HEADERS_PACKET, RlpStream::new_list(0)))) //no such header, return nothing
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
			} else if let Some(hdr) = io.ledger().block_header(BlockId::Number(number)) {
				data.append(&mut hdr.into_inner());
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
		let mut rlp = RlpStream::new_list(count as usize);
		rlp.append_raw(&data, count as usize);
		trace!(target: "sync", "{} -> GetBlockHeaders: returned {} entries", peer_id, count);
		Ok(Some((BLOCK_HEADERS_PACKET, rlp)))
    }

    /// Handle incoming packet from peer which does not require response
    pub fn on_packet(sync: &mut DagSync, io: &mut SyncIo, peer: PeerId, packet_id: PacketId, data: &[u8]) {
    }

    /// Called when a new peer is connected
    pub fn on_peer_connected(sync: &mut DagSync, io: &mut SyncIo, peer: PeerId) {
        trace!(target: "sync", "== Connected {}", peer);
        if let Err(e) = sync.send_status(io, peer) {
            debug!(target:"sync", "Error sending status request: {:?}", e);
            io.disconnect_peer(peer);
        } else {
            sync.handshaking_peers.insert(peer, Instant::now());
        }        
    }
}

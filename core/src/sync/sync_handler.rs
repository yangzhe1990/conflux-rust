use super::{
    PacketDecodeError, PacketId, PeerAsking, PeerInfo, RlpResponseResult,
    SyncState, BLOCK_BODIES_PACKET, BLOCK_HEADERS_PACKET,
    GET_BLOCK_BODIES_PACKET, GET_BLOCK_HEADERS_PACKET, MAX_BODIES_TO_SEND,
    MAX_HEADERS_TO_SEND, NEW_BLOCK_PACKET, STATUS_PACKET,
};

use super::super::block::Block;
use super::super::header::Header;
use super::sync_requester::SyncRequester;
use block_sync::BlockSyncError;
use bytes::Bytes;
use ethereum_types::{H256, U256};
use network::{Error, PeerId};
use parking_lot::RwLock;
use rlp::{Rlp, RlpStream};
use std::cmp;
use std::collections::VecDeque;
use std::time::Instant;
use sync_ctx::SyncContext;
use types::*;

/// SyncHandler is a collection of functions which handles incoming requests from peers from
/// SyncState
pub struct SyncHandler;

impl SyncHandler {
    /// Dispatch incoming requests and responses
    pub fn dispatch_packet(
        sync: &RwLock<SyncState>, io: &mut SyncContext, peer: PeerId,
        packet_id: PacketId, rlp: Rlp,
    )
    {
        trace!(target: "sync", "dispatch packet, packet_id {:}", packet_id);
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
        if let Err(e) = result {
            warn!("{:?}", e);
        }
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
        let mut count = r.item_count().unwrap_or(0);

        if count == 1 {
            debug!(target: "sync", "Empty GetBlockBodies request, ignoring.");
            let mut rlp = RlpStream::new_list(1);
            rlp.append(&(BLOCK_BODIES_PACKET as u32));
            return Ok(Some(rlp)); //no such header, return nothing
        }

        count = cmp::min(count, MAX_BODIES_TO_SEND + 1);
        let mut added = 0usize;
        let mut data = Bytes::new();
        for i in 1..count {
            if let Some(body) =
                io.ledger().block_body(BlockId::Hash(r.val_at::<H256>(i)?))
            {
                data.append(&mut body.rlp());
                added += 1;
            }
        }
        let mut rlp = RlpStream::new_list(added + 1);
        rlp.append(&(BLOCK_BODIES_PACKET as u32));
        rlp.append_raw(&data, added);
        trace!(target: "sync", "{} -> GetBlockBodies: returned {} entries", peer_id, added);
        Ok(Some(rlp))
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
            let number = r.val_at::<BlockNumber>(1)?;
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

        while number <= last && count < max_count {
            if let Some(hdr) = io.ledger().block_header(BlockId::Number(number))
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
            BLOCK_HEADERS_PACKET => {
                SyncHandler::on_peer_block_headers(sync, io, peer, rlp)
            }
            BLOCK_BODIES_PACKET => {
                SyncHandler::on_peer_block_bodies(sync, io, peer, rlp)
            }
            NEW_BLOCK_PACKET => {
                SyncHandler::on_peer_new_block(sync, io, peer, rlp)
            }
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

    fn on_peer_block_headers(
        sync: &mut SyncState, io: &mut SyncContext, peer_id: PeerId, r: &Rlp,
    ) -> Result<(), BlockSyncError> {
        let item_count = r.item_count()?;
        if item_count < 1 {
            return Err(BlockSyncError::Invalid);
        } else if item_count == 1 {
            return Ok(());
        }

        // verify the headers are organized in chain
        let mut parent_hash: H256 = H256::new();
        let mut headers: Vec<Header> = Vec::new();
        for i in 1..item_count {
            let header: Header = r.val_at(i).map_err(|e| {
               trace!(target: "sync", "Error decoding block header RLP: {:?}", e);
               BlockSyncError::Invalid
            })?;

            if !(i == 1) {
                if !(parent_hash == header.hash()) {
                    return Err(BlockSyncError::Invalid);
                }
            }
            parent_hash = *header.parent_hash();
            headers.push(header);
        }

        let mut body_hashes: Vec<H256> = Vec::new();
        for header in headers {
            let header_hash = header.hash();
            if io.ledger().block_header_exists(&header_hash) {
                break;
            } else {
                io.ledger().add_child(header.parent_hash(), &header_hash);
                body_hashes.push(header_hash);
                io.ledger().add_block_header_by_hash(&header_hash, header);
            }
        }

        if !io.ledger().block_header_exists(&parent_hash) {
            SyncRequester::request_headers_by_hash(
                sync,
                io,
                peer_id,
                &parent_hash,
                256,
                0,
                true,
            );
        }

        if !(body_hashes.is_empty()) {
            SyncRequester::fetch_block_bodies(sync, io, peer_id, body_hashes);
        }
        Ok(())
    }

    fn on_peer_block_bodies(
        _sync: &mut SyncState, io: &mut SyncContext, _peer_id: PeerId, r: &Rlp,
    ) -> Result<(), BlockSyncError> {
        let item_count = r.item_count()?;
        if item_count <= 1 {
            return Err(BlockSyncError::Useless);
        }

        let mut blocks: Vec<Block> = Vec::new();
        for i in 1..item_count {
            let body = r.val_at(i).map_err(|e| {
				trace!(target: "sync", "Error decoding block boides RLP: {:?}", e);
			    BlockSyncError::Invalid
			})?;
            blocks.push(body);
        }

        let mut blocks_to_adjust: VecDeque<H256> = VecDeque::new();
        let mut new_body_arrived = false;
        for body in blocks {
            let block_hash = body.hash();
            if let Some(_header) =
                io.ledger().block_header(BlockId::Hash(block_hash))
            {
                io.ledger().add_block_body_by_hash(&block_hash, body);
                blocks_to_adjust.push_back(block_hash);
                new_body_arrived = true;
            } else {
                debug!(target: "sync", "Skip the body without header: {:?}", block_hash);
            }
        }

        if new_body_arrived {
            let adjusted = io.ledger().adjust_main_chain(blocks_to_adjust);
            if adjusted {
                // TODO: trigger tx execution
            }
        }

        Ok(())
    }

    fn on_peer_new_block(
        sync: &mut SyncState, io: &mut SyncContext, peer_id: PeerId, r: &Rlp,
    ) -> Result<(), BlockSyncError> {
        let new_block_total_difficulty: U256 = r.val_at(1)?;
        let new_header: Header = r.val_at(2)?;
        let new_body: Block = r.val_at(3)?;

        if new_header.hash() != new_body.hash() {
            debug!(target: "sync", "hashes in header and body do not match!");
            return Err(BlockSyncError::Invalid);
        }

        let hash = new_header.hash();
        let parent_hash = *(new_header.parent_hash());

        let mut new_block_arrived = false;
        if !io.ledger().block_header_exists(&hash) {
            new_block_arrived = true;
            io.ledger().add_block_header_by_hash(&hash, new_header);
        }

        io.ledger().add_child(&parent_hash, &hash);

        if !io.ledger().block_body_exists(&hash) {
            new_block_arrived = true;
            io.ledger().add_block_body_by_hash(&hash, new_body);
        }

        if !io.ledger().block_header_exists(&parent_hash) {
            SyncRequester::request_headers_by_hash(
                sync,
                io,
                peer_id,
                &parent_hash,
                256,
                0,
                true,
            );
        }

        let mut blocks_to_adjust: VecDeque<H256> = VecDeque::new();
        blocks_to_adjust.push_back(hash);

        if new_block_arrived {
            // relay to peers
            let mut hashes: Vec<H256> = Vec::new();
            hashes.push(hash);
            let mut total_difficulties: Vec<U256> = Vec::new();
            total_difficulties.push(new_block_total_difficulty);
            sync.new_chain_blocks(io, &hashes[..], &total_difficulties[..]);

            // adjust main chain
            let adjusted = io.ledger().adjust_main_chain(blocks_to_adjust);
            if adjusted {
                // TODO: trigger tx execution
            }
        }

        Ok(())
    }
}

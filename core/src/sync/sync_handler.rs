use super::sync_requester::SyncRequester;
use super::{
    PeerAsking, PeerInfo, RlpResponse, SyncState, BLOCK_BODIES_PACKET,
    BLOCK_HEADERS_PACKET, GET_BLOCK_BODIES_PACKET, GET_BLOCK_HEADERS_PACKET,
    MAX_BODIES_TO_SEND, MAX_HEADERS_TO_SEND, NEW_BLOCK_PACKET, STATUS_PACKET,
};
use block_sync::BlockSyncError;
use bytes::Bytes;
use ethereum_types::{H256, U256};
use message::{
    BlockBodies, BlockBody as WireBlockBody, BlockHeaders,
    BlockId as WireBlockId, GetBlockBodies, GetBlockHeaders, Message, MsgId,
    NewBlock, Status,
};
use network::{Error, PeerId};
use parking_lot::RwLock;
use primitives::*;
use rlp::{DecoderError, Rlp, RlpStream};
use std::cmp;
use std::collections::VecDeque;
use std::time::Instant;
use sync_ctx::SyncContext;

/// SyncHandler is a collection of functions which handles incoming requests from peers from
/// SyncState
pub struct SyncHandler;

impl SyncHandler {
    /// Dispatch incoming requests and responses
    pub fn dispatch_message(
        sync: &RwLock<SyncState>, io: &mut SyncContext, peer: PeerId,
        msg_id: MsgId, rlp: Rlp,
    )
    {
        trace!(target: "sync", "dispatch message, msgId {:}", msg_id);
        let result = match msg_id {
            MsgId::GET_BLOCK_BODIES => SyncHandler::return_rlp(
                io,
                &rlp,
                peer,
                SyncHandler::get_block_bodies,
                |e| format!("Error sending block bodies: {:?}", e),
            ),
            MsgId::GET_BLOCK_HEADERS => SyncHandler::return_rlp(
                io,
                &rlp,
                peer,
                SyncHandler::get_block_headers,
                |e| format!("Error sending block headers: {:?}", e),
            ),
            _ => {
                sync.write().on_message(io, peer, msg_id, &rlp);
                Ok(())
            }
        };
        if let Err(e) = result {
            warn!("{:?}", e);
        }
    }

    fn return_rlp<FRlp, FError>(
        io: &mut SyncContext, rlp: &Rlp, peer: PeerId, rlp_fn: FRlp,
        error_fn: FError,
    ) -> Result<(), DecoderError>
    where
        FRlp: Fn(&SyncContext, &Rlp, PeerId) -> RlpResponse,
        FError: FnOnce(Error) -> String,
    {
        let response = rlp_fn(io, rlp, peer);
        match response {
            Err(e) => Err(e),
            Ok(Some(msg)) => {
                let mut data = Bytes::new();
                data.push(msg.msg_id().into());
                data.extend_from_slice(&msg.rlp_bytes());
                io.send(peer, data).unwrap_or_else(
                    |e| debug!(target: "sync", "{:?}", error_fn(e)),
                );
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Respond to GetBlockBodies request
    fn get_block_bodies(
        io: &SyncContext, rlp: &Rlp, peer_id: PeerId,
    ) -> RlpResponse {
        let req = rlp.as_val::<GetBlockBodies>()?;

        if req.hashes.is_empty() {
            debug!(target: "sync", "Empty GetBlockBodies request, ignoring.");
            return Ok(Some(Box::new(BlockBodies::default()))); //no such header, return nothing
        }

        let block_bodies = BlockBodies {
            bodies: req
                .hashes
                .iter()
                .take(MAX_BODIES_TO_SEND)
                .map(|hash| io.ledger().block_body(BlockId::Hash(*hash)))
                .collect(),
        };
        Ok(Some(Box::new(block_bodies)))
    }

    /// Respond to GetBlockHeaders request
    fn get_block_headers(
        io: &SyncContext, rlp: &Rlp, peer_id: PeerId,
    ) -> RlpResponse {
        let req = rlp.as_val::<GetBlockHeaders>()?;

        let number = match req.block_id {
            WireBlockId::Hash(hash) => {
                trace!(target: "sync",
                       "{} -> GetBlockHeaders (hash: {}, max: {}, skip: {}, reverse:{})",
                       peer_id, hash, req.max_headers, req.skip, req.reverse);
                match io.ledger().block_header(BlockId::Hash(hash)) {
                    Some(header) => {
                        let number = header.number().into();
                        debug_assert_eq!(header.hash(), hash);

                        if req.max_headers == 1
                            || io.ledger().block_hash(BlockId::Number(number))
                                != Some(hash)
                        {
                            // Non canonical header or single header requested
                            // TODO: handle single-step reverse hashchains of non-canon hashes
                            trace!(target: "sync", "Returning single header: {:?}", hash);
                            return Ok(Some(Box::new(BlockHeaders {
                                headers: vec![header.clone()],
                            })));
                        }
                        number
                    }
                    None => {
                        return Ok(Some(Box::new(BlockHeaders::default())));
                    }
                }
            }
            WireBlockId::Number(number) => {
                trace!(target: "sync", "{} -> GetBlockHeaders (number: {}, max: {}, skip: {}, reverse:{})", peer_id, number, req.max_headers, req.skip, req.reverse);
                number
            }
        };

        let last = io.ledger().ledger_info().best_block_number;
        let mut number = if req.reverse {
            cmp::min(last, number)
        } else {
            cmp::max(0, number)
        };
        let max_count = cmp::min(MAX_HEADERS_TO_SEND, req.max_headers);
        let mut count = 0;
        let inc = req.skip.saturating_add(1) as BlockNumber;
        let mut block_headers = BlockHeaders::default();

        while number <= last && count < max_count {
            if let Some(header) =
                io.ledger().block_header(BlockId::Number(number))
            {
                block_headers.headers.push(header.clone());
                count += 1;
            } else {
                // No required block.
                break;
            }
            if req.reverse {
                if number <= inc || number == 0 {
                    break;
                }
                number = number.saturating_sub(inc);
            } else {
                number = number.saturating_add(inc);
            }
        }
        trace!(target: "sync", "{} -> GetBlockHeaders: returned {} entries", peer_id, count);
        Ok(Some(Box::new(block_headers)))
    }

    /// Handle incoming packet from peer which does not require response
    pub fn on_message(
        sync: &mut SyncState, io: &mut SyncContext, peer: PeerId,
        msg_id: MsgId, rlp: &Rlp,
    )
    {
        if msg_id != MsgId::STATUS && !sync.peers.contains_key(&peer) {
            debug!(target: "sync", "Unexpected message {} from unregistered peer: {}", msg_id, peer);
            return;
        }

        let result = match msg_id {
            MsgId::STATUS => SyncHandler::on_peer_status(sync, io, peer, rlp),
            MsgId::BLOCK_HEADERS => {
                SyncHandler::on_peer_block_headers(sync, io, peer, rlp)
            }
            MsgId::BLOCK_BODIES => {
                SyncHandler::on_peer_block_bodies(sync, io, peer, rlp)
            }
            MsgId::NEW_BLOCK => {
                SyncHandler::on_peer_new_block(sync, io, peer, rlp)
            }
            _ => {
                debug!(target: "sync", "{}: Unknown message {}", peer, msg_id);
                Ok(())
            }
        };

        match result {
            Err(BlockSyncError::Invalid) => {
                debug!(target: "sync", "{} -> Invalid message {}", peer, msg_id);
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
            debug!(target: "sync", "Error sending status request: {:?}", e);
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

        let status = r.as_val::<Status>()?;

        let protocol_version = status.protocol_version;
        let peer = PeerInfo {
            protocol_version: protocol_version,
            difficulty: Some(status.total_difficulty),
            latest_hash: status.best_hash,
            genesis: status.genesis_hash,
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
        let block_headers = r.as_val::<BlockHeaders>()?;
        if block_headers.headers.is_empty() {
            return Ok(());
        }

        // verify the headers are organized in chain
        let mut parent_hash: H256 = H256::new();
        for i in 0..block_headers.headers.len() {
            let header = &(block_headers.headers[i]);
            if i > 0 {
                if parent_hash != header.hash() {
                    return Err(BlockSyncError::Invalid);
                }
            }
            parent_hash = *header.parent_hash();
        }

        let mut body_hashes: Vec<H256> = Vec::new();
        for header in block_headers.headers {
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
        let block_bodies = r.as_val::<BlockBodies>()?;
        if block_bodies.bodies.is_empty() {
            return Err(BlockSyncError::Useless);
        }

        let blocks: Vec<Block> = block_bodies
            .bodies
            .iter()
            .filter(|block| block.is_some())
            .map(|block| block.clone().unwrap())
            .collect();

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
                let block_number = io.ledger().best_block_number();
                io.execution_engine().execute_up_to(block_number);
            }
        }

        Ok(())
    }

    fn on_peer_new_block(
        sync: &mut SyncState, io: &mut SyncContext, peer_id: PeerId, r: &Rlp,
    ) -> Result<(), BlockSyncError> {
        let new_block = r.as_val::<NewBlock>()?;

        let new_block_total_difficulty = new_block.total_difficulty;
        let new_header = new_block.header;
        let new_body = new_block.body;
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
                let block_number = io.ledger().best_block_number();
                io.execution_engine().execute_up_to(block_number);
            }
        }

        Ok(())
    }
}

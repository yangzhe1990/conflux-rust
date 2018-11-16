use super::{
    Error, ErrorKind, SynchronizationGraph, SynchronizationPeerState,
    SynchronizationState, MAX_INFLIGHT_REQUEST_COUNT,
};
use bytes::Bytes;
use consensus::SharedConsensusGraph;
use ethereum_types::H256;
use io::TimerToken;
use message::{
    GetBlockHeaders, GetBlockHeadersResponse, GetBlocks, GetBlocksResponse,
    GetTerminalBlockHashes, GetTerminalBlockHashesResponse, Message, MsgId,
    NewBlock, NewBlockHashes, Status,
};
use network::{
    Error as NetworkError, NetworkContext, NetworkProtocolHandler, PeerId,
};
use parking_lot::RwLock;
use primitives::Block;
use rlp::Rlp;
use slab::Slab;
use std::{
    cmp,
    collections::VecDeque,
    time::{Duration, Instant},
};

pub const SYNCHRONIZATION_PROTOCOL_VERSION: u8 = 0x01;

pub const MAX_HEADERS_TO_SEND: u64 = 512;
pub const MAX_BLOCKS_TO_SEND: u64 = 256;

const TX_TIMER: TimerToken = 0;

pub struct SynchronizationProtocolHandler {
    graph: SynchronizationGraph,
    consensus_graph: SharedConsensusGraph,
    syn: RwLock<SynchronizationState>,
}

impl SynchronizationProtocolHandler {
    pub fn new(consensus_graph: SharedConsensusGraph) -> Self {
        SynchronizationProtocolHandler {
            graph: SynchronizationGraph::new(consensus_graph.clone()),
            consensus_graph,
            syn: RwLock::new(SynchronizationState::new()),
        }
    }

    fn send_message(
        &self, io: &NetworkContext, peer: PeerId, msg: &Message,
    ) -> Result<(), NetworkError> {
        let mut raw = Bytes::new();
        raw.push(msg.msg_id().into());
        raw.extend(msg.rlp_bytes().iter());
        io.send(peer, raw).map_err(|e| {
            debug!(target: "sync", "Error sending message: {:?}", e);
            io.disconnect_peer(peer);
            e
        })
    }

    fn dispatch_message(
        &self, io: &NetworkContext, peer: PeerId, msg_id: MsgId, rlp: Rlp,
    ) {
        trace!(target: "sync", "Dispatching message: peer={:?}, msgid={:?}", peer, msg_id);
        let mut syn = self.syn.write();

        if msg_id != MsgId::STATUS && !syn.peers.contains_key(&peer) {
            debug!(target: "sync", "Unexpected message from unrecognized peer: peer={:?} msgid={:?}", peer, msg_id);
            return;
        }
        match msg_id {
            MsgId::STATUS => self.on_status(io, &mut *syn, peer, &rlp),
            MsgId::GET_BLOCK_HEADERS_RESPONSE => {
                self.on_block_headers_response(io, &mut *syn, peer, &rlp)
            }
            MsgId::GET_BLOCK_HEADERS => {
                self.on_get_block_headers(io, &mut *syn, peer, &rlp)
            }
            MsgId::NEW_BLOCK => self.on_new_block(io, &mut *syn, peer, &rlp),
            MsgId::NEW_BLOCK_HASHES => self.on_new_block_hashes(io, &mut *syn, peer, &rlp),
            MsgId::GET_BLOCKS_RESPONSE => self.on_blocks_response(io, &mut *syn, peer, &rlp),
            MsgId::GET_BLOCKS => self.on_get_blocks(io, &mut *syn, peer, &rlp),
            MsgId::GET_TERMINAL_BLOCK_HASHES_RESPONSE => {
                self.on_terminal_block_hashes_response(io, &mut *syn, peer, &rlp)
            }
            MsgId::GET_TERMINAL_BLOCK_HASHES => {
                self.on_get_terminal_block_hashes(io, &mut *syn, peer, &rlp)
            }
            _ => {
                debug!(target: "sync", "Unknown message: peer={:?} msgid={:?}", peer, msg_id);
                Ok(())
            }
        }.unwrap();
    }

    fn on_get_block_headers(
        &self, io: &NetworkContext, _syn: &mut SynchronizationState,
        peer: PeerId, rlp: &Rlp,
    ) -> Result<(), Error>
    {
        let req = rlp.as_val::<GetBlockHeaders>()?;
        trace!(target: "sync", "Peer {:?} requested {:?} block headers starting at {:?}", peer, req.max_blocks, req.hash);

        let mut hash = req.hash;
        let mut block_headers_resp = GetBlockHeadersResponse::default();
        block_headers_resp.reqid = req.reqid;

        for _n in 0..cmp::min(MAX_HEADERS_TO_SEND, req.max_blocks) {
            let header = self.graph.block_header_by_hash(&hash);
            if header.is_none() {
                break;
            }
            let header = header.unwrap();
            block_headers_resp.headers.push(header.clone());
            if hash == *self.graph.genesis_hash() {
                break;
            }
            hash = header.parent_hash().clone();
        }
        trace!(target: "sync", "Returned {:?} block headers to peer {:?}", block_headers_resp.headers.len(), peer);

        let msg: Box<dyn Message> = Box::new(block_headers_resp);
        self.send_message(io, peer, msg.as_ref())?;
        Ok(())
    }

    fn on_get_blocks(
        &self, io: &NetworkContext, _syn: &mut SynchronizationState,
        peer: PeerId, rlp: &Rlp,
    ) -> Result<(), Error>
    {
        let req = rlp.as_val::<GetBlocks>()?;
        if req.hashes.is_empty() {
            debug!(target: "sync", "Received empty getblockheaders message: peer={:?}", peer);
        } else {
            let msg: Box<dyn Message> = Box::new(GetBlocksResponse {
                reqid: req.reqid,
                blocks: req
                    .hashes
                    .iter()
                    .take(MAX_BLOCKS_TO_SEND as usize)
                    .filter_map(|hash| self.graph.block_by_hash(hash))
                    .collect(),
            });
            self.send_message(io, peer, msg.as_ref())?;
        }
        Ok(())
    }

    fn on_get_terminal_block_hashes(
        &self, io: &NetworkContext, _syn: &mut SynchronizationState,
        peer_id: PeerId, rlp: &Rlp,
    ) -> Result<(), Error>
    {
        let req = rlp.as_val::<GetTerminalBlockHashes>()?;
        let msg: Box<dyn Message> = Box::new(GetTerminalBlockHashesResponse {
            reqid: req.reqid,
            hashes: self.consensus_graph.terminal_block_hashes(),
        });
        self.send_message(io, peer_id, msg.as_ref())?;
        Ok(())
    }

    fn on_terminal_block_hashes_response(
        &self, io: &NetworkContext, syn: &mut SynchronizationState,
        peer_id: PeerId, rlp: &Rlp,
    ) -> Result<(), Error>
    {
        let terminal_block_hashes =
            rlp.as_val::<GetTerminalBlockHashesResponse>()?;
        self.match_request(io, syn, peer_id, terminal_block_hashes.reqid)?;

        for hash in &terminal_block_hashes.hashes {
            if !self.graph.contains_block_header(&hash) {
                self.request_block_headers(io, syn, peer_id, hash, 256);
            }
        }
        Ok(())
    }

    fn on_status(
        &self, _io: &NetworkContext, syn: &mut SynchronizationState,
        peer_id: PeerId, rlp: &Rlp,
    ) -> Result<(), Error>
    {
        if !syn.handshaking_peers.contains_key(&peer_id)
            || syn.peers.contains_key(&peer_id)
        {
            debug!(target: "sync", "Unexpected status message: peer={:?}", peer_id);
        }
        syn.handshaking_peers.remove(&peer_id);

        let status = rlp.as_val::<Status>()?;
        let peer = SynchronizationPeerState {
            id: peer_id,
            protocol_version: status.protocol_version,
            genesis_hash: status.genesis_hash,
            inflight_requests: Slab::with_capacity(
                MAX_INFLIGHT_REQUEST_COUNT as usize,
            ),
            pending_requests: VecDeque::new(),
        };

        trace!(target: "sync", "New peer (pv={:?}, gh={:?})",
               status.protocol_version, status.genesis_hash);

        let genesis_hash = self.graph.genesis_hash();
        if *genesis_hash != status.genesis_hash {
            debug!(target: "sync", "Peer {:?} genesis hash mismatches (ours: {:?}, theirs: {:?})", peer_id, genesis_hash, status.genesis_hash);
            return Err(ErrorKind::Invalid.into());
        }

        trace!(target: "sync", "Peer {:?} connected", peer_id);
        syn.peers.insert(peer_id.clone(), peer);

        Ok(())
    }

    fn on_block_headers_response(
        &self, io: &NetworkContext, syn: &mut SynchronizationState,
        peer_id: PeerId, rlp: &Rlp,
    ) -> Result<(), Error>
    {
        let block_headers = rlp.as_val::<GetBlockHeadersResponse>()?;
        self.match_request(io, syn, peer_id, block_headers.reqid)?;

        if block_headers.headers.is_empty() {
            trace!(target: "sync", "Received empty GetBlockHeadersResponse message");
            return Ok(());
        }

        let mut parent_hash = H256::default();
        for header in &block_headers.headers {
            let hash = header.hash();
            if parent_hash != H256::default() && parent_hash != hash {
                return Err(ErrorKind::Invalid.into());
            }
            parent_hash = header.parent_hash().clone();
        }

        let mut hashes = Vec::default();

        for header in &block_headers.headers {
            let hash = header.hash();
            if !self.graph.contains_block_header(&hash) {
                self.graph.insert_block_header(header.clone());
                hashes.push(hash);
            } else if !self.graph.contains_block(&hash) {
                hashes.push(hash);
            }
        }
        let header_hashes: Vec<H256> = block_headers
            .headers
            .iter()
            .map(|header| header.hash())
            .collect();
        trace!(target:"sync", "get headers responce of hashes:{:?}, requesting block:{:?}", header_hashes, hashes);

        if parent_hash != H256::default()
            && !self.graph.contains_block_header(&parent_hash)
        {
            self.request_block_headers(io, syn, peer_id, &parent_hash, 256);
        }
        if !hashes.is_empty() {
            self.request_blocks(io, syn, peer_id, hashes);
        }

        Ok(())
    }

    fn on_blocks_response(
        &self, io: &NetworkContext, syn: &mut SynchronizationState,
        peer_id: PeerId, rlp: &Rlp,
    ) -> Result<(), Error>
    {
        let blocks = rlp.as_val::<GetBlocksResponse>()?;
        self.match_request(io, syn, peer_id, blocks.reqid)?;

        let new_block_hashes: Vec<H256> = blocks
            .blocks
            .into_iter()
            .map(|block| {
                let hash = block.hash();

                if !self.graph.contains_block_header(&hash) {
                    self.graph.insert_block_header(block.block_header.clone());
                }
                if !self.graph.contains_block(&hash) {
                    self.graph.insert_block(block);
                    Some(hash)
                } else {
                    None
                }
            })
            .filter_map(|h| h)
            .collect();

        trace!(target:"sync", "receive new blocks:{:?}", new_block_hashes);

        if !new_block_hashes.is_empty() {
            let new_block_hash_msg: Box<dyn Message> =
                Box::new(NewBlockHashes {
                    block_hashes: new_block_hashes,
                });
            self.broadcast_message(
                io,
                syn,
                peer_id,
                new_block_hash_msg.as_ref(),
            )?;
        }

        Ok(())
    }

    pub fn on_mined_block(&self, block: Block) {
        let hash = block.block_header.hash();
        let parent_hash = block.block_header.parent_hash();

        assert!(self.graph.contains_block_header(parent_hash));

        assert!(!self.graph.contains_block_header(&hash));
        self.graph.insert_block_header(block.block_header.clone());

        assert!(!self.graph.contains_block(&hash));
        self.graph.insert_block(block.clone());
    }

    fn on_new_block(
        &self, io: &NetworkContext, syn: &mut SynchronizationState,
        peer_id: PeerId, rlp: &Rlp,
    ) -> Result<(), Error>
    {
        let new_block = rlp.as_val::<NewBlock>()?;
        let hash = new_block.block.block_header.hash();
        let parent_hash = new_block.block.block_header.parent_hash();

        if !self.graph.contains_block_header(&hash) {
            self.graph
                .insert_block_header(new_block.block.block_header.clone());
        }

        let is_new = if !self.graph.contains_block(&hash) {
            self.graph.insert_block(new_block.block.clone());
            true
        } else {
            false
        };

        if !self.graph.contains_block_header(parent_hash) {
            self.request_block_headers(io, syn, peer_id, parent_hash, 256);
        }

        if is_new {
            // broadcast the hash of the newly got block
            let mut block_hashes: Vec<H256> = Vec::new();
            block_hashes.push(hash);
            let new_block_hash_msg: Box<dyn Message> =
                Box::new(NewBlockHashes { block_hashes });
            self.broadcast_message(
                io,
                syn,
                peer_id,
                new_block_hash_msg.as_ref(),
            )?;
        }

        Ok(())
    }

    fn on_new_block_hashes(
        &self, io: &NetworkContext, syn: &mut SynchronizationState,
        peer_id: PeerId, rlp: &Rlp,
    ) -> Result<(), Error>
    {
        let new_block_hashes = rlp.as_val::<NewBlockHashes>()?;

        for hash in new_block_hashes.block_hashes.iter() {
            if !self.graph.contains_block_header(hash) {
                self.request_block_headers(io, syn, peer_id, hash, 256);
                // FIXME: This might need better design.
                // The current rationale is that newblockhashes message
                // are only produced in 2 cases.
                // 1. After seeing a new block, the newblockhashes only
                // contains 1 hash value of the new block;
                // 2. After getting a series of blocks from getblockresponse
                // message, those blocks are in a same parental chain and in
                // reverse order, so we only need to fetch the first header
                // backward following the parental chain.
                break;
            }
        }
        Ok(())
    }

    fn broadcast_message(
        &self, io: &NetworkContext, syn: &mut SynchronizationState,
        skip_id: PeerId, msg: &Message,
    ) -> Result<(), NetworkError>
    {
        for (id, _) in syn.peers.iter() {
            if *id != skip_id {
                self.send_message(io, *id, msg)?;
            }
        }
        Ok(())
    }

    fn send_status(
        &self, io: &NetworkContext, peer: PeerId,
    ) -> Result<(), NetworkError> {
        trace!(target: "sync", "Sending status message to {:?}", peer);

        let msg: Box<dyn Message> = Box::new(Status {
            protocol_version: SYNCHRONIZATION_PROTOCOL_VERSION,
            network_id: 0x0,
            genesis_hash: *self.graph.genesis_hash(),
            best_epoch_hash: H256::default(),
        });
        self.send_message(io, peer, msg.as_ref())
    }

    fn request_block_headers(
        &self, io: &NetworkContext, syn: &mut SynchronizationState,
        peer_id: PeerId, hash: &H256, max_blocks: u64,
    )
    {
        trace!(target: "sync", "Requesting {:?} block headers starting at {:?} from peer {:?}", max_blocks, hash, peer_id);

        self.send_request(
            io,
            syn,
            peer_id,
            Box::new(GetBlockHeaders {
                reqid: 0,
                hash: *hash,
                max_blocks,
            }),
        );
    }

    fn request_blocks(
        &self, io: &NetworkContext, syn: &mut SynchronizationState,
        peer_id: PeerId, hashes: Vec<H256>,
    )
    {
        trace!(target: "sync", "Requesting {:?} blocks from {:?}", hashes.len(), peer_id);

        self.send_request(
            io,
            syn,
            peer_id,
            Box::new(GetBlocks { reqid: 0, hashes }),
        );
    }

    fn send_request(
        &self, io: &NetworkContext, syn: &mut SynchronizationState,
        peer_id: PeerId, mut msg: Box<dyn Message>,
    )
    {
        if let Some(ref mut peer) = syn.peers.get_mut(&peer_id) {
            if let Some(reqid) = peer.next_request_id() {
                msg.set_request_id(reqid as u16);
                self.send_message(io, peer_id, msg.as_ref()).unwrap();
                peer.append_inflight_request(reqid, msg);
            } else {
                peer.append_pending_request(msg);
            }
        }
    }

    fn match_request(
        &self, io: &NetworkContext, syn: &mut SynchronizationState,
        peer_id: PeerId, request_id: u16,
    ) -> Result<(), Error>
    {
        if let Some(ref mut peer) = syn.peers.get_mut(&peer_id) {
            let reqid = request_id as usize;
            if peer.is_inflight_request(reqid) {
                if peer.has_pending_requests() {
                    let pending_req = peer.pop_pending_request().unwrap();
                    let mut pending_msg = pending_req.message;
                    pending_msg.set_request_id(request_id);
                    self.send_message(io, peer_id, pending_msg.as_ref())
                        .unwrap();
                    peer.append_inflight_request(reqid, pending_msg);
                } else {
                    peer.remove_inflight_request(reqid);
                }
                Ok(())
            } else {
                Err(ErrorKind::UnexpectedResponse.into())
            }
        } else {
            Err(ErrorKind::UnknownPeer.into())
        }
    }

    pub fn announce_new_blocks(&self, io: &NetworkContext, hashes: &[H256]) {
        let syn = self.syn.write();
        for hash in hashes {
            let block = self.graph.block_by_hash(hash).unwrap();
            let msg: Box<dyn Message> = Box::new(NewBlock { block });
            for (id, _) in syn.peers.iter() {
                self.send_message(io, *id, msg.as_ref())
                    .expect("Error sending new blocks!");
            }
        }
    }

    pub fn propagate_new_transactions(&self, io: &NetworkContext) {
        let syn = self.syn.write();

        if syn.peers.is_empty() {
            return;
        }
    }
}

impl NetworkProtocolHandler for SynchronizationProtocolHandler {
    fn initialize(&self, io: &NetworkContext) {
        io.register_timer(TX_TIMER, Duration::from_millis(1300))
            .expect("Error registering transactions timer");
    }

    fn on_message(&self, io: &NetworkContext, peer: PeerId, raw: &[u8]) {
        let msg_id = raw[0];
        let rlp = Rlp::new(&raw[1..]);
        debug!(target: "sync", "on_message: peer={:?}, msgid={:?}", peer, msg_id);
        self.dispatch_message(io, peer, msg_id.into(), rlp);
    }

    fn on_peer_connected(&self, io: &NetworkContext, peer: PeerId) {
        let mut syn = self.syn.write();

        trace!(target: "sync", "Peer connected: peer={:?}", peer);
        if let Err(e) = self.send_status(io, peer) {
            debug!(target: "sync", "Error sending status message: {:?}", e);
            io.disconnect_peer(peer);
        } else {
            syn.handshaking_peers.insert(peer, Instant::now());
        }
    }

    fn on_peer_disconnected(&self, _io: &NetworkContext, peer: PeerId) {
        trace!(target: "sync", "Peer disconnected: peer={:?}", peer);
    }

    fn on_timeout(&self, io: &NetworkContext, timer: TimerToken) {
        trace!(target: "sync", "Timeout: timer={:?}", timer);

        match timer {
            TX_TIMER => {
                self.propagate_new_transactions(io);
            }
            _ => warn!("Unknown timer {} triggered.", timer),
        }
    }
}

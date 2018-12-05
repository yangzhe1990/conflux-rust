use super::{
    super::transaction_pool::SharedTransactionPool, random, Error, ErrorKind,
    SynchronizationGraph, SynchronizationPeerState, SynchronizationState,
    MAX_INFLIGHT_REQUEST_COUNT,
};
use bytes::Bytes;
use consensus::SharedConsensusGraph;
use ethereum_types::H256;
use io::TimerToken;
use message::{
    GetBlockHeaders, GetBlockHeadersResponse, GetBlocks, GetBlocksResponse,
    GetTerminalBlockHashes, GetTerminalBlockHashesResponse, Message, MsgId,
    NewBlock, NewBlockHashes, Status, Transactions,
};
use network::{
    Error as NetworkError, NetworkContext, NetworkProtocolHandler, PeerId,
};
use parking_lot::{Mutex, RwLock};
use primitives::{Block, SignedTransaction};
use rand::RngCore;
use rlp::Rlp;
use slab::Slab;
use std::{
    cmp::{self, Ordering},
    collections::{BinaryHeap, HashMap, HashSet, VecDeque},
    sync::{
        atomic::{AtomicBool, Ordering as AtomicOrdering},
        Arc,
    },
    time::{Duration, Instant},
};
use sync::synchronization_state::RequestMessage;

pub const SYNCHRONIZATION_PROTOCOL_VERSION: u8 = 0x01;

pub const MAX_HEADERS_TO_SEND: u64 = 512;
pub const MAX_BLOCKS_TO_SEND: u64 = 256;
const MIN_PEERS_PROPAGATION: usize = 4;
const MAX_PEERS_PROPAGATION: usize = 128;
const HEADERS_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const BLOCKS_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_GET_HEADERS_NUM: u64 = 1;

const TX_TIMER: TimerToken = 0;
const CHECK_REQUEST_TIMER: TimerToken = 1;

pub struct SynchronizationProtocolHandler {
    graph: SynchronizationGraph,
    syn: RwLock<SynchronizationState>,
    headers_in_flight: Mutex<HashSet<H256>>,
    blocks_in_flight: Mutex<HashSet<H256>>,
    requests_queue: Mutex<BinaryHeap<Arc<TimedSyncRequests>>>,
}

pub struct TimedSyncRequests {
    pub peer_id: PeerId,
    pub timeout_time: Instant,
    pub request_id: usize,
    pub removed: AtomicBool,
}

impl TimedSyncRequests {
    pub fn new(
        peer_id: PeerId, timeout: Duration, request_id: usize,
    ) -> TimedSyncRequests {
        TimedSyncRequests {
            peer_id,
            timeout_time: Instant::now() + timeout,
            request_id,
            removed: AtomicBool::new(false),
        }
    }

    pub fn from_request(
        peer_id: PeerId, request_id: usize, msg: &RequestMessage,
    ) -> TimedSyncRequests {
        let timeout = match *msg {
            RequestMessage::Headers(_) => HEADERS_REQUEST_TIMEOUT,
            RequestMessage::Blocks(_) => BLOCKS_REQUEST_TIMEOUT,
            _ => Duration::default(),
        };
        TimedSyncRequests::new(peer_id, timeout, request_id)
    }
}

impl Ord for TimedSyncRequests {
    fn cmp(&self, other: &Self) -> Ordering {
        other.timeout_time.cmp(&self.timeout_time)
    }
}
impl PartialOrd for TimedSyncRequests {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        other.timeout_time.partial_cmp(&self.timeout_time)
    }
}
impl Eq for TimedSyncRequests {}
impl PartialEq for TimedSyncRequests {
    fn eq(&self, other: &Self) -> bool {
        self.timeout_time == other.timeout_time
    }
}

impl SynchronizationProtocolHandler {
    pub fn new(consensus_graph: SharedConsensusGraph) -> Self {
        SynchronizationProtocolHandler {
            graph: SynchronizationGraph::new(consensus_graph.clone()),
            syn: RwLock::new(SynchronizationState::new()),
            headers_in_flight: Mutex::new(HashSet::new()),
            blocks_in_flight: Mutex::new(HashSet::new()),
            requests_queue: Mutex::new(BinaryHeap::new()),
        }
    }

    pub fn block_by_hash(&self, hash: &H256) -> Option<Block> {
        self.graph.block_by_hash(hash)
    }

    fn send_message(
        &self, io: &NetworkContext, peer: PeerId, msg: &Message,
    ) -> Result<(), NetworkError> {
        let mut raw = Bytes::new();
        raw.push(msg.msg_id().into());
        raw.extend(msg.rlp_bytes().iter());
        io.send(peer, raw).map_err(|e| {
            debug!("Error sending message: {:?}", e);
            io.disconnect_peer(peer);
            e
        });
        // FIXME return error after we implement error handling
        Ok(())
    }

    fn dispatch_message(
        &self, io: &NetworkContext, peer: PeerId, msg_id: MsgId, rlp: Rlp,
    ) {
        trace!("Dispatching message: peer={:?}, msgid={:?}", peer, msg_id);
        let mut syn = self.syn.write();

        if msg_id != MsgId::STATUS && !syn.peers.contains_key(&peer) {
            warn!("Unexpected message from unrecognized peer: peer={:?} msgid={:?}", peer, msg_id);
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
            MsgId::NEW_BLOCK_HASHES => {
                self.on_new_block_hashes(io, &mut *syn, peer, &rlp)
            }
            MsgId::GET_BLOCKS_RESPONSE => {
                self.on_blocks_response(io, &mut *syn, peer, &rlp)
            }
            MsgId::GET_BLOCKS => self.on_get_blocks(io, &mut *syn, peer, &rlp),
            MsgId::GET_TERMINAL_BLOCK_HASHES_RESPONSE => self
                .on_terminal_block_hashes_response(io, &mut *syn, peer, &rlp),
            MsgId::GET_TERMINAL_BLOCK_HASHES => {
                self.on_get_terminal_block_hashes(io, &mut *syn, peer, &rlp)
            }
            MsgId::TRANSACTIONS => {
                self.on_transactions(io, &mut *syn, peer, &rlp)
            }
            _ => {
                warn!("Unknown message: peer={:?} msgid={:?}", peer, msg_id);
                Ok(())
            }
        }
        .unwrap_or_else(|e| {
            warn!(
                "Error while handling message msgid={:?}, error={:?}",
                msg_id, e
            );
        });
    }

    fn on_transactions(
        &self, _io: &NetworkContext, syn: &mut SynchronizationState,
        peer_id: PeerId, rlp: &Rlp,
    ) -> Result<(), Error>
    {
        let transactions = rlp.as_val::<Transactions>()?;
        let transactions = transactions.transactions;
        trace!(
            "Received {:?} transactions from Peer {:?}",
            transactions.len(),
            peer_id
        );
        if let Some(peer_info) = syn.peers.get_mut(&peer_id) {
            peer_info
                .last_sent_transactions
                .extend(transactions.iter().map(|tx| tx.hash()));
        }
        self.get_transaction_pool().insert_new_transactions(
            self.graph.consensus.best_block_hash(),
            transactions,
        );
        trace!("Transactions successfully inserted to transaction pool");
        Ok(())
    }

    fn on_get_block_headers(
        &self, io: &NetworkContext, _syn: &mut SynchronizationState,
        peer: PeerId, rlp: &Rlp,
    ) -> Result<(), Error>
    {
        let req = rlp.as_val::<GetBlockHeaders>()?;
        trace!("on_get_block_headers, msg=:{:?}", req);

        let mut hash = req.hash;
        let mut block_headers_resp = GetBlockHeadersResponse::default();
        block_headers_resp.set_request_id(req.request_id());

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
        trace!(
            "Returned {:?} block headers to peer {:?}",
            block_headers_resp.headers.len(),
            peer
        );

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
        trace!("on_get_blocks, msg=:{:?}", req);
        if req.hashes.is_empty() {
            debug!("Received empty getblocks message: peer={:?}", peer);
        } else {
            let msg: Box<dyn Message> = Box::new(GetBlocksResponse {
                request_id: req.request_id().into(),
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
        trace!("on_get_terminal_block_hashes, msg=:{:?}", req);
        let msg: Box<dyn Message> = Box::new(GetTerminalBlockHashesResponse {
            request_id: req.request_id().into(),
            hashes: self.graph.consensus.terminal_block_hashes(),
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
        trace!(
            "on_terminal_block_hashes_response, msg=:{:?}",
            terminal_block_hashes
        );
        self.match_request(
            io,
            syn,
            peer_id,
            terminal_block_hashes.request_id(),
        )?;

        for hash in &terminal_block_hashes.hashes {
            if !self.graph.contains_block_header(&hash) {
                self.request_block_headers(
                    io,
                    syn,
                    peer_id,
                    hash,
                    DEFAULT_GET_HEADERS_NUM,
                );
            }
        }
        Ok(())
    }

    fn on_status(
        &self, io: &NetworkContext, syn: &mut SynchronizationState,
        peer_id: PeerId, rlp: &Rlp,
    ) -> Result<(), Error>
    {
        if !syn.handshaking_peers.contains_key(&peer_id)
            || syn.peers.contains_key(&peer_id)
        {
            debug!("Unexpected status message: peer={:?}", peer_id);
        }
        syn.handshaking_peers.remove(&peer_id);

        let status = rlp.as_val::<Status>()?;
        trace!("on_status, msg=:{:?}", status);
        let peer = SynchronizationPeerState {
            id: peer_id,
            protocol_version: status.protocol_version,
            genesis_hash: status.genesis_hash,
            inflight_requests: Slab::with_capacity(
                MAX_INFLIGHT_REQUEST_COUNT as usize,
            ),
            pending_requests: VecDeque::new(),
            last_sent_transactions: HashSet::new(),
        };

        trace!(
            "New peer (pv={:?}, gh={:?})",
            status.protocol_version,
            status.genesis_hash
        );

        let genesis_hash = self.graph.genesis_hash();
        if *genesis_hash != status.genesis_hash {
            debug!(
                "Peer {:?} genesis hash mismatches (ours: {:?}, theirs: {:?})",
                peer_id, genesis_hash, status.genesis_hash
            );
            return Err(ErrorKind::Invalid.into());
        }

        trace!("Peer {:?} connected", peer_id);
        syn.peers.insert(peer_id.clone(), peer);

        // FIXME Need better design.
        // Should be refactored with on_new_block_hashes.
        for terminal_block_hash in status.terminal_block_hashes {
            if !self.graph.contains_block_header(&terminal_block_hash) {
                self.request_block_headers(
                    io,
                    syn,
                    peer_id,
                    &terminal_block_hash,
                    DEFAULT_GET_HEADERS_NUM,
                );
            }
        }

        Ok(())
    }

    fn on_block_headers_response(
        &self, io: &NetworkContext, syn: &mut SynchronizationState,
        peer_id: PeerId, rlp: &Rlp,
    ) -> Result<(), Error>
    {
        let block_headers = rlp.as_val::<GetBlockHeadersResponse>()?;
        trace!("on_block_headers_response, msg=:{:?}", block_headers);
        self.match_request(io, syn, peer_id, block_headers.request_id())?;

        if block_headers.headers.is_empty() {
            trace!("Received empty GetBlockHeadersResponse message");
            return Ok(());
        }

        let mut parent_hash = H256::default();

        let mut hashes = Vec::default();
        let mut dependent_hashes = Vec::new();

        for header in &block_headers.headers {
            let hash = header.hash();
            if !self.graph.contains_block_header(&hash) {
                self.graph.insert_block_header(header.clone());
                hashes.push(hash);
            } else if !self.graph.contains_block(&hash) {
                hashes.push(hash);
            }
            for referee in header.referee_hashes() {
                dependent_hashes.push(*referee);
            }
        }
        {
            let mut headers_in_flight = self.headers_in_flight.lock();
            for header in &block_headers.headers {
                let hash = header.hash();
                headers_in_flight.remove(&header.hash());
                if parent_hash != H256::default() && parent_hash != hash {
                    return Err(ErrorKind::Invalid.into());
                }
                parent_hash = header.parent_hash().clone();
            }
        }
        dependent_hashes.push(parent_hash);
        let header_hashes: Vec<H256> = block_headers
            .headers
            .iter()
            .map(|header| header.hash())
            .collect();
        trace!(
            "get headers responce of hashes:{:?}, requesting block:{:?}",
            header_hashes,
            hashes
        );

        for past_hash in &dependent_hashes {
            if *past_hash != H256::default()
                && !self.graph.contains_block_header(past_hash)
            {
                self.request_block_headers(
                    io,
                    syn,
                    peer_id,
                    past_hash,
                    DEFAULT_GET_HEADERS_NUM,
                );
            }
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
        trace!("on_blocks_response, get {} blocks", blocks.blocks.len());
        self.match_request(io, syn, peer_id, blocks.request_id())?;

        let mut blocks_in_flight = self.blocks_in_flight.lock();
        let new_block_hashes: Vec<H256> = blocks
            .blocks
            .into_iter()
            .map(|block| {
                let hash = block.hash();
                if !self.graph.contains_block_header(&hash) {
                    self.graph.insert_block_header(block.block_header.clone());
                }
                blocks_in_flight.remove(&hash);
                if !self.graph.contains_block(&hash) {
                    self.graph.insert_block(block);
                    Some(hash)
                } else {
                    None
                }
            })
            .filter_map(|h| h)
            .collect();

        trace!("receive new blocks:{:?}", new_block_hashes);

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
        trace!("on_new_block, header={:?}", new_block.block.block_header);
        let hash = new_block.block.block_header.hash();
        let parent_hash = new_block.block.block_header.parent_hash();
        let referee_hashes = new_block.block.block_header.referee_hashes();

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
        self.headers_in_flight.lock().remove(&hash);
        self.blocks_in_flight.lock().remove(&hash);

        if !self.graph.contains_block_header(parent_hash) {
            self.request_block_headers(
                io,
                syn,
                peer_id,
                parent_hash,
                DEFAULT_GET_HEADERS_NUM,
            );
        }
        for hash in referee_hashes {
            if !self.graph.contains_block_header(hash) {
                self.request_block_headers(
                    io,
                    syn,
                    peer_id,
                    hash,
                    DEFAULT_GET_HEADERS_NUM,
                );
            }
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
        trace!("on_new_block_hashes, msg={:?}", new_block_hashes);

        for hash in new_block_hashes.block_hashes.iter() {
            if !self.graph.contains_block_header(hash) {
                self.request_block_headers(
                    io,
                    syn,
                    peer_id,
                    hash,
                    DEFAULT_GET_HEADERS_NUM,
                );
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
        trace!("Sending status message to {:?}", peer);

        let msg: Box<dyn Message> = Box::new(Status {
            protocol_version: SYNCHRONIZATION_PROTOCOL_VERSION,
            network_id: 0x0,
            genesis_hash: *self.graph.genesis_hash(),
            terminal_block_hashes: self.graph.consensus.terminal_block_hashes(),
        });
        self.send_message(io, peer, msg.as_ref())
    }

    fn request_block_headers(
        &self, io: &NetworkContext, syn: &mut SynchronizationState,
        peer_id: PeerId, hash: &H256, max_blocks: u64,
    )
    {
        let mut headers_in_flight = self.headers_in_flight.lock();
        if headers_in_flight.contains(hash) {
            return;
        }
        trace!(
            "Requesting {:?} block headers starting at {:?} from peer {:?}",
            max_blocks,
            hash,
            peer_id
        );

        if let Some(timed_req) = self.send_request(
            io,
            syn,
            peer_id,
            Box::new(RequestMessage::Headers(GetBlockHeaders {
                request_id: 0.into(),
                hash: *hash,
                max_blocks,
            })),
        ) {
            headers_in_flight.insert(hash.clone());
            self.requests_queue.lock().push(timed_req);
        }
    }

    fn request_blocks(
        &self, io: &NetworkContext, syn: &mut SynchronizationState,
        peer_id: PeerId, mut hashes: Vec<H256>,
    )
    {
        let mut blocks_in_flight = self.blocks_in_flight.lock();
        hashes.retain(|hash| !blocks_in_flight.contains(hash));
        trace!("Requesting {:?} blocks from {:?}", hashes.len(), peer_id);
        if let Some(timed_req) = self.send_request(
            io,
            syn,
            peer_id,
            Box::new(RequestMessage::Blocks(GetBlocks {
                request_id: 0.into(),
                hashes: hashes.clone(),
            })),
        ) {
            for hash in hashes {
                blocks_in_flight.insert(hash);
            }
            self.requests_queue.lock().push(timed_req);
        }
    }

    fn send_request(
        &self, io: &NetworkContext, syn: &mut SynchronizationState,
        peer_id: PeerId, mut msg: Box<RequestMessage>,
    ) -> Option<Arc<TimedSyncRequests>>
    {
        if let Some(ref mut peer) = syn.peers.get_mut(&peer_id) {
            if let Some(request_id) = peer.next_request_id() {
                msg.set_request_id(request_id as u16);
                self.send_message(io, peer_id, msg.get_msg())
                    .unwrap_or_else(|e| {
                        warn!("Error while send_message, err={:?}", e);
                    });
                let timed_req = Arc::new(TimedSyncRequests::from_request(
                    peer_id, request_id, &msg,
                ));
                peer.append_inflight_request(
                    request_id,
                    msg,
                    timed_req.clone(),
                );
                return Some(timed_req);
            } else {
                peer.append_pending_request(msg);
                return None;
            }
        }
        None
    }

    fn match_request(
        &self, io: &NetworkContext, syn: &mut SynchronizationState,
        peer_id: PeerId, request_id: u16,
    ) -> Result<(), Error>
    {
        if let Some(ref mut peer) = syn.peers.get_mut(&peer_id) {
            if peer.is_inflight_request(request_id) {
                if peer.has_pending_requests() {
                    self.remove_request(peer, request_id as usize, false);
                    let pending_req = peer.pop_pending_request().unwrap();
                    let mut pending_msg = pending_req.message;
                    pending_msg.set_request_id(request_id);
                    self.send_message(io, peer_id, pending_msg.get_msg())?;
                    let timed_req = Arc::new(TimedSyncRequests::from_request(
                        peer_id,
                        request_id as usize,
                        &pending_msg,
                    ));
                    peer.append_inflight_request(
                        request_id as usize,
                        pending_msg,
                        timed_req.clone(),
                    );
                    self.requests_queue.lock().push(timed_req);
                } else {
                    self.remove_request(peer, request_id as usize, true);
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

    fn get_transaction_pool(&self) -> SharedTransactionPool {
        self.graph.consensus.txpool.clone()
    }

    fn select_peers_for_transactions<F>(
        &self, syn: &mut SynchronizationState, filter: F,
    ) -> Vec<PeerId>
    where F: Fn(&PeerId) -> bool {
        // sqrt(x)/x scaled to max u32
        let fraction = ((syn.peers.len() as f64).powf(-0.5)
            * (u32::max_value() as f64).round()) as u32;
        let small = syn.peers.len() < MIN_PEERS_PROPAGATION;

        let mut random = random::new();
        syn.peers
            .keys()
            .cloned()
            .filter(filter)
            .filter(|_| small || random.next_u32() < fraction)
            .take(MAX_PEERS_PROPAGATION)
            .collect()
    }

    fn propagate_transactions_to_peers(
        &self, syn: &mut SynchronizationState, io: &NetworkContext,
        peers: Vec<PeerId>, transactions: Vec<SignedTransaction>,
    )
    {
        let all_transactions_hashes = transactions
            .iter()
            .map(|tx| tx.hash())
            .collect::<HashSet<H256>>();

        let lucky_peers = {
            peers.into_iter()
                .filter_map(|peer_id| {
                    let peer_info = syn.peers.get_mut(&peer_id)
                        .expect("peer_id is from peers; peers is result of select_peers_for_transactions; select_peers_for_transactions selects peers from syn.peers; qed");

                    // Send all transactions
                    if peer_info.last_sent_transactions.is_empty() {
                        let mut tx_msg = Box::new(Transactions { transactions: Vec::new() });
                        for tx in &transactions {
                            tx_msg.transactions.push(tx.transaction.clone());
                        }
                        peer_info.last_sent_transactions = all_transactions_hashes.clone();
                        return Some((peer_id, transactions.len(), tx_msg));
                    }

                    // Get hashes of all transactions to send to this peer
                    let to_send = all_transactions_hashes.difference(&peer_info.last_sent_transactions)
                        .cloned()
                        .collect::<HashSet<_>>();
                    if to_send.is_empty() {
                        return None;
                    }

                    let mut tx_msg = Box::new(Transactions { transactions: Vec::new() });
                    for tx in &transactions {
                        if to_send.contains(&tx.hash()) {
                            tx_msg.transactions.push(tx.transaction.clone());
                        }
                    }
                    peer_info.last_sent_transactions = all_transactions_hashes
                        .intersection(&peer_info.last_sent_transactions)
                        .chain(&to_send)
                        .cloned()
                        .collect();
                    Some((peer_id, tx_msg.transactions.len(), tx_msg))
                })
                .collect::<Vec<_>>()
        };

        if lucky_peers.len() > 0 {
            let mut max_sent = 0;
            let lucky_peers_len = lucky_peers.len();
            for (peer_id, sent, msg) in lucky_peers {
                self.send_message(io, peer_id, msg.as_ref())
                    .expect("Error sending transactions!");
                trace!("{:02} <- Transactions ({} entries)", peer_id, sent);
                max_sent = cmp::max(max_sent, sent);
            }
            debug!(
                "Sent up to {} transactions to {} peers.",
                max_sent, lucky_peers_len
            );
        }
    }

    pub fn propagate_new_transactions(&self, io: &NetworkContext) {
        let mut syn = self.syn.write();

        if syn.peers.is_empty() {
            return;
        }

        let transactions =
            self.get_transaction_pool().transactions_to_propagate();
        if transactions.is_empty() {
            return;
        }

        let peers = self.select_peers_for_transactions(&mut *syn, |_| true);
        self.propagate_transactions_to_peers(
            &mut *syn,
            io,
            peers,
            transactions,
        );
    }

    pub fn remove_expired_flying_request(&self, io: &NetworkContext) {
        // FIXME should get expired requests from other peers
        let mut syn = self.syn.write();
        let mut requests = self.requests_queue.lock();
        let now = Instant::now();
        loop {
            if requests.is_empty() {
                break;
            }
            let sync_req = requests.pop().unwrap();
            if sync_req.removed.load(AtomicOrdering::Relaxed) == true {
                continue;
            }
            if sync_req.timeout_time >= now {
                requests.push(sync_req);
                break;
            } else {
                // TODO Requests of removed peers should be handled when the
                // peer is removed. And should handle timeout peers.
                self.match_request(
                    io,
                    &mut *syn,
                    sync_req.peer_id,
                    sync_req.request_id as u16,
                );
            }
        }
        trace!("headers_in_flight: {:?}", *self.headers_in_flight.lock());
        trace!("blocks_in_flight: {:?}", *self.blocks_in_flight.lock());
    }

    pub fn remove_request(
        &self, peer: &mut SynchronizationPeerState, request_id: usize,
        remove_slab: bool,
    )
    {
        let has = if let Some(req) = peer.inflight_requests.get(request_id) {
            match *req.message {
                RequestMessage::Headers(ref get_headers) => {
                    self.headers_in_flight.lock().remove(&get_headers.hash);
                }
                RequestMessage::Blocks(ref get_blocks) => {
                    let mut blocks = self.blocks_in_flight.lock();
                    for hash in &get_blocks.hashes {
                        blocks.remove(hash);
                    }
                }
                _ => return,
            }
            // This should always be some for now
            if let Some(ref timed_req) = req.timed_req {
                timed_req.removed.store(true, AtomicOrdering::Relaxed);
            }
            true
        } else {
            false
        };
        if has && remove_slab {
            peer.remove_inflight_request(request_id);
        }
    }
}

impl NetworkProtocolHandler for SynchronizationProtocolHandler {
    fn initialize(&self, io: &NetworkContext) {
        io.register_timer(TX_TIMER, Duration::from_millis(1300))
            .expect("Error registering transactions timer");
        io.register_timer(CHECK_REQUEST_TIMER, Duration::from_secs(5))
            .expect("Error registering transactions timer");
    }

    fn on_message(&self, io: &NetworkContext, peer: PeerId, raw: &[u8]) {
        let msg_id = raw[0];
        let rlp = Rlp::new(&raw[1..]);
        debug!("on_message: peer={:?}, msgid={:?}", peer, msg_id);
        self.dispatch_message(io, peer, msg_id.into(), rlp);
    }

    fn on_peer_connected(&self, io: &NetworkContext, peer: PeerId) {
        let mut syn = self.syn.write();

        trace!("Peer connected: peer={:?}", peer);
        if let Err(e) = self.send_status(io, peer) {
            debug!("Error sending status message: {:?}", e);
            io.disconnect_peer(peer);
        } else {
            syn.handshaking_peers.insert(peer, Instant::now());
        }
    }

    fn on_peer_disconnected(&self, _io: &NetworkContext, peer: PeerId) {
        trace!("Peer disconnected: peer={:?}", peer);
    }

    fn on_timeout(&self, io: &NetworkContext, timer: TimerToken) {
        trace!("Timeout: timer={:?}", timer);

        match timer {
            TX_TIMER => {
                self.propagate_new_transactions(io);
            }
            CHECK_REQUEST_TIMER => {
                self.remove_expired_flying_request(io);
            }
            _ => warn!("Unknown timer {} triggered.", timer),
        }
    }
}

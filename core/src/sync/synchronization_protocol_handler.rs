use super::{
    super::transaction_pool::SharedTransactionPool, random, Error, ErrorKind,
    SharedSynchronizationGraph, SynchronizationGraph, SynchronizationPeerState,
    SynchronizationState, MAX_INFLIGHT_REQUEST_COUNT,
};
use crate::{
    bytes::Bytes, consensus::SharedConsensusGraph, pow::ProofOfWorkConfig,
};
use ethereum_types::H256;
use io::TimerToken;
use message::{
    GetBlockHeaders, GetBlockHeadersResponse, GetBlockTxn, GetBlockTxnResponce,
    GetBlocks, GetBlocksResponse, GetCompactBlocks, GetCompactBlocksResponse,
    GetTerminalBlockHashes, GetTerminalBlockHashesResponse, Message, MsgId,
    NewBlock, NewBlockHashes, Status, Transactions,
};
use network::{
    Error as NetworkError, NetworkContext, NetworkProtocolHandler, PeerId,
};
use parking_lot::{Mutex, RwLock};
use primitives::{Block, SignedTransaction};
use rand::{Rng, RngCore};
use rlp::Rlp;
//use slab::Slab;
use crate::{
    sync::synchronization_state::RequestMessage,
    verification::VerificationConfig,
};
use std::{
    cmp::{self, Ordering},
    collections::{BinaryHeap, HashSet, VecDeque},
    iter::FromIterator,
    sync::{
        atomic::{AtomicBool, Ordering as AtomicOrdering},
        Arc,
    },
    time::{Duration, Instant},
};

pub const SYNCHRONIZATION_PROTOCOL_VERSION: u8 = 0x01;

pub const MAX_HEADERS_TO_SEND: u64 = 512;
pub const MAX_BLOCKS_TO_SEND: u64 = 256;
const MIN_PEERS_PROPAGATION: usize = 4;
const MAX_PEERS_PROPAGATION: usize = 128;
const DEFAULT_GET_HEADERS_NUM: u64 = 1;

const TX_TIMER: TimerToken = 0;
const CHECK_REQUEST_TIMER: TimerToken = 1;
const BLOCK_CACHE_GC_TIMER: TimerToken = 2;
const PERSIST_TERMINAL_TIMER: TimerToken = 3;

pub struct SynchronizationProtocolHandler {
    protocol_config: ProtocolConfiguration,
    graph: SharedSynchronizationGraph,
    syn: RwLock<SynchronizationState>,
    headers_in_flight: Mutex<HashSet<H256>>,
    blocks_in_flight: Mutex<HashSet<H256>>,
    requests_queue: Mutex<BinaryHeap<Arc<TimedSyncRequests>>>,
}

pub struct ProtocolConfiguration {
    pub send_tx_period: Duration,
    pub check_request_period: Duration,
    pub block_cache_gc_period: Duration,
    pub persist_terminal_period: Duration,
    pub headers_request_timeout: Duration,
    pub blocks_request_timeout: Duration,
}

#[derive(Debug)]
pub struct TimedSyncRequests {
    pub peer_id: PeerId,
    pub timeout_time: Instant,
    pub request_id: u64,
    pub removed: AtomicBool,
}

impl TimedSyncRequests {
    pub fn new(
        peer_id: PeerId, timeout: Duration, request_id: u64,
    ) -> TimedSyncRequests {
        TimedSyncRequests {
            peer_id,
            timeout_time: Instant::now() + timeout,
            request_id,
            removed: AtomicBool::new(false),
        }
    }

    pub fn from_request(
        peer_id: PeerId, request_id: u64, msg: &RequestMessage,
        conf: &ProtocolConfiguration,
    ) -> TimedSyncRequests
    {
        let timeout = match *msg {
            RequestMessage::Headers(_) => conf.headers_request_timeout,
            RequestMessage::Blocks(_)
            | RequestMessage::Compact(_)
            | RequestMessage::BlockTxn(_) => conf.blocks_request_timeout,
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
    pub fn new(
        protocol_config: ProtocolConfiguration,
        consensus_graph: SharedConsensusGraph, pow_config: ProofOfWorkConfig,
        verification_config: VerificationConfig,
    ) -> Self
    {
        SynchronizationProtocolHandler {
            protocol_config,
            graph: Arc::new(SynchronizationGraph::new(
                consensus_graph.clone(),
                pow_config,
                verification_config,
            )),
            syn: RwLock::new(SynchronizationState::new()),
            headers_in_flight: Mutex::new(HashSet::new()),
            blocks_in_flight: Mutex::new(HashSet::new()),
            requests_queue: Mutex::new(BinaryHeap::new()),
        }
    }

    pub fn get_synchronization_graph(&self) -> SharedSynchronizationGraph {
        self.graph.clone()
    }

    pub fn block_by_hash(&self, hash: &H256) -> Option<Arc<Block>> {
        self.graph.block_by_hash(hash)
    }

    fn send_message(
        &self, io: &NetworkContext, peer: PeerId, msg: &Message,
    ) -> Result<(), NetworkError> {
        let mut raw = Bytes::new();
        raw.push(msg.msg_id().into());
        raw.extend(msg.rlp_bytes().iter());
        if let Err(e) = io.send(peer, raw) {
            debug!("Error sending message: {:?}", e);
            io.disconnect_peer(peer);
            // FIXME return error after we implement error handling
            return Ok(());
        };
        debug!(
            "Send message({}) to {:?}",
            msg.msg_id(),
            io.get_peer_node_id(peer)
        );
        Ok(())
    }

    fn dispatch_message(
        &self, io: &NetworkContext, peer: PeerId, msg_id: MsgId, rlp: Rlp,
    ) {
        trace!("Dispatching message: peer={:?}, msgid={:?}", peer, msg_id);
        match msg_id {
            MsgId::STATUS => self.on_status(io, peer, &rlp),
            MsgId::GET_BLOCK_HEADERS_RESPONSE => {
                self.on_block_headers_response(io, peer, &rlp)
            }
            MsgId::GET_BLOCK_HEADERS => {
                self.on_get_block_headers(io, peer, &rlp)
            }
            MsgId::NEW_BLOCK => self.on_new_block(io, peer, &rlp),
            MsgId::NEW_BLOCK_HASHES => self.on_new_block_hashes(io, peer, &rlp),
            MsgId::GET_BLOCKS_RESPONSE => {
                self.on_blocks_response(io, peer, &rlp)
            }
            MsgId::GET_BLOCKS => self.on_get_blocks(io, peer, &rlp),
            MsgId::GET_TERMINAL_BLOCK_HASHES_RESPONSE => {
                self.on_terminal_block_hashes_response(io, peer, &rlp)
            }
            MsgId::GET_TERMINAL_BLOCK_HASHES => {
                self.on_get_terminal_block_hashes(io, peer, &rlp)
            }
            MsgId::TRANSACTIONS => self.on_transactions(peer, &rlp),
            MsgId::GET_CMPCT_BLOCKS => {
                self.on_get_compact_blocks(io, peer, &rlp)
            }
            MsgId::GET_CMPCT_BLOCKS_RESPONSE => {
                self.on_get_compact_blocks_response(io, peer, &rlp)
            }
            MsgId::GET_BLOCK_TXN => self.on_get_blocktxn(io, peer, &rlp),
            MsgId::GET_BLOCK_TXN_RESPONSE => {
                self.on_get_blocktxn_response(io, peer, &rlp)
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

    fn on_get_compact_blocks(
        &self, io: &NetworkContext, peer: PeerId, rlp: &Rlp,
    ) -> Result<(), Error> {
        let req: GetCompactBlocks = rlp.as_val()?;
        let mut compact_blocks = Vec::with_capacity(req.hashes.len());
        debug!("on_get_compact_blocks, msg=:{:?}", req);
        for hash in &req.hashes {
            if let Some(compact_block) = self.graph.compact_block_by_hash(hash)
            {
                compact_blocks.push(compact_block);
            } else if let Some(block) = self.graph.block_by_hash(hash) {
                debug!("Have complete block but no compact block, return complete block instead");
                let block_resp = GetBlocksResponse {
                    request_id: req.request_id,
                    blocks: vec![block.as_ref().clone()],
                };
                self.send_message(io, peer, &block_resp)?;
                return Ok(());
            } else {
                warn!(
                    "Peer {} requested non-existent compact block {}",
                    peer, hash
                );
            }
        }
        let resp = GetCompactBlocksResponse {
            request_id: req.request_id,
            blocks: compact_blocks,
        };
        self.send_message(io, peer, &resp)?;
        Ok(())
    }

    fn on_get_compact_blocks_response(
        &self, io: &NetworkContext, peer: PeerId, rlp: &Rlp,
    ) -> Result<(), Error> {
        let resp: GetCompactBlocksResponse = rlp.as_val()?;
        debug!("on_get_compact_blocks_response {:?}", resp);
        let req = self.match_request(io, peer, resp.request_id())?;
        let mut failed_blocks = Vec::new();
        let mut completed_blocks = Vec::new();
        let mut requested_blocks: HashSet<H256> = match req {
            RequestMessage::Compact(request) => {
                HashSet::from_iter(request.hashes.iter().cloned())
            }
            _ => {
                warn!("Get response not matching the request! req={:?}, resp={:?}", req, resp);
                return Err(ErrorKind::UnexpectedResponse.into());
            }
        };
        for mut cmpct in resp.blocks {
            let hash = cmpct.hash();
            if !requested_blocks.remove(&hash) {
                warn!("Response has not requested compact block {:?}", hash);
                continue;
            }
            if self.graph.contains_block(&hash) {
                debug!(
                    "Get cmpct block, but full block already received, hash={}",
                    hash
                );
                self.blocks_in_flight.lock().remove(&hash);
                continue;
            } else {
                if self.graph.contains_block_header(&hash) {
                    if self.graph.contains_compact_block(&hash) {
                        debug!("Cmpct block already received, hash={}", hash);
                        self.blocks_in_flight.lock().remove(&hash);
                        continue;
                    } else {
                        debug!("Cmpct block Processing, hash={}", hash);
                        let missing = cmpct.build_partial(
                            &*self
                                .get_transaction_pool()
                                .transaction_pubkey_cache
                                .read(),
                        );
                        if !missing.is_empty() {
                            debug!(
                                "Request {} missing tx in {}",
                                missing.len(),
                                hash
                            );
                            self.graph.insert_compact_block(cmpct);
                            self.request_blocktxn(io, peer, hash, missing);
                        } else {
                            let trans = cmpct
                                .reconstructed_txes
                                .into_iter()
                                .map(|tx| tx.unwrap())
                                .collect();
                            self.blocks_in_flight.lock().remove(&hash);
                            let (success, to_relay) = self.graph.insert_block(
                                Block {
                                    block_header: cmpct.block_header,
                                    transactions: trans,
                                },
                                true,
                                true,
                            );

                            // May fail due to transactions hash collision
                            if !success {
                                failed_blocks.push(hash.clone());
                            }
                            if to_relay {
                                completed_blocks.push(hash);
                            }
                        }
                    }
                } else {
                    warn!(
                        "Get cmpct block, but header not received, hash={}",
                        hash
                    );
                    self.blocks_in_flight.lock().remove(&hash);
                    continue;
                }
            }
        }

        // Request full block if reconstruction fails
        if !failed_blocks.is_empty() {
            self.request_blocks(io, peer, failed_blocks);
        }

        // Broadcast completed block_header_ready blocks
        if !completed_blocks.is_empty() {
            let new_block_hash_msg: Box<dyn Message> =
                Box::new(NewBlockHashes {
                    block_hashes: completed_blocks,
                });
            self.broadcast_message(
                io,
                PeerId::max_value(),
                new_block_hash_msg.as_ref(),
            )?;
        }

        // Request missing compact blocks from another random peer
        if !requested_blocks.is_empty() {
            {
                let mut blocks_in_flight = self.blocks_in_flight.lock();
                for hash in &requested_blocks {
                    blocks_in_flight.remove(hash);
                }
            }
            let chosen_peer = self.choose_peer_after_failure(peer);
            self.request_compact_block(
                io,
                chosen_peer,
                requested_blocks.into_iter().collect(),
            );
        }
        Ok(())
    }

    fn on_get_blocktxn(
        &self, io: &NetworkContext, peer: PeerId, rlp: &Rlp,
    ) -> Result<(), Error> {
        let req: GetBlockTxn = rlp.as_val()?;
        debug!("on_get_blocktxn");
        match self.graph.block_by_hash(&req.block_hash) {
            Some(block) => {
                debug!("Process get_blocktxn hash={:?}", block.hash());
                let mut tx_resp = Vec::with_capacity(req.indexes.len());
                let mut last = 0;
                for index in req.indexes {
                    last += index;
                    if last >= block.transactions.len() {
                        warn!(
                            "Request tx index out of bound, peer={}, hash={}",
                            peer,
                            block.hash()
                        );
                        return Err(ErrorKind::Invalid.into());
                    }
                    tx_resp.push(block.transactions[last].transaction.clone());
                    last += 1;
                }
                let resp = GetBlockTxnResponce {
                    request_id: req.request_id,
                    block_hash: req.block_hash,
                    block_txn: tx_resp,
                };
                self.send_message(io, peer, &resp)?;
            }
            None => {
                warn!(
                    "Get blocktxn request of non-existent block, hash={}",
                    req.block_hash
                );

                let resp = GetBlockTxnResponce {
                    request_id: req.request_id,
                    block_hash: H256::default(),
                    block_txn: Vec::new(),
                };
                self.send_message(io, peer, &resp)?;
            }
        }
        Ok(())
    }

    fn on_get_blocktxn_response(
        &self, io: &NetworkContext, peer: PeerId, rlp: &Rlp,
    ) -> Result<(), Error> {
        let resp: GetBlockTxnResponce = rlp.as_val()?;
        debug!("on_get_blocktxn_response");
        let hash = resp.block_hash;
        let req = self.match_request(io, peer, resp.request_id())?;
        let req = match req {
            RequestMessage::BlockTxn(request) => request,
            _ => {
                warn!("Get response not matching the request! req={:?}, resp={:?}", req, resp);
                return Err(ErrorKind::UnexpectedResponse.into());
            }
        };
        let mut request_again = false;
        if hash != req.block_hash {
            warn!("Response blocktxn is not the requested block, req={:?}, resp={:?}", req.block_hash, hash);
            request_again = true;
        } else {
            if self.graph.contains_block(&hash) {
                debug!(
                    "Get blocktxn, but full block already received, hash={}",
                    hash
                );
            } else {
                if self.graph.contains_block_header(&hash) {
                    debug!("Process blocktxn hash={:?}", hash);
                    let signed_txes =
                        SignedTransaction::batch_recover_with_cache(
                            &resp.block_txn,
                            &mut *self
                                .get_transaction_pool()
                                .transaction_pubkey_cache
                                .write(),
                        )?;
                    match self.graph.compact_block_by_hash(&hash) {
                        Some(cmpct) => {
                            let mut trans = Vec::with_capacity(
                                cmpct.reconstructed_txes.len(),
                            );
                            let mut index = 0;
                            for tx in cmpct.reconstructed_txes {
                                match tx {
                                    Some(tx) => trans.push(tx),
                                    None => {
                                        trans.push(signed_txes[index].clone());
                                        index += 1;
                                    }
                                }
                            }

                            let (success, to_relay) = self.graph.insert_block(
                                Block {
                                    block_header: cmpct.block_header,
                                    transactions: trans,
                                },
                                true,
                                true,
                            );

                            let mut blocks = Vec::new();
                            blocks.push(hash);
                            // May fail due to transactions hash collision
                            if !success {
                                self.request_blocks(io, peer, blocks.clone());
                            }
                            if to_relay {
                                let new_block_hash_msg: Box<dyn Message> =
                                    Box::new(NewBlockHashes {
                                        block_hashes: blocks,
                                    });
                                self.broadcast_message(
                                    io,
                                    PeerId::max_value(),
                                    new_block_hash_msg.as_ref(),
                                )?;
                            }
                        }
                        None => {
                            request_again = true;
                            warn!("Get blocktxn, but misses compact block, hash={}", hash);
                        }
                    }
                } else {
                    request_again = true;
                    warn!(
                        "Get blocktxn, but header not received, hash={}",
                        hash
                    );
                }
            }
        }
        if request_again {
            let chosen_peer = self.choose_peer_after_failure(peer);
            self.request_blocks(io, chosen_peer, vec![req.block_hash]);
        }
        Ok(())
    }

    fn on_transactions(&self, peer: PeerId, rlp: &Rlp) -> Result<(), Error> {
        if !self.syn.read().peers.contains_key(&peer) {
            warn!("Unexpected message from unrecognized peer: peer={:?} msg=GET_TERMINAL_BLOCK_HASHES", peer);
            return Ok(());
        }

        let transactions = rlp.as_val::<Transactions>()?;
        let transactions = transactions.transactions;
        debug!(
            "Received {:?} transactions from Peer {:?}",
            transactions.len(),
            peer
        );

        self.syn
            .write()
            .peers
            .get_mut(&peer)
            .unwrap()
            .last_sent_transactions
            .extend(transactions.iter().map(|tx| tx.hash()));

        self.get_transaction_pool().insert_new_transactions(
            self.graph.consensus.best_state_block_hash(),
            transactions,
        );
        debug!("Transactions successfully inserted to transaction pool");
        Ok(())
    }

    fn on_get_block_headers(
        &self, io: &NetworkContext, peer: PeerId, rlp: &Rlp,
    ) -> Result<(), Error> {
        if !self.syn.read().peers.contains_key(&peer) {
            warn!("Unexpected message from unrecognized peer: peer={:?} msg=GET_BLOCK_HEADERS", peer);
            return Ok(());
        }

        let req = rlp.as_val::<GetBlockHeaders>()?;
        debug!("on_get_block_headers, msg=:{:?}", req);

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
        debug!(
            "Returned {:?} block headers to peer {:?}",
            block_headers_resp.headers.len(),
            peer
        );

        let msg: Box<dyn Message> = Box::new(block_headers_resp);
        self.send_message(io, peer, msg.as_ref())?;
        Ok(())
    }

    fn on_get_blocks(
        &self, io: &NetworkContext, peer: PeerId, rlp: &Rlp,
    ) -> Result<(), Error> {
        if !self.syn.read().peers.contains_key(&peer) {
            warn!("Unexpected message from unrecognized peer: peer={:?} msg=GET_BLOCKS", peer);
            return Ok(());
        }

        let req = rlp.as_val::<GetBlocks>()?;
        debug!("on_get_blocks, msg=:{:?}", req);
        if req.hashes.is_empty() {
            debug!("Received empty getblocks message: peer={:?}", peer);
        } else {
            let msg: Box<dyn Message> = Box::new(GetBlocksResponse {
                request_id: req.request_id().into(),
                blocks: req
                    .hashes
                    .iter()
                    .take(MAX_BLOCKS_TO_SEND as usize)
                    .filter_map(|hash| {
                        self.graph
                            .block_by_hash(hash)
                            .map(|b| b.as_ref().clone())
                    })
                    .collect(),
            });
            self.send_message(io, peer, msg.as_ref())?;
        }
        Ok(())
    }

    fn on_get_terminal_block_hashes(
        &self, io: &NetworkContext, peer: PeerId, rlp: &Rlp,
    ) -> Result<(), Error> {
        if !self.syn.read().peers.contains_key(&peer) {
            warn!("Unexpected message from unrecognized peer: peer={:?} msg=GET_TERMINAL_BLOCK_HASHES", peer);
            return Ok(());
        }

        let req = rlp.as_val::<GetTerminalBlockHashes>()?;
        debug!("on_get_terminal_block_hashes, msg=:{:?}", req);
        let msg: Box<dyn Message> = Box::new(GetTerminalBlockHashesResponse {
            request_id: req.request_id().into(),
            hashes: self.graph.get_best_info().terminal_block_hashes,
        });
        self.send_message(io, peer, msg.as_ref())?;
        Ok(())
    }

    fn on_terminal_block_hashes_response(
        &self, io: &NetworkContext, peer: PeerId, rlp: &Rlp,
    ) -> Result<(), Error> {
        if !self.syn.read().peers.contains_key(&peer) {
            warn!("Unexpected message from unrecognized peer: peer={:?} msg=GET_TERMINAL_BLOCK_HASHES_RESPONSE", peer);
            return Ok(());
        }

        let terminal_block_hashes =
            rlp.as_val::<GetTerminalBlockHashesResponse>()?;
        debug!(
            "on_terminal_block_hashes_response, msg=:{:?}",
            terminal_block_hashes
        );
        self.match_request(io, peer, terminal_block_hashes.request_id())?;

        for hash in &terminal_block_hashes.hashes {
            if !self.graph.contains_block_header(&hash) {
                self.request_block_headers(
                    io,
                    peer,
                    hash,
                    DEFAULT_GET_HEADERS_NUM,
                );
            }
        }
        Ok(())
    }

    fn on_status(
        &self, io: &NetworkContext, peer: PeerId, rlp: &Rlp,
    ) -> Result<(), Error> {
        {
            let mut syn = self.syn.write();

            if !syn.handshaking_peers.contains_key(&peer)
                || syn.peers.contains_key(&peer)
            {
                debug!("Unexpected status message: peer={:?}", peer);
            }
            syn.handshaking_peers.remove(&peer);
        }

        let mut status = rlp.as_val::<Status>()?;
        debug!("on_status, msg=:{:?}", status);
        let genesis_hash = self.graph.genesis_hash();
        if *genesis_hash != status.genesis_hash {
            debug!(
                "Peer {:?} genesis hash mismatches (ours: {:?}, theirs: {:?})",
                peer, genesis_hash, status.genesis_hash
            );
            return Err(ErrorKind::Invalid.into());
        }

        let mut requests_vec =
            Vec::with_capacity(MAX_INFLIGHT_REQUEST_COUNT as usize);
        for _i in 0..MAX_INFLIGHT_REQUEST_COUNT {
            requests_vec.push(None);
        }
        let peer_state = SynchronizationPeerState {
            id: peer,
            protocol_version: status.protocol_version,
            genesis_hash: status.genesis_hash,
            inflight_requests: requests_vec,
            lowest_request_id: 0,
            next_request_id: 0,
            pending_requests: VecDeque::new(),
            last_sent_transactions: HashSet::new(),
        };

        debug!(
            "New peer (pv={:?}, gh={:?})",
            status.protocol_version, status.genesis_hash
        );

        debug!("Peer {:?} connected", peer);
        {
            let mut syn = self.syn.write();
            syn.peers.insert(peer.clone(), peer_state);
        }

        {
            let mut missed_hashes =
                self.graph.initial_missed_block_hashes.lock();
            if !missed_hashes.is_empty() {
                status
                    .terminal_block_hashes
                    .extend(missed_hashes.iter().clone());
                missed_hashes.clear();
            }
        };

        // FIXME Need better design.
        // Should be refactored with on_new_block_hashes.
        for terminal_block_hash in status.terminal_block_hashes {
            if !self.graph.contains_block_header(&terminal_block_hash) {
                self.request_block_headers(
                    io,
                    peer,
                    &terminal_block_hash,
                    DEFAULT_GET_HEADERS_NUM,
                );
            }
        }

        Ok(())
    }

    fn on_block_headers_response(
        &self, io: &NetworkContext, peer: PeerId, rlp: &Rlp,
    ) -> Result<(), Error> {
        if !self.syn.read().peers.contains_key(&peer) {
            warn!("Unexpected message from unrecognized peer: peer={:?} msg=GET_BLOCK_HEADERS_RESPONSE", peer);
            return Ok(());
        }

        let block_headers = rlp.as_val::<GetBlockHeadersResponse>()?;
        debug!("on_block_headers_response, msg=:{:?}", block_headers);
        self.match_request(io, peer, block_headers.request_id())?;

        if block_headers.headers.is_empty() {
            trace!("Received empty GetBlockHeadersResponse message");
            return Ok(());
        }

        let mut parent_hash = H256::default();

        let mut hashes = Vec::default();
        let mut dependent_hashes = Vec::new();
        let mut need_to_relay = Vec::new();

        for header in &block_headers.headers {
            let hash = header.hash();

            let res = self.graph.insert_block_header(header.clone(), true);

            if res.0 {
                // Valid block based on header
                if !self.graph.contains_block(&hash) {
                    hashes.push(hash);
                }

                need_to_relay.extend(res.1);

                for referee in header.referee_hashes() {
                    dependent_hashes.push(*referee);
                }
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

        // FIXME: Should we make this code executable only in debug mode?
        let header_hashes: Vec<H256> = block_headers
            .headers
            .iter()
            .map(|header| header.hash())
            .collect();
        debug!(
            "get headers responce of hashes:{:?}, requesting block:{:?}",
            header_hashes, hashes
        );

        for past_hash in &dependent_hashes {
            if *past_hash != H256::default()
                && !self.graph.contains_block_header(past_hash)
            {
                self.request_block_headers(
                    io,
                    peer,
                    past_hash,
                    DEFAULT_GET_HEADERS_NUM,
                );
            }
        }
        if !hashes.is_empty() {
            // TODO configure which path to use
            self.request_compact_block(io, peer, hashes);
            // self.request_blocks(io, peer, hashes);
        }

        if !need_to_relay.is_empty() {
            let new_block_hash_msg: Box<dyn Message> =
                Box::new(NewBlockHashes {
                    block_hashes: need_to_relay,
                });
            self.broadcast_message(
                io,
                PeerId::max_value(),
                new_block_hash_msg.as_ref(),
            )?;
        }

        Ok(())
    }

    fn on_blocks_response(
        &self, io: &NetworkContext, peer: PeerId, rlp: &Rlp,
    ) -> Result<(), Error> {
        if !self.syn.read().peers.contains_key(&peer) {
            warn!("Unexpected message from unrecognized peer: peer={:?} msg=GET_BLOCKS_RESPONSE", peer);
            return Ok(());
        }

        let blocks = rlp.as_val::<GetBlocksResponse>()?;
        debug!(
            "on_blocks_response, get block hashes {:?}",
            blocks
                .blocks
                .iter()
                .map(|b| b.block_header.hash())
                .collect::<Vec<H256>>()
        );
        let req = self.match_request(io, peer, blocks.request_id())?;
        let req_hashes_vec = match req {
            RequestMessage::Blocks(request) => request.hashes,
            RequestMessage::Compact(request) => request.hashes,
            _ => {
                warn!("Get response not matching the request! req={:?}, resp={:?}", req, blocks);
                return Err(ErrorKind::UnexpectedResponse.into());
            }
        };
        let mut requested_blocks: HashSet<H256> =
            req_hashes_vec.into_iter().collect();

        let mut need_to_relay = Vec::new();
        for mut block in blocks.blocks {
            let hash = block.hash();
            if !requested_blocks.remove(&hash) {
                warn!("Response has not requested block {:?}", hash);
                continue;
            }
            block.recover_public(
                &mut *self
                    .get_transaction_pool()
                    .transaction_pubkey_cache
                    .write(),
            )?;

            let res = self
                .graph
                .insert_block_header(block.block_header.clone(), true);
            if res.0 {
                need_to_relay.extend(res.1);
            } else {
                continue;
            }

            let (_, to_relay) = self.graph.insert_block(block, true, true);
            if to_relay {
                need_to_relay.push(hash);
            }
        }

        if !need_to_relay.is_empty() {
            let new_block_hash_msg: Box<dyn Message> =
                Box::new(NewBlockHashes {
                    block_hashes: need_to_relay,
                });
            self.broadcast_message(
                io,
                PeerId::max_value(),
                new_block_hash_msg.as_ref(),
            )?;
        }

        // Request missing blocks from another random peer
        if !requested_blocks.is_empty() {
            let chosen_peer = self.choose_peer_after_failure(peer);
            self.request_blocks(
                io,
                chosen_peer,
                requested_blocks.into_iter().collect(),
            );
        }

        Ok(())
    }

    pub fn on_mined_block(&self, block: Block) -> Vec<H256> {
        let hash = block.block_header.hash();
        let parent_hash = *block.block_header.parent_hash();

        assert!(self.graph.contains_block_header(&parent_hash));
        assert!(!self.graph.contains_block_header(&hash));
        let res = self
            .graph
            .insert_block_header(block.block_header.clone(), false);
        assert!(res.0);

        assert!(!self.graph.contains_block(&hash));
        // Do not need to look at the result since this new block will be
        // broadcast to peers.
        self.graph.insert_block(block, false, true);
        res.1
    }

    fn on_new_decoded_block(
        &self, block: Block, need_to_verify: bool, persistent: bool,
    ) -> Result<Vec<H256>, Error> {
        let hash = block.block_header.hash();
        let mut need_to_relay = Vec::new();
        let res = self
            .graph
            .insert_block_header(block.block_header.clone(), need_to_verify);
        if res.0 {
            need_to_relay.extend(res.1);
        } else {
            return Err(Error::from_kind(ErrorKind::Invalid));
        }

        let (_, to_relay) =
            self.graph.insert_block(block, need_to_verify, persistent);
        if to_relay {
            need_to_relay.push(hash);
        }
        Ok(need_to_relay)
    }

    fn on_new_block(
        &self, io: &NetworkContext, peer: PeerId, rlp: &Rlp,
    ) -> Result<(), Error> {
        if !self.syn.read().peers.contains_key(&peer) {
            warn!("Unexpected message from unrecognized peer: peer={:?} msg=NEW_BLOCK", peer);
            return Ok(());
        }

        let mut new_block = rlp.as_val::<NewBlock>()?;
        new_block.block.recover_public(
            &mut *self.get_transaction_pool().transaction_pubkey_cache.write(),
        )?;
        let block = new_block.block;
        debug!(
            "on_new_block, header={:?} tx_number={}",
            block.block_header,
            block.transactions.len()
        );
        let hash = block.block_header.hash();

        self.headers_in_flight.lock().remove(&hash);
        self.blocks_in_flight.lock().remove(&hash);

        let parent_hash = block.block_header.parent_hash().clone();
        let referee_hashes = block.block_header.referee_hashes().clone();

        let need_to_relay = self.on_new_decoded_block(block, true, true)?;

        debug_assert!(!self.graph.verified_invalid(&parent_hash));
        if !self.graph.contains_block_header(&parent_hash) {
            self.request_block_headers(
                io,
                peer,
                &parent_hash,
                DEFAULT_GET_HEADERS_NUM,
            );
        }
        for hash in referee_hashes {
            debug_assert!(!self.graph.verified_invalid(&hash));
            if !self.graph.contains_block_header(&hash) {
                self.request_block_headers(
                    io,
                    peer,
                    &hash,
                    DEFAULT_GET_HEADERS_NUM,
                );
            }
        }

        // broadcast the hash of the newly got block
        if !need_to_relay.is_empty() {
            let new_block_hash_msg: Box<dyn Message> =
                Box::new(NewBlockHashes {
                    block_hashes: need_to_relay,
                });
            self.broadcast_message(
                io,
                PeerId::max_value(),
                new_block_hash_msg.as_ref(),
            )?;
        }

        Ok(())
    }

    fn on_new_block_hashes(
        &self, io: &NetworkContext, peer: PeerId, rlp: &Rlp,
    ) -> Result<(), Error> {
        if !self.syn.read().peers.contains_key(&peer) {
            warn!("Unexpected message from unrecognized peer: peer={:?} msg=NEW_BLOCK_HASHES", peer);
            return Ok(());
        }

        let new_block_hashes = rlp.as_val::<NewBlockHashes>()?;
        debug!("on_new_block_hashes, msg={:?}", new_block_hashes);

        for hash in new_block_hashes.block_hashes.iter() {
            if !self.graph.contains_block_header(hash) {
                self.request_block_headers(
                    io,
                    peer,
                    hash,
                    DEFAULT_GET_HEADERS_NUM,
                );
            }
        }
        Ok(())
    }

    fn broadcast_message(
        &self, io: &NetworkContext, skip_id: PeerId, msg: &Message,
    ) -> Result<(), NetworkError> {
        for (id, _) in self.syn.read().peers.iter() {
            if *id != skip_id {
                self.send_message(io, *id, msg)?;
            }
        }
        Ok(())
    }

    fn send_status(
        &self, io: &NetworkContext, peer: PeerId,
    ) -> Result<(), NetworkError> {
        debug!("Sending status message to {:?}", peer);

        let msg: Box<dyn Message> = Box::new(Status {
            protocol_version: SYNCHRONIZATION_PROTOCOL_VERSION,
            network_id: 0x0,
            genesis_hash: *self.graph.genesis_hash(),
            terminal_block_hashes: self
                .graph
                .get_best_info()
                .terminal_block_hashes,
        });
        self.send_message(io, peer, msg.as_ref())
    }

    fn request_block_headers(
        &self, io: &NetworkContext, peer_id: PeerId, hash: &H256,
        max_blocks: u64,
    )
    {
        let syn = &mut *self.syn.write();
        let mut headers_in_flight = self.headers_in_flight.lock();
        if headers_in_flight.contains(hash) {
            return;
        }

        if let Some(timed_req) = self.send_request(
            io,
            peer_id,
            Box::new(RequestMessage::Headers(GetBlockHeaders {
                request_id: 0.into(),
                hash: *hash,
                max_blocks,
            })),
            syn,
        ) {
            debug!(
                "Requesting {:?} block headers starting at {:?} from peer {:?} request_id={:?}",
                max_blocks,
                hash,
                peer_id,
                timed_req.request_id
            );
            headers_in_flight.insert(hash.clone());
            self.requests_queue.lock().push(timed_req);
        }
    }

    fn request_blocks(
        &self, io: &NetworkContext, peer_id: PeerId, mut hashes: Vec<H256>,
    ) {
        let syn = &mut *self.syn.write();
        let mut blocks_in_flight = self.blocks_in_flight.lock();
        hashes.retain(|hash| !blocks_in_flight.contains(hash));

        if let Some(timed_req) = self.send_request(
            io,
            peer_id,
            Box::new(RequestMessage::Blocks(GetBlocks {
                request_id: 0.into(),
                hashes: hashes.clone(),
            })),
            syn,
        ) {
            debug!(
                "Requesting blocks {:?} from {:?} request_id={}",
                hashes, peer_id, timed_req.request_id
            );
            for hash in hashes {
                blocks_in_flight.insert(hash);
            }
            self.requests_queue.lock().push(timed_req);
        }
    }

    fn request_compact_block(
        &self, io: &NetworkContext, peer_id: PeerId, mut hashes: Vec<H256>,
    ) {
        let syn = &mut *self.syn.write();
        let mut blocks_in_flight = self.blocks_in_flight.lock();
        hashes.retain(|hash| !blocks_in_flight.contains(hash));

        if let Some(timed_req) = self.send_request(
            io,
            peer_id,
            Box::new(RequestMessage::Compact(GetCompactBlocks {
                request_id: 0.into(),
                hashes: hashes.clone(),
            })),
            syn,
        ) {
            debug!(
                "Requesting compact blocks {:?} from {:?} request_id={}",
                hashes, peer_id, timed_req.request_id
            );
            for hash in hashes {
                blocks_in_flight.insert(hash);
            }
            self.requests_queue.lock().push(timed_req);
        }
    }

    fn request_blocktxn(
        &self, io: &NetworkContext, peer_id: PeerId, block_hash: H256,
        indexes: Vec<usize>,
    )
    {
        let syn = &mut *self.syn.write();
        if let Some(timed_req) = self.send_request(
            io,
            peer_id,
            Box::new(RequestMessage::BlockTxn(GetBlockTxn {
                request_id: 0.into(),
                block_hash: block_hash.clone(),
                indexes: indexes.clone(),
            })),
            syn,
        ) {
            debug!(
                "Requesting blocktxn {:?} from {:?} request_id={}",
                block_hash, peer_id, timed_req.request_id
            );
            self.requests_queue.lock().push(timed_req);
        }
    }

    fn send_request(
        &self, io: &NetworkContext, peer_id: PeerId,
        mut msg: Box<RequestMessage>, syn: &mut SynchronizationState,
    ) -> Option<Arc<TimedSyncRequests>>
    {
        if let Some(ref mut peer) = syn.peers.get_mut(&peer_id) {
            if let Some(request_id) = peer.get_next_request_id() {
                msg.set_request_id(request_id);
                self.send_message(io, peer_id, msg.get_msg())
                    .unwrap_or_else(|e| {
                        warn!("Error while send_message, err={:?}", e);
                    });
                let timed_req = Arc::new(TimedSyncRequests::from_request(
                    peer_id,
                    request_id,
                    &msg,
                    &self.protocol_config,
                ));
                peer.append_inflight_request(
                    request_id,
                    msg,
                    timed_req.clone(),
                );
                return Some(timed_req);
            } else {
                trace!("Append requests for later:{:?}", msg);
                peer.append_pending_request(msg);
                return None;
            }
        }
        warn!("No peer for request:{:?}", msg);
        None
    }

    fn match_request(
        &self, io: &NetworkContext, peer_id: PeerId, request_id: u64,
    ) -> Result<RequestMessage, Error> {
        let mut syn = self.syn.write();
        if let Some(ref mut peer) = syn.peers.get_mut(&peer_id) {
            if let Some(removed_req) = self.remove_request(peer, request_id) {
                while peer.has_pending_requests() {
                    if let Some(new_request_id) = peer.get_next_request_id() {
                        let mut pending_msg =
                            peer.pop_pending_request().unwrap();
                        pending_msg.set_request_id(new_request_id);
                        self.send_message(io, peer_id, pending_msg.get_msg())?;
                        let timed_req =
                            Arc::new(TimedSyncRequests::from_request(
                                peer_id,
                                new_request_id,
                                &pending_msg,
                                &self.protocol_config,
                            ));
                        peer.append_inflight_request(
                            new_request_id,
                            pending_msg,
                            timed_req.clone(),
                        );
                        self.requests_queue.lock().push(timed_req);
                    } else {
                        break;
                    }
                }
                Ok(removed_req)
            } else {
                warn!(
                    "Unexpected Responce: peer={:?} request_id={:?}",
                    peer_id, request_id
                );
                Err(ErrorKind::UnexpectedResponse.into())
            }
        } else {
            Err(ErrorKind::UnknownPeer.into())
        }
    }

    fn choose_peer_after_failure(&self, failed_peer: PeerId) -> PeerId {
        let syn = self.syn.read();
        if syn.peers.len() <= 1 {
            failed_peer
        } else {
            let mut exclude = HashSet::new();
            exclude.insert(failed_peer);
            syn.get_random_peer(&exclude).expect("Has available peer")
        }
    }

    pub fn announce_new_blocks(&self, io: &NetworkContext, hashes: &[H256]) {
        let syn = self.syn.read();
        for hash in hashes {
            let block = self.graph.block_by_hash(hash).unwrap();
            let msg: Box<dyn Message> = Box::new(NewBlock {
                block: (*block).clone().into(),
            });
            for (id, _) in syn.peers.iter() {
                self.send_message(io, *id, msg.as_ref())
                    .unwrap_or_else(|e| {
                        warn!("Error sending new blocks, err={:?}", e);
                    });
            }
        }
    }

    pub fn relay_blocks(
        &self, io: &NetworkContext, need_to_relay: Vec<H256>,
    ) -> Result<(), Error> {
        if !need_to_relay.is_empty() {
            let new_block_hash_msg: Box<dyn Message> =
                Box::new(NewBlockHashes {
                    block_hashes: need_to_relay,
                });
            self.broadcast_message(
                io,
                PeerId::max_value(),
                new_block_hash_msg.as_ref(),
            )?;
        }

        Ok(())
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
        peers: Vec<PeerId>, transactions: Vec<Arc<SignedTransaction>>,
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
        if self.syn.read().peers.is_empty() {
            return;
        }

        let transactions =
            self.get_transaction_pool().transactions_to_propagate();
        if transactions.is_empty() {
            return;
        }

        let mut syn = self.syn.write();
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
        let now = Instant::now();
        let mut timeout_requests = Vec::new();
        {
            let mut requests = self.requests_queue.lock();
            loop {
                if requests.is_empty() {
                    break;
                }
                let sync_req = requests.pop().expect("queue not empty");
                if sync_req.removed.load(AtomicOrdering::Relaxed) == true {
                    continue;
                }
                if sync_req.timeout_time >= now {
                    requests.push(sync_req);
                    break;
                } else {
                    // TODO And should handle timeout peers.
                    timeout_requests.push(sync_req);
                }
            }
        }
        for sync_req in timeout_requests {
            trace!("Timeout sync_req: {:?}", sync_req);
            let req =
                self.match_request(io, sync_req.peer_id, sync_req.request_id);
            match req {
                Ok(request) => {
                    // TODO may have better choice than random peer
                    debug!("Timeout request: {:?}", request);
                    let chosen_peer = {
                        let syn = self.syn.read();
                        let peers_vec: Vec<&PeerId> =
                            syn.peers.keys().collect();
                        let p = peers_vec
                            .get(random::new().gen_range(0, peers_vec.len()))
                            .unwrap();
                        **p
                    };
                    match request {
                        RequestMessage::Headers(get_headers) => {
                            self.request_block_headers(
                                io,
                                chosen_peer,
                                &get_headers.hash,
                                get_headers.max_blocks,
                            );
                        }
                        RequestMessage::Blocks(get_blocks) => {
                            self.request_blocks(
                                io,
                                chosen_peer,
                                get_blocks.hashes,
                            );
                        }
                        RequestMessage::Compact(get_compact) => {
                            {
                                let mut blocks_in_flight =
                                    self.blocks_in_flight.lock();
                                for hash in &get_compact.hashes {
                                    blocks_in_flight.remove(hash);
                                }
                            }
                            self.request_blocks(
                                io,
                                chosen_peer,
                                get_compact.hashes,
                            );
                        }
                        RequestMessage::BlockTxn(blocktxn) => {
                            let mut hashes = Vec::new();
                            hashes.push(blocktxn.block_hash);
                            self.request_blocks(io, chosen_peer, hashes);
                        }
                        _ => {}
                    }
                }
                Err(e) => {
                    warn!("match request err={:?}", e);
                }
            }
        }
        debug!("headers_in_flight: {:?}", *self.headers_in_flight.lock());
        debug!("blocks_in_flight: {:?}", *self.blocks_in_flight.lock());
    }

    pub fn remove_request(
        &self, peer: &mut SynchronizationPeerState, request_id: u64,
    ) -> Option<RequestMessage> {
        peer.remove_inflight_request(request_id).map(|req| {
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
                RequestMessage::BlockTxn(ref blocktxn) => {
                    self.blocks_in_flight.lock().remove(&blocktxn.block_hash);
                }
                _ => {}
            }
            req.timed_req.removed.store(true, AtomicOrdering::Relaxed);
            *req.message
        })
    }

    fn block_cache_gc(&self) { self.graph.block_cache_gc(); }

    fn persist_terminals(&self) { self.graph.persist_terminals(); }
}

impl NetworkProtocolHandler for SynchronizationProtocolHandler {
    fn initialize(&self, io: &NetworkContext) {
        io.register_timer(TX_TIMER, self.protocol_config.send_tx_period)
            .expect("Error registering transactions timer");
        io.register_timer(
            CHECK_REQUEST_TIMER,
            self.protocol_config.check_request_period,
        )
        .expect("Error registering transactions timer");
        io.register_timer(
            BLOCK_CACHE_GC_TIMER,
            self.protocol_config.block_cache_gc_period,
        )
        .expect("Error registering block_cache_gc timer");
        io.register_timer(
            PERSIST_TERMINAL_TIMER,
            self.protocol_config.persist_terminal_period,
        )
        .expect("Error registering persist terminals timer");
    }

    fn on_message(&self, io: &NetworkContext, peer: PeerId, raw: &[u8]) {
        let msg_id = raw[0];
        let rlp = Rlp::new(&raw[1..]);
        debug!("on_message: peer={:?}, msgid={:?}", peer, msg_id);
        self.dispatch_message(io, peer, msg_id.into(), rlp);
    }

    fn on_peer_connected(&self, io: &NetworkContext, peer: PeerId) {
        let mut syn = self.syn.write();

        debug!("Peer connected: peer={:?}", peer);
        if let Err(e) = self.send_status(io, peer) {
            debug!("Error sending status message: {:?}", e);
            io.disconnect_peer(peer);
        } else {
            syn.handshaking_peers.insert(peer, Instant::now());
        }
    }

    fn on_peer_disconnected(&self, _io: &NetworkContext, peer: PeerId) {
        info!("Peer disconnected: peer={:?}", peer);
        let mut syn = self.syn.write();
        syn.peers.remove(&peer);
        syn.handshaking_peers.remove(&peer);
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
            BLOCK_CACHE_GC_TIMER => {
                self.block_cache_gc();
            }
            PERSIST_TERMINAL_TIMER => {
                self.persist_terminals();
            }
            _ => warn!("Unknown timer {} triggered.", timer),
        }
    }
}

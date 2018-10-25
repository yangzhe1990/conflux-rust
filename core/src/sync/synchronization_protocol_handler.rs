use super::{
    Error, ErrorKind, SynchronizationGraph, SynchronizationPeerAsking,
    SynchronizationPeerState, SynchronizationState,
};
use bytes::Bytes;
use consensus::SharedConsensusGraph;
use ethereum_types::H256;
use io::TimerToken;
use message::{
    BlockHeaders, Blocks, GetBlockHeaders, GetBlocks, Message, MsgId, NewBlock,
    Status, TerminalBlockHashes,
};
use network::{
    Error as NetworkError, NetworkContext, NetworkProtocolHandler, PeerId,
};
use parking_lot::RwLock;
use rlp::Rlp;
use std::{cmp, time::Instant};

pub const SYNCHRONIZATION_PROTOCOL_VERSION: u8 = 0x01;

pub const MAX_HEADERS_TO_SEND: u64 = 512;
pub const MAX_BLOCKS_TO_SEND: u64 = 256;

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
        &self, io: &NetworkContext, peer: PeerId, msg: Box<dyn Message>,
    ) -> Result<(), NetworkError> {
        let mut raw = Bytes::new();
        raw.push(msg.msg_id().into());
        raw.extend(msg.rlp_bytes());
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
            MsgId::BLOCK_HEADERS => {
                self.on_block_headers(io, &mut *syn, peer, &rlp)
            }
            MsgId::GET_BLOCK_HEADERS => {
                self.get_block_headers(io, &mut *syn, &rlp, peer)
            }
            MsgId::NEW_BLOCK => self.on_new_block(io, &mut *syn, peer, &rlp),
            MsgId::BLOCKS => self.on_blocks(io, &mut *syn, peer, &rlp),
            MsgId::GET_BLOCKS => self.get_blocks(io, &mut *syn, &rlp, peer),
            MsgId::TERMINAL_BLOCK_HASHES => {
                self.on_terminal_block_hashes(io, &mut *syn, &rlp, peer)
            }
            MsgId::GET_TERMINAL_BLOCK_HASHES => {
                self.get_terminal_block_hashes(io, &mut *syn, &rlp, peer)
            }
            _ => {
                debug!(target: "sync", "Unknown message: peer={:?} msgid={:?}", peer, msg_id);
                Ok(())
            }
        }.unwrap();
    }

    fn get_block_headers(
        &self, io: &NetworkContext, syn: &mut SynchronizationState, rlp: &Rlp,
        peer: PeerId,
    ) -> Result<(), Error>
    {
        let req = rlp.as_val::<GetBlockHeaders>()?;
        trace!(target: "sync", "Peer {:?} requested {:?} block headers starting at {:?}", peer, req.max_blocks, req.hash);

        let mut hash = req.hash;
        let mut block_headers = BlockHeaders::default();

        for _n in 0..cmp::min(MAX_HEADERS_TO_SEND, req.max_blocks) {
            let header = self.graph.block_header_by_hash(&hash);
            if header.is_none() {
                break;
            }
            let header = header.unwrap();
            block_headers.headers.push(header.clone());
            if hash == *self.graph.genesis_hash() {
                break;
            }
            hash = header.parent_hash().clone();
        }
        trace!(target: "sync", "Returned {:?} block headers to peer {:?}", block_headers.headers.len(), peer);

        self.send_message(io, peer, Box::new(block_headers))?;
        Ok(())
    }

    fn get_blocks(
        &self, io: &NetworkContext, syn: &mut SynchronizationState, rlp: &Rlp,
        peer: PeerId,
    ) -> Result<(), Error>
    {
        let req = rlp.as_val::<GetBlocks>()?;
        if req.hashes.is_empty() {
            debug!(target: "sync", "Received empty getblockheaders message: peer={:?}", peer);
        } else {
            self.send_message(
                io,
                peer,
                Box::new(Blocks {
                    blocks: req
                        .hashes
                        .iter()
                        .take(MAX_BLOCKS_TO_SEND as usize)
                        .filter_map(|hash| self.graph.block_by_hash(hash))
                        .collect(),
                }),
            )?;
        }
        Ok(())
    }

    fn get_terminal_block_hashes(
        &self, io: &NetworkContext, syn: &mut SynchronizationState, rlp: &Rlp,
        peer_id: PeerId,
    ) -> Result<(), Error>
    {
        self.send_message(
            io,
            peer_id,
            Box::new(TerminalBlockHashes {
                hashes: self.consensus_graph.terminal_block_hashes(),
            }),
        )?;
        Ok(())
    }

    fn on_terminal_block_hashes(
        &self, io: &NetworkContext, syn: &mut SynchronizationState, rlp: &Rlp,
        peer_id: PeerId,
    ) -> Result<(), Error>
    {
        let terminal_block_hashes = rlp.as_val::<TerminalBlockHashes>()?;
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
            protocol_version: status.protocol_version,
            genesis_hash: status.genesis_hash,
            asking: SynchronizationPeerAsking::Nothing,
            asking_headers: Vec::new(),
            asking_blocks: Vec::new(),
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

    fn on_block_headers(
        &self, io: &NetworkContext, syn: &mut SynchronizationState,
        peer_id: PeerId, rlp: &Rlp,
    ) -> Result<(), Error>
    {
        let block_headers = rlp.as_val::<BlockHeaders>()?;
        if block_headers.headers.is_empty() {
            trace!(target: "sync", "Received empty BlockHeaders message");
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
            if self.graph.contains_block_header(&hash) {
                break;
            } else {
                self.graph.insert_block_header(header.clone());
                hashes.push(hash);
            }
        }

        if !self.graph.contains_block_header(&parent_hash) {
            self.request_block_headers(io, syn, peer_id, &parent_hash, 256);
        }
        if !hashes.is_empty() {
            self.request_blocks(io, syn, peer_id, hashes);
        }

        Ok(())
    }

    fn on_blocks(
        &self, _io: &NetworkContext, _syn: &mut SynchronizationState,
        _peer_id: PeerId, rlp: &Rlp,
    ) -> Result<(), Error>
    {
        let blocks = rlp.as_val::<Blocks>()?;

        for block in blocks.blocks {
            let hash = block.hash();

            if !self.graph.contains_block_header(&hash) {
                self.graph.insert_block_header(block.block_header.clone());
            }
            if !self.graph.contains_block(&hash) {
                self.graph.insert_block(block);
            }
        }

        Ok(())
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
        if !self.graph.contains_block(&hash) {
            self.graph.insert_block(new_block.block.clone());
        }

        if !self.graph.contains_block_header(parent_hash) {
            self.request_block_headers(io, syn, peer_id, parent_hash, 256);
        }

        Ok(())
    }

    fn send_status(
        &self, io: &NetworkContext, peer: PeerId,
    ) -> Result<(), NetworkError> {
        trace!(target: "sync", "Sending status message to {:?}", peer);

        self.send_message(
            io,
            peer,
            Box::new(Status {
                protocol_version: SYNCHRONIZATION_PROTOCOL_VERSION,
                network_id: 0x0,
                genesis_hash: *self.graph.genesis_hash(),
                best_epoch_hash: H256::default(),
            }),
        )
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
            SynchronizationPeerAsking::BlockHeaders,
            Box::new(GetBlockHeaders {
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
            SynchronizationPeerAsking::Blocks,
            Box::new(GetBlocks { hashes }),
        )
    }

    fn send_request(
        &self, io: &NetworkContext, syn: &mut SynchronizationState,
        peer_id: PeerId, asking: SynchronizationPeerAsking,
        msg: Box<dyn Message>,
    )
    {
        if let Some(ref mut peer) = syn.peers.get_mut(&peer_id) {
            if peer.asking != SynchronizationPeerAsking::Nothing {
                warn!(target: "sync", "Requesting {:?} from peer {:?} while asking {:?}", asking, peer_id, peer.asking);
            }
            peer.asking = asking;
            self.send_message(io, peer_id, msg).unwrap();
        }
    }
}

impl NetworkProtocolHandler for SynchronizationProtocolHandler {
    fn initialize(&self, _io: &NetworkContext) {}

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

    fn on_timeout(&self, _io: &NetworkContext, timer: TimerToken) {
        trace!(target: "sync", "Timeout: timer={:?}", timer);
    }
}

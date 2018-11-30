use ethereum_types::H256;
use message::{GetBlockHeaders, GetBlocks, GetTerminalBlockHashes, Message};
use network::PeerId;
use slab::Slab;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
    time::Instant,
};
use sync::synchronization_protocol_handler::TimedSyncRequests;

pub const MAX_INFLIGHT_REQUEST_COUNT: usize = 64;

pub enum RequestMessage {
    Headers(GetBlockHeaders),
    Blocks(GetBlocks),
    Terminals(GetTerminalBlockHashes),
}

impl RequestMessage {
    pub fn set_request_id(&mut self, reqid: u16) {
        match self {
            RequestMessage::Headers(ref mut msg) => msg.set_request_id(reqid),
            RequestMessage::Blocks(ref mut msg) => msg.set_request_id(reqid),
            RequestMessage::Terminals(ref mut msg) => msg.set_request_id(reqid),
        }
    }

    pub fn get_msg(&self) -> &Message {
        match self {
            RequestMessage::Headers(ref msg) => msg,
            RequestMessage::Blocks(ref msg) => msg,
            RequestMessage::Terminals(ref msg) => msg,
        }
    }
}

pub struct SynchronizationPeerRequest {
    pub message: Box<RequestMessage>,
    pub timed_req: Option<Arc<TimedSyncRequests>>,
}

impl SynchronizationPeerRequest {
    pub fn default() -> Self {
        SynchronizationPeerRequest {
            message: Box::new(RequestMessage::Headers(GetBlockHeaders {
                reqid: 0,
                hash: H256::default(),
                max_blocks: 0,
            })),
            timed_req: None,
        }
    }
}

pub struct SynchronizationPeerState {
    pub id: PeerId,
    pub protocol_version: u8,
    pub genesis_hash: H256,
    pub inflight_requests: Slab<SynchronizationPeerRequest>,
    pub pending_requests: VecDeque<SynchronizationPeerRequest>,
    /// Holds a set of transactions recently sent to this peer to avoid
    /// spamming.
    pub last_sent_transactions: HashSet<H256>,
}

impl SynchronizationPeerState {
    /// If new request will be allowed to send, advance the reqid now,
    /// otherwise, actual new reqid will be given to this request
    /// when it is moved from pending to inflight queue.
    pub fn next_request_id(&mut self) -> Option<usize> {
        if self.inflight_requests.len() < self.inflight_requests.capacity() {
            let reqid = self
                .inflight_requests
                .insert(SynchronizationPeerRequest::default());
            assert!(reqid < MAX_INFLIGHT_REQUEST_COUNT);
            return Some(reqid);
        }
        None
    }

    pub fn append_inflight_request(
        &mut self, reqid: usize, msg: Box<RequestMessage>,
        timed_req: Arc<TimedSyncRequests>,
    )
    {
        let slot = self.inflight_requests.get_mut(reqid).unwrap();
        slot.message = msg;
        slot.timed_req = Some(timed_req);
    }

    pub fn append_pending_request(&mut self, msg: Box<RequestMessage>) {
        self.pending_requests.push_back(SynchronizationPeerRequest {
            message: msg,
            timed_req: None,
        });
    }

    pub fn is_inflight_request(&self, reqid: usize) -> bool {
        self.inflight_requests.contains(reqid)
    }

    pub fn has_pending_requests(&self) -> bool {
        !self.pending_requests.is_empty()
    }

    pub fn pop_pending_request(
        &mut self,
    ) -> Option<SynchronizationPeerRequest> {
        self.pending_requests.pop_front()
    }

    pub fn remove_inflight_request(&mut self, reqid: usize) {
        self.inflight_requests.remove(reqid);
    }
}

pub type SynchronizationPeers = HashMap<PeerId, SynchronizationPeerState>;

pub struct SynchronizationState {
    pub peers: SynchronizationPeers,
    pub handshaking_peers: HashMap<PeerId, Instant>,
}

impl SynchronizationState {
    pub fn new() -> Self {
        SynchronizationState {
            peers: HashMap::new(),
            handshaking_peers: HashMap::new(),
        }
    }
}

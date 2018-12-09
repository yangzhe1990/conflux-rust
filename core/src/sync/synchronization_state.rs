use ethereum_types::H256;
use message::{GetBlockHeaders, GetBlocks, GetTerminalBlockHashes, Message};
use network::PeerId;
use slab::Slab;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
    time::Instant,
    mem,
};
use sync::synchronization_protocol_handler::TimedSyncRequests;

pub const MAX_INFLIGHT_REQUEST_COUNT: usize = 64;

pub enum RequestMessage {
    Headers(GetBlockHeaders),
    Blocks(GetBlocks),
    Terminals(GetTerminalBlockHashes),
}

impl RequestMessage {
    pub fn set_request_id(&mut self, request_id: u16) {
        match self {
            RequestMessage::Headers(ref mut msg) => {
                msg.set_request_id(request_id)
            }
            RequestMessage::Blocks(ref mut msg) => {
                msg.set_request_id(request_id)
            }
            RequestMessage::Terminals(ref mut msg) => {
                msg.set_request_id(request_id)
            }
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
                request_id: 0.into(),
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
    /// If new request will be allowed to send, advance the request id now,
    /// otherwise, actual new request id will be given to this request
    /// when it is moved from pending to inflight queue.
    pub fn next_request_id(&mut self) -> Option<usize> {
        if self.inflight_requests.len() < self.inflight_requests.capacity() {
            let request_id = self
                .inflight_requests
                .insert(SynchronizationPeerRequest::default());
            assert!(request_id < MAX_INFLIGHT_REQUEST_COUNT);
            Some(request_id)
        } else {
            None
        }
    }

    pub fn append_inflight_request(
        &mut self, request_id: usize, msg: Box<RequestMessage>,
        timed_req: Arc<TimedSyncRequests>,
    ) -> RequestMessage
    {
        let slot = self.inflight_requests.get_mut(request_id).unwrap();
        let req = mem::replace(&mut slot.message, msg);
        slot.timed_req = Some(timed_req);
        *req
    }

    pub fn append_pending_request(&mut self, msg: Box<RequestMessage>) {
        self.pending_requests.push_back(SynchronizationPeerRequest {
            message: msg,
            timed_req: None,
        });
    }

    pub fn is_inflight_request(&self, request_id: u16) -> bool {
        self.inflight_requests.contains(request_id as usize)
    }

    pub fn has_pending_requests(&self) -> bool {
        !self.pending_requests.is_empty()
    }

    pub fn pop_pending_request(
        &mut self,
    ) -> Option<SynchronizationPeerRequest> {
        self.pending_requests.pop_front()
    }

    pub fn remove_inflight_request(&mut self, request_id: usize) -> SynchronizationPeerRequest {
        self.inflight_requests.remove(request_id)
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

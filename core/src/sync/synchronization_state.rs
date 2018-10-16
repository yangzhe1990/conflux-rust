use ethereum_types::H256;
use network::PeerId;
use std::{collections::HashMap, time::Instant};

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum SynchronizationPeerAsking {
    Nothing,
    BlockHeaders,
    Blocks,
}

#[derive(Clone)]
pub struct SynchronizationPeerState {
    pub protocol_version: u8,
    pub genesis_hash: H256,
    pub asking: SynchronizationPeerAsking,
    pub asking_headers: Vec<H256>,
    pub asking_blocks: Vec<H256>,
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

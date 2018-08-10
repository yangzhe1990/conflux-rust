mod sync_handler;
mod sync_requester;

use self::sync_handler::SyncHandler;
use self::sync_requester::SyncRequester;
use super::PacketId;
use ethereum_types::{H256, U256};
use network::{Error, PeerId};
use parking_lot::RwLock;
use rlp::{DecoderError, Rlp, RlpStream};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use sync_ctx::SyncIo;

pub const CONFLUX_PROTOCOL_VERSION_1: u8 = 0x01;

pub const MAX_HEADERS_TO_SEND: usize = 512;

const STATUS_PACKET: u8 = 0x00;
pub const GET_BLOCK_HEADERS_PACKET: u8 = 0x01;
pub const GET_BLOCK_BODIES_PACKET: u8 = 0x02;
pub const BLOCK_HEADERS_PACKET: u8 = 0x03;

pub type PacketDecodeError = DecoderError;
pub type RlpResponseResult = Result<Option<RlpStream>, PacketDecodeError>;

#[derive(PartialEq, Eq, Debug, Clone)]
/// Peer data type requested
pub enum PeerAsking {
    Nothing,
    ForkHeader,
    BlockHeaders,
    BlockBodies,
    BlockReceipts,
    SnapshotManifest,
    SnapshotData,
}

#[derive(Clone)]
/// Syncing peer information
pub struct PeerInfo {
    /// eth protocol version
    protocol_version: u8,
    /// Peer chain genesis hash
    genesis: H256,
    /// Peer best block hash
    latest_hash: H256,
    /// Peer total difficulty if known
    difficulty: Option<U256>,
    /// Type of data currenty being requested from peer.
    asking: PeerAsking,
    /// A set of block numbers being requested
    asking_blocks: Vec<H256>,
    /// Holds requested header hash if currently requesting block header by hash
    asking_hash: Option<H256>,
    /// Request timestamp
    ask_time: Instant,
    /// Pending request is expired and result should be ignored
    expired: bool,
}

pub type Peers = HashMap<PeerId, PeerInfo>;

/// Conflux DAG sync handler.
pub struct SyncState {
    /// All connected peers
    peers: Peers,
    /// Peers active for current sync round
    active_peers: HashSet<PeerId>,
    /// Sync start timestamp. Measured when first peer is connected
    sync_start_time: Option<Instant>,
    /// The max total difficulty that has been seen
    max_seen_total_difficulty: U256,

    /// Connected peers pending Status message.
    /// Value is request timestamp.
    handshaking_peers: HashMap<PeerId, Instant>,

    headers_in_fetching: HashMap<H256, PeerId>,
    bodies_in_fetching: HashMap<H256, PeerId>,
}

impl SyncState {
    /// Create a new instance of syncing strategy.
    pub fn new() -> Self {
        let sync = SyncState {
            peers: HashMap::new(),
            active_peers: HashSet::new(),
            sync_start_time: None,
            max_seen_total_difficulty: Default::default(),
            handshaking_peers: HashMap::new(),
            headers_in_fetching: HashMap::new(),
            bodies_in_fetching: HashMap::new(),
        };

        sync
    }

    /// Dispatch incoming requests and responses
    pub fn dispatch_packet(sync: &RwLock<SyncState>, io: &mut SyncIo, peer: PeerId, packet_id: PacketId, rlp: Rlp) {
        SyncHandler::dispatch_packet(sync, io, peer, packet_id, rlp)
    }

    /// Handle incoming packet from peer which does not require response
    /// Require write lock on SyncState
    pub fn on_packet(&mut self, io: &mut SyncIo, peer: PeerId, packet_id: PacketId, rlp: &Rlp) {
        debug!(target: "sync", "{} -> Dispatching packet: {}", peer, packet_id);
        SyncHandler::on_packet(self, io, peer, packet_id, rlp);
    }

    /// Called when a new peer is connected
    pub fn on_peer_connected(&mut self, io: &mut SyncIo, peer: PeerId) {
        SyncHandler::on_peer_connected(self, io, peer);
    }

    /// Send Status message
    fn send_status(
        &mut self, io: &mut SyncIo, peer: PeerId,
    ) -> Result<(), Error> {
        let protocol = CONFLUX_PROTOCOL_VERSION_1;
        trace!(target: "sync", "Sending status to {}, protocol version {}", peer, protocol);
        let ledger = io.ledger().ledger_info();
        let mut packet = RlpStream::new_list(5);
        packet.append(&(STATUS_PACKET as u32));
        packet.append(&(protocol as u32));
        packet.append(&ledger.total_difficulty);
        packet.append(&ledger.best_block_hash);
        packet.append(&ledger.genesis_hash);

        io.send(peer, packet.out())
    }

    /// Remove peer from active peer set. Peer will be reactivated on the next sync round
    fn deactivate_peer(&mut self, _io: &mut SyncIo, peer_id: PeerId) {
        trace!(target: "sync", "Deactivating peer {}", peer_id);
        self.active_peers.remove(&peer_id);
    }

    fn peer_status_changed(&mut self, io: &mut SyncIo, peer_id: PeerId) {
        let (peer_latest, peer_difficulty) = {
            match self.peers.get(&peer_id){
			    Some(p) => {
				    (p.latest_hash.clone(), p.difficulty.clone())
                },
                _ => {
                    return;
                },
            }
		};
        let higher_difficulty = peer_difficulty.map_or(false, |pd| pd > self.max_seen_total_difficulty);
        if !higher_difficulty {
            return;
        }

        match peer_difficulty {
            Some(difficulty) => { self.max_seen_total_difficulty = difficulty},
            None => {}
        }
        
        if !io.ledger().block_header_exists(&peer_latest) {
            if !self.headers_in_fetching.contains_key(&peer_latest) {
                self.headers_in_fetching.insert(peer_latest, peer_id);
                SyncRequester::request_headers_by_hash(self, io, peer_id, &peer_latest, 256, 0, true);
            }
        }        
    }
}

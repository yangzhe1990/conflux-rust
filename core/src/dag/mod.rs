mod sync_handler;

use super::{PacketId};
use self::sync_handler::SyncHandler;
use parking_lot::RwLock;
use sync_ctx::SyncIo;
use network::{PeerId, Error};
use rlp::{Rlp, RlpStream, DecoderError};
use std::time::{Duration, Instant};
use std::collections::{HashMap, HashSet};
use ethereum_types::{H256, U256};

pub const CONFLUX_PROTOCOL_VERSION_1: u8 = 0x01;

pub const MAX_HEADERS_TO_SEND: usize = 512;

const STATUS_PACKET: u8 = 0x00;
pub const GET_BLOCK_HEADERS_PACKET: u8 = 0x01;
pub const GET_BLOCK_BODIES_PACKET: u8 = 0x02;
pub const BLOCK_HEADERS_PACKET: u8 = 0x03;

pub type PacketDecodeError = DecoderError;
pub type RlpResponseResult = Result<Option<(PacketId, RlpStream)>, PacketDecodeError>;

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
pub struct DagSync {
    /// All connected peers
    peers: Peers,
    /// Peers active for current sync round
    active_peers: HashSet<PeerId>,
    /// Sync start timestamp. Measured when first peer is connected
    sync_start_time: Option<Instant>,

    /// Connected peers pending Status message.
    /// Value is request timestamp.
    handshaking_peers: HashMap<PeerId, Instant>,
}

impl DagSync {
    /// Create a new instance of syncing strategy.
    pub fn new() -> Self {
        let sync = DagSync {
            peers: HashMap::new(),
            active_peers: HashSet::new(),
            sync_start_time: None,
            handshaking_peers: HashMap::new(),
        };

        sync
    }

    /// Dispatch incoming requests and responses
    pub fn dispatch_packet(sync: &RwLock<DagSync>, io: &mut SyncIo, peer: PeerId, packet_id: PacketId, data: &[u8]) {
        SyncHandler::dispatch_packet(sync, io, peer, packet_id, data)
    }

    /// Handle incoming packet from peer which does not require response
    /// Require write lock on DagSync
    pub fn on_packet(&mut self, io: &mut SyncIo, peer: PeerId, packet_id: PacketId, data: &[u8]) {
        debug!(target: "sync", "{} -> Dispatching packet: {}", peer, packet_id);
        SyncHandler::on_packet(self, io, peer, packet_id, data);
    }

    /// Called when a new peer is connected
    pub fn on_peer_connected(&mut self, io: &mut SyncIo, peer: PeerId) {
        SyncHandler::on_peer_connected(self, io, peer);
    }

    /// Send Status message
    fn send_status(&mut self, io: &mut SyncIo, peer: PeerId) -> Result<(), Error> {
        let protocol = CONFLUX_PROTOCOL_VERSION_1;
        trace!(target: "sync", "Sending status to {}, protocol version {}", peer, protocol);
        let ledger = io.ledger().ledger_info();
        let mut packet = RlpStream::new_list(4);
        packet.append(&(protocol as u32));
        packet.append(&ledger.total_difficulty);
        packet.append(&ledger.best_block_hash);
        packet.append(&ledger.genesis_hash);

        io.respond(STATUS_PACKET, packet.out())
    }

    /// Remove peer from active peer set. Peer will be reactivated on the next sync round
    fn deactivate_peer(&mut self, _io: &mut SyncIo, peer_id: PeerId) {
        trace!(target: "sync", "Deactivating peer {}", peer_id);
        self.active_peers.remove(&peer_id);
    }

    /// Find something to do for a peer. Called for a new peer or when a peer is done with its task.
    fn sync_peer(&mut self, io: &mut SyncIo, peer_id: PeerId, force: bool) {
    }

    /// Resume downloading
    fn continue_sync(&mut self, io: &mut SyncIo) {
    }
}

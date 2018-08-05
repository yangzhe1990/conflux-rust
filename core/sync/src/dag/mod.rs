mod sync_handler;

use super::{PacketId};
use self::sync_handler::SyncHandler;
use parking_lot::RwLock;
use sync_ctx::SyncIo;
use network::{PeerId, Error};
use rlp::{Rlp, RlpStream, DecoderError};
use std::time::{Duration, Instant};
use std::collections::HashMap;

pub const CONFLUX_PROTOCOL_VERSION_1: u8 = 0x01;

pub const MAX_HEADERS_TO_SEND: usize = 512;

const STATUS_PACKET: u8 = 0x00;
pub const GET_BLOCK_HEADERS_PACKET: u8 = 0x01;
pub const GET_BLOCK_BODIES_PACKET: u8 = 0x02;
pub const BLOCK_HEADERS_PACKET: u8 = 0x03;

pub type PacketDecodeError = DecoderError;
pub type RlpResponseResult = Result<Option<(PacketId, RlpStream)>, PacketDecodeError>;

#[derive(Clone)]
/// Syncing peer information
pub struct PeerInfo {
}

pub type Peers = HashMap<PeerId, PeerInfo>;

/// Conflux DAG sync handler.
pub struct DagSync {
    /// All connected peers
    peers: Peers,

    /// Connected peers pending Status message.
    /// Value is request timestamp.
    handshaking_peers: HashMap<PeerId, Instant>,
}

impl DagSync {
    /// Create a new instance of syncing strategy.
    pub fn new() -> DagSync {
        let sync = DagSync {
            peers: HashMap::new(),
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
}

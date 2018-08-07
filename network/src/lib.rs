extern crate io;
#[macro_use]
extern crate log;
extern crate mio;
extern crate parking_lot;
extern crate slab;
#[macro_use]
extern crate error_chain;
extern crate bytes;
extern crate rlp;

pub type ProtocolId = [u8; 3];
pub type PeerId = usize;
pub type NodeId = SocketAddr;

mod connection;
mod error;
mod service;
mod session;

pub use error::{DisconnectReason, Error, ErrorKind};
pub use io::TimerToken;
pub use service::NetworkService;

use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use std::cmp::Ordering;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub struct NetworkConfiguration {
    pub listen_address: Option<SocketAddr>,
    pub boot_nodes: Vec<String>,
}

impl Default for NetworkConfiguration {
    fn default() -> Self { NetworkConfiguration::new() }
}

impl NetworkConfiguration {
    pub fn new() -> Self {
        NetworkConfiguration {
            listen_address: None,
            boot_nodes: Vec::new(),
        }
    }

    pub fn new_with_port(port: u16) -> NetworkConfiguration {
        let mut config = NetworkConfiguration::new();
        config.listen_address = Some(SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::new(0, 0, 0, 0),
            port,
        )));
        config
    }

    pub fn new_local() -> NetworkConfiguration {
        let mut config = NetworkConfiguration::new();
        config.listen_address = Some(SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::new(127, 0, 0, 1),
            0,
        )));
        config
    }
}

#[derive(Clone)]
pub enum NetworkIoMessage {
    Start,
    AddHandler {
        handler: Arc<NetworkProtocolHandler + Sync>,
        protocol: ProtocolId,
        versions: Vec<u8>,
    },
    /// Disconnect a peer.
    Disconnect(PeerId),
}

pub trait NetworkProtocolHandler: Sync + Send {
    fn initialize(&self, io: &NetworkContext) {}

    fn on_message(&self, io: &NetworkContext, peer: PeerId, data: &[u8]);

    fn on_peer_connected(&self, io: &NetworkContext, peer: PeerId);

    fn on_peer_disconnected(&self, io: &NetworkContext, peer: PeerId);

    fn on_timeout(&self, io: &NetworkContext, timer: TimerToken);
}

pub trait NetworkContext {
    fn send(&self, peer: PeerId, msg: Vec<u8>) -> Result<(), Error>;

    fn disconnect_peer(&self, peer: PeerId);
}

#[derive(Debug, Clone)]
pub struct SessionMetadata {
    pub id: Option<NodeId>,
    pub capabilities: Vec<Capability>,
    pub peer_capabilities: Vec<Capability>,
    pub originated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capability {
    pub protocol: ProtocolId,
    pub version: u8,
}

impl Encodable for Capability {
    fn rlp_append(&self, rlp: &mut RlpStream) {
        rlp.begin_list(2);
        rlp.append(&&self.protocol[..]);
        rlp.append(&self.version);
    }
}

impl Decodable for Capability {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        let p: Vec<u8> = rlp.val_at(0)?;
        if p.len() != 3 {
            return Err(DecoderError::Custom(
                "Invalid subprotocol string length",
            ));
        }
        let mut protocol: ProtocolId = [0u8; 3];
        protocol.clone_from_slice(&p);
        Ok(Capability {
            protocol: protocol,
            version: rlp.val_at(1)?,
        })
    }
}

impl PartialOrd for Capability {
    fn partial_cmp(&self, other: &Capability) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Capability {
    fn cmp(&self, other: &Capability) -> Ordering {
        return self.protocol.cmp(&other.protocol);
    }
}

extern crate ethcore_io as io;
#[macro_use]
extern crate log;
extern crate mio;
extern crate parking_lot;
extern crate slab;
#[macro_use]
extern crate error_chain;
extern crate bytes;

pub type MsgId = u8;
pub type ProtocolId = [u8; 3];
pub type PeerId = usize;

mod connection;
mod error;
mod service;
mod session;

pub use error::{DisconnectReason, Error, ErrorKind};
pub use io::TimerToken;
pub use service::NetworkService;

use bytes::Buf;
use std::cmp::Ordering;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub struct NetworkConfiguration {
    listen_address: Option<SocketAddr>,
}

impl Default for NetworkConfiguration {
    fn default() -> Self { NetworkConfiguration::new() }
}

impl NetworkConfiguration {
    pub fn new() -> Self {
        NetworkConfiguration {
            listen_address: None,
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
    },
}

pub trait NetworkProtocolHandler: Sync + Send {
    fn initialize(&self, io: &NetworkContext) {}

    fn on_message(
        &self, io: &NetworkContext, peer: PeerId, msg_id: u8, data: &[u8],
    );

    fn on_peer_connected(&self, io: &NetworkContext, peer: PeerId);

    fn on_peer_disconnected(&self, io: &NetworkContext, peer: PeerId);

    fn on_timeout(&self, io: &NetworkContext, timer: TimerToken);
}

pub trait NetworkContext {
    fn send(
        &self, peer: PeerId, msg_id: MsgId, data: Vec<u8>,
    ) -> Result<(), Error>;

    fn disconnect_peer(&self, peer: PeerId);
}

#[derive(Debug, Clone)]
pub struct SessionMetadata {
    pub capabilities: Vec<SessionCapability>,
    pub peer_capabilities: Vec<PeerCapability>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerCapability {
    pub protocol: ProtocolId,
    pub version: u8,
}

impl PeerCapability {
    fn decode(buf: &mut Buf) -> PeerCapability {
        PeerCapability {
            protocol: [buf.get_u8(), buf.get_u8(), buf.get_u8()],
            version: buf.get_u8(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCapability {
    pub protocol: ProtocolId,
    pub version: u8,
    pub packet_count: u8,
    pub id_offset: u8,
}

impl PartialOrd for SessionCapability {
    fn partial_cmp(&self, other: &SessionCapability) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SessionCapability {
    fn cmp(&self, other: &SessionCapability) -> Ordering {
        return self.protocol.cmp(&other.protocol);
    }
}

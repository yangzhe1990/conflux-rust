extern crate ethcore_io as io;
#[macro_use]
extern crate log;
extern crate mio;
extern crate parking_lot;
extern crate slab;
#[macro_use]
extern crate error_chain;
extern crate bytes;

pub type ProtocolId = [u8; 3];
pub type PeerId = usize;

mod connection;
mod error;
mod service;
mod session;

pub use error::{DisconnectReason, Error, ErrorKind};
pub use io::TimerToken;
pub use service::NetworkService;

use bytes::{Buf, BufMut};
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
    pub capabilities: Vec<Capability>,
    pub peer_capabilities: Vec<Capability>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capability {
    pub protocol: ProtocolId,
    pub version: u8,
}

impl Capability {
    fn decode(buf: &mut Buf) -> Capability {
        Capability {
            protocol: [buf.get_u8(), buf.get_u8(), buf.get_u8()],
            version: buf.get_u8(),
        }
    }

    pub fn encode(&self, buf: &mut BufMut) {
        buf.put_slice(&self.protocol[..]);
        buf.put_u8(self.version);
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

mod tests {
    use super::*;

    struct TestNetworkProtocolHandler;

    impl NetworkProtocolHandler for TestNetworkProtocolHandler {
        fn on_message(&self, io: &NetworkContext, peer: PeerId, data: &[u8]) {}

        fn on_peer_connected(&self, io: &NetworkContext, peer: PeerId) {}

        fn on_peer_disconnected(&self, io: &NetworkContext, peer: PeerId) {}

        fn on_timeout(&self, io: &NetworkContext, timer: TimerToken) {}
    }

    #[test]
    fn test_basic() {
    }
}


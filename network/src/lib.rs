#![allow(deprecated)]
extern crate io;
#[macro_use]
extern crate log;
extern crate mio;
extern crate parking_lot;
extern crate slab;
#[macro_use]
extern crate error_chain;
extern crate bytes;
extern crate ipnetwork;
extern crate rlp;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate ethereum_types;
extern crate ethkey;
extern crate igd;
extern crate libc;
extern crate parity_bytes;
extern crate parity_path;

pub type ProtocolId = [u8; 3];
pub type PeerId = usize;
pub type NodeId = SocketAddr;

mod connection;
mod discovery;
mod error;
mod ip_utils;
mod node_table;
mod service;
mod session;

pub use error::{DisconnectReason, Error, ErrorKind};
pub use io::TimerToken;
pub use service::NetworkService;

use ethkey::Secret;
use ipnetwork::{IpNetwork, IpNetworkError};
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use std::cmp::Ordering;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::str::{self, FromStr};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub struct NetworkConfiguration {
    /// Directory path to store general network configuration. None means nothing will be saved
    pub config_path: Option<String>,
    pub listen_address: Option<SocketAddr>,
    /// IP address to advertise. Detected automatically if none.
    pub public_address: Option<SocketAddr>,
    pub udp_port: Option<u16>,
    /// Enable NAT configuration
    pub nat_enabled: bool,
    /// Enable discovery
    pub discovery_enabled: bool,
    pub boot_nodes: Vec<String>,
    /// Use provided node key instead of default
    pub use_secret: Option<Secret>,
    /// IP filter
    pub ip_filter: IpFilter,
}

impl Default for NetworkConfiguration {
    fn default() -> Self { NetworkConfiguration::new() }
}

impl NetworkConfiguration {
    pub fn new() -> Self {
        NetworkConfiguration {
            config_path: Some("./config".to_string()),
            listen_address: None,
            public_address: None,
            udp_port: None,
            nat_enabled: true,
            discovery_enabled: true,
            boot_nodes: Vec::new(),
            use_secret: None,
            ip_filter: IpFilter::default(),
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
    fn initialize(&self, _io: &NetworkContext) {}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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

#[derive(Serialize)]
pub struct PeerInfo {
    pub id: PeerId,
    pub addr: SocketAddr,
    pub caps: Vec<Capability>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IpFilter {
    pub predefined: AllowIP,
    pub custom_allow: Vec<IpNetwork>,
    pub custom_block: Vec<IpNetwork>,
}

impl Default for IpFilter {
    fn default() -> Self {
        IpFilter {
            predefined: AllowIP::All,
            custom_allow: vec![],
            custom_block: vec![],
        }
    }
}

impl IpFilter {
    /// Attempt to parse the peer mode from a string.
    pub fn parse(s: &str) -> Result<IpFilter, IpNetworkError> {
        let mut filter = IpFilter::default();
        for f in s.split_whitespace() {
            match f {
                "all" => filter.predefined = AllowIP::All,
                "private" => filter.predefined = AllowIP::Private,
                "public" => filter.predefined = AllowIP::Public,
                "none" => filter.predefined = AllowIP::None,
                custom => {
                    if custom.starts_with("-") {
                        filter.custom_block.push(IpNetwork::from_str(
                            &custom.to_owned().split_off(1),
                        )?)
                    } else {
                        filter.custom_allow.push(IpNetwork::from_str(custom)?)
                    }
                }
            }
        }
        Ok(filter)
    }
}

/// IP fiter
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AllowIP {
    /// Connect to any address
    All,
    /// Connect to private network only
    Private,
    /// Connect to public network only
    Public,
    /// Block all addresses
    None,
}

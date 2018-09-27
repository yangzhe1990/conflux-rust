use ethkey::{recover, sign, KeyPair, Secret};
use std::net::SocketAddr;
use node_table::*;
use service::UdpIOContext;
use {Error};
use IpFilter;

pub struct Discovery {}

impl Discovery {
    pub fn new(
        key: &KeyPair, public: NodeEndpoint, ip_filter: IpFilter,
    ) -> Discovery {
        Discovery {}
    }

    pub fn add_node_list(&mut self, uio: &UdpIOContext) {

    }

    pub fn on_packet(&mut self, packet: &[u8], from: SocketAddr) -> Result<(), Error> {
        Ok(())
    }
}

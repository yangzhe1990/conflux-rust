use ethkey::{recover, sign, KeyPair, Secret};
use node_table::*;
use service::UdpIOContext;
use IpFilter;

pub struct Discovery {}

impl Discovery {
    pub fn new(
        key: &KeyPair, public: NodeEndpoint, ip_filter: IpFilter,
    ) -> Discovery {
        Discovery {}
    }

    pub fn add_node_list(&mut self, uio: &UdpIOContext) {}
}

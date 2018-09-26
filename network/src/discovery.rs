use ethkey::{Secret, KeyPair, sign, recover};
use node_table::*;
use {IpFilter};

pub struct Discovery {
}

impl Discovery {
    pub fn new(key: &KeyPair, public: NodeEndpoint, ip_filter: IpFilter) -> Discovery {
		Discovery {
		}
	}
}

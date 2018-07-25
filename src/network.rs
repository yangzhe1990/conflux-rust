use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

#[derive(Debug, Clone, PartialEq)]
pub struct NetworkConfiguration {
    pub listen_address: Option<SocketAddr>,
    pub port: Option<u16>,
}

impl Default for NetworkConfiguration {
    fn default() -> Self {
        NetworkConfiguration::new()
    }
}

impl NetworkConfiguration {
    pub fn new() -> Self {
        NetworkConfiguration {
            listen_address: None,
            port: None,
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

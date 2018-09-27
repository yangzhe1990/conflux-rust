use ethereum_types::H512;
use ip_utils::*;
use rlp::{DecoderError, Rlp, RlpStream};
use std::net::{
    Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6, ToSocketAddrs,
};
use std::slice;
use std::str::FromStr;
use {AllowIP, IpFilter};
use {Error, ErrorKind};

/// Node public key
pub type NodeId = H512;

#[derive(Debug, Clone, PartialEq)]
/// Node address info
pub struct NodeEndpoint {
    /// IP(V4 or V6) address
    pub address: SocketAddr,
    /// Connection port.
    pub udp_port: u16,
}

impl NodeEndpoint {
    pub fn udp_address(&self) -> SocketAddr {
        match self.address {
            SocketAddr::V4(a) => {
                SocketAddr::V4(SocketAddrV4::new(*a.ip(), self.udp_port))
            }
            SocketAddr::V6(a) => SocketAddr::V6(SocketAddrV6::new(
                *a.ip(),
                self.udp_port,
                a.flowinfo(),
                a.scope_id(),
            )),
        }
    }

    pub fn is_allowed(&self, filter: &IpFilter) -> bool {
        (self.is_allowed_by_predefined(&filter.predefined) || filter
            .custom_allow
            .iter()
            .any(|ipnet| self.address.ip().is_within(ipnet)))
            && !filter
                .custom_block
                .iter()
                .any(|ipnet| self.address.ip().is_within(ipnet))
    }

    pub fn is_allowed_by_predefined(&self, filter: &AllowIP) -> bool {
        match filter {
            AllowIP::All => true,
            AllowIP::Private => self.address.ip().is_usable_private(),
            AllowIP::Public => self.address.ip().is_usable_public(),
            AllowIP::None => false,
        }
    }

    pub fn from_rlp(rlp: &Rlp) -> Result<Self, DecoderError> {
        let tcp_port = rlp.val_at::<u16>(2)?;
        let udp_port = rlp.val_at::<u16>(1)?;
        let addr_bytes = rlp.at(0)?.data()?;
        let address = match addr_bytes.len() {
            4 => Ok(SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::new(
                    addr_bytes[0],
                    addr_bytes[1],
                    addr_bytes[2],
                    addr_bytes[3],
                ),
                tcp_port,
            ))),
            16 => unsafe {
                let o: *const u16 = addr_bytes.as_ptr() as *const u16;
                let o = slice::from_raw_parts(o, 8);
                Ok(SocketAddr::V6(SocketAddrV6::new(
                    Ipv6Addr::new(
                        o[0], o[1], o[2], o[3], o[4], o[5], o[6], o[7],
                    ),
                    tcp_port,
                    0,
                    0,
                )))
            },
            _ => Err(DecoderError::RlpInconsistentLengthAndData),
        }?;
        Ok(NodeEndpoint { address, udp_port })
    }

    pub fn to_rlp(&self, rlp: &mut RlpStream) {
        match self.address {
            SocketAddr::V4(a) => {
                rlp.append(&(&a.ip().octets()[..]));
            }
            SocketAddr::V6(a) => unsafe {
                let o: *const u8 = a.ip().segments().as_ptr() as *const u8;
                rlp.append(&slice::from_raw_parts(o, 16));
            },
        };
        rlp.append(&self.udp_port);
        rlp.append(&self.address.port());
    }

    pub fn to_rlp_list(&self, rlp: &mut RlpStream) {
        rlp.begin_list(3);
        self.to_rlp(rlp);
    }

    /// Validates that the port is not 0 and address IP is specified
    pub fn is_valid(&self) -> bool {
        self.udp_port != 0 && self.address.port() != 0 && match self.address {
            SocketAddr::V4(a) => !a.ip().is_unspecified(),
            SocketAddr::V6(a) => !a.ip().is_unspecified(),
        }
    }
}

impl FromStr for NodeEndpoint {
    type Err = Error;

    /// Create endpoint from string. Performs name resolution if given a host name.
    fn from_str(s: &str) -> Result<NodeEndpoint, Error> {
        let address = s.to_socket_addrs().map(|mut i| i.next());
        match address {
            Ok(Some(a)) => Ok(NodeEndpoint {
                address: a,
                udp_port: a.port(),
            }),
            Ok(None) => bail!(ErrorKind::AddressResolve(None)),
            Err(_) => Err(ErrorKind::AddressParse.into()), // always an io::Error of InvalidInput kind
        }
    }
}

use jsonrpc_core::{IoHandler, Value};
use parity_reactor::TokioRemote;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use tcp::ServerBuilder as TcpServerBuilder;

pub use tcp::Server as TcpServer;

pub struct Dependencies {
    pub remote: TokioRemote,
}

#[derive(Debug, PartialEq)]
pub struct TcpConfiguration {
    pub enabled: bool,
    pub socket_addr: SocketAddr,
}

impl TcpConfiguration {
    pub fn with_port(port: u16) -> Self {
        TcpConfiguration {
            enabled: true,
            socket_addr: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), port)),
        }
    }
}

pub fn new_tcp(
    conf: TcpConfiguration,
    dependencies: &Dependencies,
) -> Result<Option<TcpServer>, String> {
    if !conf.enabled {
        return Ok(None);
    }

    let mut handler = IoHandler::new();
    handler.add_method("say_hello", |_| Ok(Value::String("Hello, world!".into())));

    let remote = dependencies.remote.clone();

    match TcpServerBuilder::new(handler)
        .event_loop_remote(remote)
        .start(&conf.socket_addr)
    {
        Ok(server) => Ok(Some(server)),
        Err(io_error) => Err(format!("TCP error: {}", io_error)),
    }
}

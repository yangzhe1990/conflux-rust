use jsonrpc_core::{IoHandler, Value, Params, Error};
use parity_reactor::TokioRemote;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use tcp::ServerBuilder as TcpServerBuilder;

pub use tcp::Server as TcpServer;

const DEFAULT_TCP_PORT: u16 = 32324;

pub struct Dependencies {
    pub remote: TokioRemote,
}

#[derive(Debug, PartialEq)]
pub struct TcpConfiguration {
    pub enabled: bool,
    pub socket_addr: SocketAddr,
}

impl TcpConfiguration {
    pub fn new(port: Option<u16>) -> Self {
        TcpConfiguration {
            enabled: true,
            socket_addr: SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::new(0, 0, 0, 0),
                port.unwrap_or(DEFAULT_TCP_PORT),
            )),
        }
    }
}

macro_rules! parse_u64 {
    ($a:expr, $k:expr, $m:expr) => {
        {
            let x = $a.get($k);
            if x == None {
                return Err(Error::invalid_params(format!("{} {} {}", $k, "not specified in", $m)));
            }
            let y = x.unwrap().as_u64();
            if y == None {
                return Err(Error::invalid_params(format!("{} {} {}", $k, "must be integer in", $m)));
            }
            y.unwrap()
        }
    };
}

macro_rules! parse_string {
    ($a:expr, $k:expr, $m:expr) => {
        {
            let x = $a.get($k);
            if x == None {
                return Err(Error::invalid_params(format!("{} {} {}", $k, "not specified in", $m)));
            }
            let y = x.unwrap().as_string();
            if y == None {
                return Err(Error::invalid_params(format!("{} {} {}", $k, "must be string in", $m)));
            }
            y.unwrap()
        }
    };
}


pub fn new_tcp(
    conf: TcpConfiguration, dependencies: &Dependencies,
) -> Result<Option<TcpServer>, String> {
    if !conf.enabled {
        return Ok(None);
    }

    let mut handler = IoHandler::new();
    handler.add_method("say_hello", |_| Ok(Value::String("Hello, world!".into())));
    // make_active makes the client to enter the active synchronization mode.
    handler.add_method("make_active", |_| {
        unimplemented!();
        Ok(Value::String("Not implemented!".into()))
    });
    // make_passive makes the client to enter the passive synchronization mode. Passive mode is
    // mainly used for debugging.
    handler.add_method("make_passive", |_| {
        unimplemented!();
        Ok(Value::String("Not implemented!".into()))
    });
    // generate_blocks makes the client to generate one or more blocks. 
    handler.add_method("generate_blocks", |_param: Params| {
        match _param {
            Params::Map(m) => {
                let num = parse_u64!(m, "num", "generate_blocks");
                Ok(Value::String(format!("{}{}", "generate_blocks get ", num)))
            }
            _ => Err(Error::invalid_params("Invalid format for \"generate_blocks\""))
        }
    });
    handler.add_method("add_peer", |_param: Params| {
        unimplemented!();
        Ok(Value::String("Not implemented!".into()))
    });
    handler.add_method("drop_peer", |_param: Params| {
        unimplemented!();
        Ok(Value::String("Not implemented!".into()))
    });
    handler.add_method("sync_status", |_param: Params| {
        unimplemented!();
        Ok(Value::String("Not implemented!".into()))
    });

    let remote = dependencies.remote.clone();

    match TcpServerBuilder::new(handler)
        .event_loop_remote(remote)
        .start(&conf.socket_addr)
    {
        Ok(server) => Ok(Some(server)),
        Err(io_error) => Err(format!("TCP error: {}", io_error)),
    }
}

use std::path::PathBuf;

use helpers::conflux_ipc_path;
use ipc::ServerBuilder as IpcServerBuilder;
use jsonrpc_core::{IoHandler, Value};
use parity_reactor::TokioRemote;
use std::net::SocketAddr;
use tcp::ServerBuilder as TcpServerBuilder;

pub use ipc::Server as IpcServer;
pub use tcp::Server as TcpServer;

pub struct Dependencies {
    pub remote: TokioRemote,
}

#[derive(Debug, PartialEq)]
pub struct IpcConfiguration {
    pub enabled: bool,
    pub socket_addr: String,
}

impl Default for IpcConfiguration {
    fn default() -> Self {
        IpcConfiguration {
            enabled: true,
            socket_addr: {
                let data_dir = ::dir::default_data_path();
                conflux_ipc_path(&data_dir, "$BASE/jsonrpc.ipc", 0)
            },
        }
    }
}

pub fn new_ipc(
    conf: IpcConfiguration,
    dependencies: &Dependencies,
) -> Result<Option<IpcServer>, String> {
    if !conf.enabled {
        return Ok(None);
    }

    let mut handler = IoHandler::new();
    handler.add_method("say_hello", |_| Ok(Value::String("Hello, world!".into())));

    let remote = dependencies.remote.clone();
    let path = PathBuf::from(&conf.socket_addr);

    if let Some(dir) = path.parent() {
        ::std::fs::create_dir_all(&dir).map_err(|err| {
            format!(
                "Unable to create IPC directory at {}: {}",
                dir.display(),
                err
            )
        })?;
    }

    match IpcServerBuilder::new(handler)
        .event_loop_remote(remote)
        .start(&conf.socket_addr)
    {
        Ok(server) => Ok(Some(server)),
        Err(io_error) => Err(format!("IPC error: {}", io_error)),
    }
}

#[derive(Debug, PartialEq)]
pub struct TcpConfiguration {
    pub enabled: bool,
    pub socket_addr: SocketAddr,
}

impl Default for TcpConfiguration {
    fn default() -> Self {
        TcpConfiguration {
            enabled: true,
            socket_addr: "0.0.0.0:2323".parse().unwrap(),
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

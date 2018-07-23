use std::any::Any;

use dir::Directories;
use parity_reactor::EventLoop;
use rpc;

#[derive(Debug, PartialEq)]
pub struct RunCmd {
    pub dirs: Directories,
    pub ipc_conf: rpc::IpcConfiguration,
    pub tcp_conf: rpc::TcpConfiguration,
}

pub struct RunningClient {
    inner: RunningClientInner,
}

struct RunningClientInner {
    keep_alive: Box<Any>,
}

pub fn execute(cmd: RunCmd) -> Result<RunningClient, String> {
    let event_loop = EventLoop::spawn();

    let dependencies = rpc::Dependencies {
        remote: event_loop.raw_remote(),
    };

    let ipc_server = rpc::new_ipc(cmd.ipc_conf, &dependencies)?;
    let tcp_server = rpc::new_tcp(cmd.tcp_conf, &dependencies)?;

    Ok(RunningClient {
        inner: RunningClientInner {
            keep_alive: Box::new((event_loop, ipc_server, tcp_server)),
        },
    })
}

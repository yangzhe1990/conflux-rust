use dir::Directories;
use rpc;

#[derive(Debug, PartialEq)]
pub struct RunCmd {
    pub dirs: Directories,
    pub tcp_conf: rpc::TcpConfiguration,
}

use core::ExecutionEngineRef;
use ethereum_types::{Address, H256};
use jsonrpc_core::{Error, Error as RpcError, IoHandler, Result as RpcResult};
use parity_reactor::TokioRemote;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use tcp::ServerBuilder as TcpServerBuilder;

pub use tcp::Server as TcpServer;

const DEFAULT_TCP_PORT: u16 = 32324;

pub struct Dependencies {
    pub remote: TokioRemote,
    pub execution_engine: ExecutionEngineRef,
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

build_rpc_trait! {
    pub trait Rpc {
        #[rpc(name = "say_hello")]
        fn say_hello(&self) -> RpcResult<String>;

        #[rpc(name = "getbalance")]
        fn get_balance(&self, Address) -> RpcResult<f64>;

        #[rpc(name = "getbestblockhash")]
        fn get_best_block_hash(&self) -> RpcResult<H256>;

        #[rpc(name = "getblockcount")]
        fn get_block_count(&self) -> RpcResult<usize>;

        #[rpc(name = "generate")]
        fn generate(&self, usize) -> RpcResult<()>;
    }
}

struct RpcImpl {
    execution_engine: ExecutionEngineRef,
}

impl RpcImpl {
    fn new(execution_engine: ExecutionEngineRef) -> Self {
        RpcImpl {
            execution_engine: execution_engine,
        }
    }
}

impl Rpc for RpcImpl {
    fn say_hello(&self) -> RpcResult<String> { Ok("Hello, world".into()) }

    fn get_balance(&self, addr: Address) -> RpcResult<f64> {
        let state = self.execution_engine.state.accounts.read();

        let acc = state.get(&addr);
        if acc.is_none() {
            Err(RpcError::invalid_params("Unknown account"))
        } else {
            Ok(acc.unwrap().balance())
        }
    }

    fn get_best_block_hash(&self) -> RpcResult<H256> { Ok(H256::zero()) }

    fn get_block_count(&self) -> RpcResult<usize> { Ok(0) }

    fn generate(&self, num_blocks: usize) -> RpcResult<()> { Ok(()) }
}

fn setup_apis(dependencies: &Dependencies) -> IoHandler {
    let mut handler = IoHandler::new();

    handler.extend_with(
        RpcImpl::new(dependencies.execution_engine.clone()).to_delegate(),
    );

    handler
}

pub fn new_tcp(
    conf: TcpConfiguration, dependencies: &Dependencies,
) -> Result<Option<TcpServer>, String> {
    if !conf.enabled {
        return Ok(None);
    }

    let handler = setup_apis(dependencies);
    let remote = dependencies.remote.clone();

    match TcpServerBuilder::new(handler)
        .event_loop_remote(remote)
        .start(&conf.socket_addr)
    {
        Ok(server) => Ok(Some(server)),
        Err(io_error) => Err(format!("TCP error: {}", io_error)),
    }
}

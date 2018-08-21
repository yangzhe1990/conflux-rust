use blockgen::BlockGeneratorRef;
use core::PeerInfo;
use core::{ExecutionEngineRef, LedgerRef, SyncEngineRef};
use ethereum_types::{Address, H256};
use http::Server as HttpServer;
use http::ServerBuilder as HttpServerBuilder;
use jsonrpc_core::{Error as RpcError, IoHandler, Result as RpcResult};
use network::NodeId;
use parity_reactor::TokioRemote;
use parking_lot::{Condvar, Mutex};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use tcp::Server as TcpServer;
use tcp::ServerBuilder as TcpServerBuilder;

pub struct Dependencies {
    pub remote: TokioRemote,
    pub ledger: LedgerRef,
    pub execution_engine: ExecutionEngineRef,
    pub sync_engine: SyncEngineRef,
    pub block_gen: BlockGeneratorRef,
    pub exit: Arc<(Mutex<bool>, Condvar)>,
}

#[derive(Debug, PartialEq)]
pub struct TcpConfiguration {
    pub enabled: bool,
    pub address: SocketAddr,
}

impl TcpConfiguration {
    pub fn new(port: Option<u16>) -> Self {
        TcpConfiguration {
            enabled: port.is_some(),
            address: SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::new(0, 0, 0, 0),
                port.unwrap_or(0),
            )),
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct HttpConfiguration {
    pub enabled: bool,
    pub address: SocketAddr,
}

impl HttpConfiguration {
    pub fn new(port: Option<u16>) -> Self {
        HttpConfiguration {
            enabled: port.is_some(),
            address: SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::new(0, 0, 0, 0),
                port.unwrap_or(0),
            )),
        }
    }
}

// The macro from jsonrpc_core to facilitate the definition of handlers
build_rpc_trait! {
    pub trait Rpc {
        #[rpc(name = "sayhello")]
        fn say_hello(&self) -> RpcResult<String>;

        #[rpc(name = "getbalance")]
        fn get_balance(&self, Address) -> RpcResult<f64>;

        #[rpc(name = "getbestblockhash")]
        fn get_best_block_hash(&self) -> RpcResult<H256>;

        #[rpc(name = "getblockcount")]
        fn get_block_count(&self) -> RpcResult<usize>;

        #[rpc(name = "generate")]
        fn generate(&self, usize) -> RpcResult<()>;

        #[rpc(name = "addnode")]
        fn add_peer(&self, SocketAddr) -> RpcResult<NodeId>;

        #[rpc(name = "removenode")]
        fn drop_peer(&self, NodeId) -> RpcResult<()>;

        #[rpc(name = "getpeerinfo")]
        fn get_peer_info(&self) -> RpcResult<Vec<PeerInfo>>;

        #[rpc(name = "stop")]
        fn stop(&self) -> RpcResult<()>;
    }
}

struct RpcImpl {
    ledger: LedgerRef,
    execution_engine: ExecutionEngineRef,
    sync_engine: SyncEngineRef,
    block_gen: BlockGeneratorRef,
    exit: Arc<(Mutex<bool>, Condvar)>,
}

impl RpcImpl {
    fn new(
        ledger: LedgerRef, execution_engine: ExecutionEngineRef,
        sync_engine: SyncEngineRef, block_gen: BlockGeneratorRef,
        exit: Arc<(Mutex<bool>, Condvar)>,
    ) -> Self
    {
        RpcImpl {
            ledger: ledger,
            execution_engine: execution_engine,
            sync_engine: sync_engine,
            block_gen: block_gen,
            exit: exit,
        }
    }
}

impl Rpc for RpcImpl {
    fn say_hello(&self) -> RpcResult<String> { Ok("Hello, world".into()) }

    fn get_balance(&self, addr: Address) -> RpcResult<f64> {
        info!("RPC Request: get_balance({:?})", addr);
        let state = self.execution_engine.state.accounts.read();

        let acc = state.get(&addr);
        if acc.is_none() {
            Err(RpcError::invalid_params("Unknown account"))
        } else {
            Ok(acc.unwrap().balance())
        }
    }

    fn get_best_block_hash(&self) -> RpcResult<H256> {
        info!("RPC Request: get_best_block_hash()");
        Ok(self.ledger.best_block_hash())
    }

    fn get_block_count(&self) -> RpcResult<usize> {
        info!("RPC Request: get_block_count()");
        Ok(self.ledger.best_block_number() as usize)
    }

    fn add_peer(&self, addr: SocketAddr) -> RpcResult<NodeId> {
        info!("RPC Request: add_peer({:?})", addr);
        match self.sync_engine.add_peer(addr) {
            Ok(x) => Ok(x),
            Err(_) => Err(RpcError::internal_error()),
        }
    }

    fn drop_peer(&self, id: NodeId) -> RpcResult<()> {
        info!("RPC Request: drop_peer({:?})", id);
        match self.sync_engine.drop_peer(id) {
            Ok(_) => Ok(()),
            Err(_) => Err(RpcError::internal_error()),
        }
    }

    fn generate(&self, num_txs: usize) -> RpcResult<()> {
        info!("RPC Request: generate({:?})", num_txs);
        self.block_gen.generate_block(num_txs);
        Ok(())
    }

    fn get_peer_info(&self) -> RpcResult<Vec<PeerInfo>> {
        info!("RPC Request: get_peer_info");
        Ok(self.sync_engine.get_peer_info())
    }

    fn stop(&self) -> RpcResult<()> {
        *self.exit.0.lock() = true;
        self.exit.1.notify_all();

        Ok(())
    }
}

fn setup_apis(dependencies: &Dependencies) -> IoHandler {
    let mut handler = IoHandler::new();

    // extend_with maps each method in RpcImpl object into a RPC handler
    handler.extend_with(
        RpcImpl::new(
            dependencies.ledger.clone(),
            dependencies.execution_engine.clone(),
            dependencies.sync_engine.clone(),
            dependencies.block_gen.clone(),
            dependencies.exit.clone(),
        ).to_delegate(),
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
        .start(&conf.address)
    {
        Ok(server) => Ok(Some(server)),
        Err(io_error) => {
            Err(format!("TCP error: {} (addr = {})", io_error, conf.address))
        }
    }
}

pub fn new_http(
    conf: HttpConfiguration, dependencies: &Dependencies,
) -> Result<Option<HttpServer>, String> {
    if !conf.enabled {
        return Ok(None);
    }

    let handler = setup_apis(dependencies);
    let remote = dependencies.remote.clone();

    match HttpServerBuilder::new(handler)
        .event_loop_remote(remote)
        .start_http(&conf.address)
    {
        Ok(server) => Ok(Some(server)),
        Err(io_error) => Err(format!(
            "HTTP error: {} (addr = {})",
            io_error, conf.address
        )),
    }
}

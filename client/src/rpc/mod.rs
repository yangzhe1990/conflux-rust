use crate::{
    http::{Server as HttpServer, ServerBuilder as HttpServerBuilder},
    tcp::{Server as TcpServer, ServerBuilder as TcpServerBuilder},
};
use blockgen::BlockGenerator;
use core::{
    state::State,
    statedb::StateDb,
    storage::{StorageManager, StorageManagerTrait},
    PeerInfo, SharedConsensusGraph, SharedSynchronizationService,
    SharedTransactionPool,
};
use ethereum_types::{Address, H256, U256};
use jsonrpc_core::{Error as RpcError, IoHandler, Result as RpcResult};
use jsonrpc_macros::build_rpc_trait;
use network::node_table::{NodeEndpoint, NodeEntry, NodeId};
use parity_reactor::TokioRemote;
use parking_lot::{Condvar, Mutex};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::Arc,
};

mod types;

use self::types::{
    Block as RpcBlock, Receipt as RpcReceipt, Status as RpcStatus,
    Transaction as RpcTransaction, H256 as RpcH256,
};
use primitives::{
    Action, SignedTransaction, Transaction, TransactionWithSignature,
};

pub struct Dependencies {
    pub remote: TokioRemote,
    pub storage_manager: Arc<StorageManager>,
    pub consensus: SharedConsensusGraph,
    pub sync: SharedSynchronizationService,
    pub block_gen: Arc<BlockGenerator>,
    pub tx_pool: SharedTransactionPool,
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
        fn get_balance(&self, Address) -> RpcResult<U256>;

        #[rpc(name = "getbestblockhash")]
        fn get_best_block_hash(&self) -> RpcResult<H256>;

        #[rpc(name = "getblockcount")]
        fn get_block_count(&self) -> RpcResult<usize>;

        #[rpc(name = "getblock")]
        fn get_block(&self, H256) -> RpcResult<RpcBlock>;

        #[rpc(name = "generate")]
        fn generate(&self, usize, usize) -> RpcResult<Vec<H256>>;

        #[rpc(name = "generatefixedblock")]
        fn generate_fixed_block(&self, H256, Vec<H256>, usize) -> RpcResult<H256>;

        #[rpc(name = "addnode")]
        fn add_peer(&self, NodeId, SocketAddr) -> RpcResult<()>;

        #[rpc(name = "removenode")]
        fn drop_peer(&self, NodeId, SocketAddr) -> RpcResult<()>;

        #[rpc(name = "getpeerinfo")]
        fn get_peer_info(&self) -> RpcResult<Vec<PeerInfo>>;

        #[rpc(name = "stop")]
        fn stop(&self) -> RpcResult<()>;

        #[rpc(name = "getnodeid")]
        fn get_nodeid(&self, Vec<u8>) -> RpcResult<Vec<u8>>;

        #[rpc(name = "getstatus")]
        fn get_status(&self) -> RpcResult<RpcStatus>;

        #[rpc(name = "addlatency")]
        fn add_latency(&self, NodeId, f64) -> RpcResult<()>;

        #[rpc(name = "generateoneblock")]
        fn generate_one_block(&self, usize) -> RpcResult<H256>;

        #[rpc(name = "gettransactionreceipt")]
        fn get_transaction_receipt(&self, H256) -> RpcResult<Option<RpcReceipt>>;

        #[rpc(name = "cfx_call")]
        fn call(&self, RpcTransaction) -> RpcResult<Vec<u8>>;
    }
}

struct RpcImpl {
    storage_manager: Arc<StorageManager>,
    consensus: SharedConsensusGraph,
    sync: SharedSynchronizationService,
    block_gen: Arc<BlockGenerator>,
    tx_pool: SharedTransactionPool,
    exit: Arc<(Mutex<bool>, Condvar)>,
}

impl RpcImpl {
    fn new(
        storage_manager: Arc<StorageManager>, consensus: SharedConsensusGraph,
        sync: SharedSynchronizationService, block_gen: Arc<BlockGenerator>,
        tx_pool: SharedTransactionPool, exit: Arc<(Mutex<bool>, Condvar)>,
    ) -> Self
    {
        RpcImpl {
            storage_manager,
            consensus,
            sync,
            block_gen,
            tx_pool,
            exit,
        }
    }
}

impl Rpc for RpcImpl {
    fn say_hello(&self) -> RpcResult<String> { Ok("Hello, world".into()) }

    fn get_balance(&self, addr: Address) -> RpcResult<U256> {
        info!("RPC Request: get_balance({:?})", addr);
        let state = State::new(
            StateDb::new(
                self.storage_manager
                    .get_state_at(self.consensus.best_state_block_hash())
                    .unwrap(),
            ),
            0.into(),
            Default::default(),
        );

        match state.balance(&addr) {
            Ok(balance) => Ok(balance),
            Err(_) => Err(RpcError::internal_error()),
        }
    }

    fn get_best_block_hash(&self) -> RpcResult<H256> {
        info!("RPC Request: get_best_block_hash()");
        Ok(self.consensus.best_block_hash())
    }

    fn get_block_count(&self) -> RpcResult<usize> {
        info!("RPC Request: get_block_count()");
        Ok(self.consensus.block_count())
    }

    fn get_block(&self, block_hash: H256) -> RpcResult<RpcBlock> {
        info!("RPC Request: get_block({:?})", block_hash);

        if let Some(block) = self.sync.block_by_hash(&block_hash) {
            Ok(RpcBlock::new(
                &*block,
                self.consensus.get_block_epoch_number(&block_hash),
                self.consensus.get_block_total_difficulty(&block_hash),
            ))
        } else {
            Err(RpcError::invalid_params("Invalid block hash"))
        }
    }

    fn add_peer(&self, node_id: NodeId, address: SocketAddr) -> RpcResult<()> {
        let node = NodeEntry {
            id: node_id,
            endpoint: NodeEndpoint {
                address,
                udp_port: address.port(),
            },
        };
        info!("RPC Request: add_peer({:?})", node.clone());
        match self.sync.add_peer(node) {
            Ok(x) => Ok(x),
            Err(_) => Err(RpcError::internal_error()),
        }
    }

    fn drop_peer(&self, node_id: NodeId, address: SocketAddr) -> RpcResult<()> {
        let node = NodeEntry {
            id: node_id,
            endpoint: NodeEndpoint {
                address,
                udp_port: address.port(),
            },
        };
        info!("RPC Request: drop_peer({:?})", node.clone());
        match self.sync.drop_peer(node) {
            Ok(_) => Ok(()),
            Err(_) => Err(RpcError::internal_error()),
        }
    }

    fn generate(
        &self, num_blocks: usize, num_txs: usize,
    ) -> RpcResult<Vec<H256>> {
        info!("RPC Request: generate({:?})", num_blocks);
        let mut hashes = Vec::new();
        for _i in 0..num_blocks {
            hashes
                .push(self.block_gen.generate_block_with_transactions(num_txs));
        }
        Ok(hashes)
    }

    fn generate_fixed_block(
        &self, parent_hash: H256, referee: Vec<H256>, num_txs: usize,
    ) -> RpcResult<H256> {
        info!(
            "RPC Request: generate_fixed_block({:?}, {:?}, {:?})",
            parent_hash, referee, num_txs
        );
        let hash =
            self.block_gen
                .generate_fixed_block(parent_hash, referee, num_txs);
        Ok(hash)
    }

    fn generate_one_block(&self, num_txs: usize) -> RpcResult<H256> {
        info!("RPC Request: generate_one_block()");
        // TODO Choose proper num_txs
        let hash = self.block_gen.generate_block(num_txs);
        Ok(hash)
    }

    fn get_peer_info(&self) -> RpcResult<Vec<PeerInfo>> {
        info!("RPC Request: get_peer_info");
        Ok(self.sync.get_peer_info())
    }

    fn stop(&self) -> RpcResult<()> {
        *self.exit.0.lock() = true;
        self.exit.1.notify_all();

        Ok(())
    }

    fn get_nodeid(&self, challenge: Vec<u8>) -> RpcResult<Vec<u8>> {
        match self.sync.sign_challenge(challenge) {
            Ok(r) => Ok(r),
            Err(_) => Err(RpcError::internal_error()),
        }
    }

    fn get_status(&self) -> RpcResult<RpcStatus> {
        let best_hash = self.consensus.best_block_hash();
        let block_number = self.consensus.block_count();
        let tx_count = self.tx_pool.len();
        if let Some(epoch_number) =
            self.consensus.get_block_epoch_number(&best_hash)
        {
            Ok(RpcStatus {
                best_hash: RpcH256::from(best_hash),
                epoch_number,
                block_number,
                pending_tx_number: tx_count,
            })
        } else {
            Err(RpcError::internal_error())
        }
    }

    fn add_latency(&self, id: NodeId, latency_ms: f64) -> RpcResult<()> {
        match self.sync.add_latency(id, latency_ms) {
            Ok(_) => Ok(()),
            Err(_) => Err(RpcError::internal_error()),
        }
    }

    /// The first element is true if the tx is executed in a confirmed block.
    /// The second element indicate the execution result (standin
    /// for receipt)
    fn get_transaction_receipt(
        &self, tx_hash: H256,
    ) -> RpcResult<Option<RpcReceipt>> {
        let maybe_receipt = self
            .consensus
            .get_transaction_receipt(&tx_hash)
            .map(|receipt| {
                RpcReceipt::new(receipt.gas_used.into(), receipt.outcome_status)
            });
        Ok(maybe_receipt)
    }

    fn call(&self, rpc_tx: RpcTransaction) -> RpcResult<Vec<u8>> {
        let tx = Transaction {
            nonce: rpc_tx.nonce.into(),
            gas: rpc_tx.gas.into(),
            gas_price: rpc_tx.gas_price.into(),
            value: rpc_tx.value.into(),
            action: match rpc_tx.to {
                Some(to) => Action::Call(to.into()),
                None => Action::Create,
            },
            data: rpc_tx.data,
        };
        let mut signed_tx = SignedTransaction::new_unsigned(
            TransactionWithSignature::new_unsigned(tx),
        );
        signed_tx.sender = rpc_tx.from.into();
        trace!("call tx {:?}", signed_tx);
        let result = self.consensus.call_virtual(&signed_tx);
        result.map_err(|e| {
            warn!("Transaction execution error {:?}", e);
            RpcError::internal_error()
        })
    }
}

fn setup_apis(dependencies: &Dependencies) -> IoHandler {
    let mut handler = IoHandler::new();

    // extend_with maps each method in RpcImpl object into a RPC handler
    handler.extend_with(
        RpcImpl::new(
            dependencies.storage_manager.clone(),
            dependencies.consensus.clone(),
            dependencies.sync.clone(),
            dependencies.block_gen.clone(),
            dependencies.tx_pool.clone(),
            dependencies.exit.clone(),
        )
        .to_delegate(),
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

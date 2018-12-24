#![allow(deprecated)]

extern crate clap;
extern crate ctrlc;
extern crate jsonrpc_core;
extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate jsonrpc_macros;
extern crate error_chain;
extern crate io;
extern crate jsonrpc_http_server as http;
extern crate jsonrpc_tcp_server as tcp;
extern crate mio;
extern crate parity_reactor;
extern crate parking_lot;
#[macro_use]
extern crate log;
extern crate ethereum_types;
extern crate keccak_hash as hash;
extern crate log4rs;
extern crate network;
extern crate rlp;
extern crate slab;
extern crate toml;

extern crate blockgen;
extern crate core;
extern crate db;
extern crate ethkey;
extern crate kvdb_rocksdb;
extern crate primitives;
extern crate rand;
extern crate rpc as conflux_rpc;
extern crate secret_store;
extern crate txgen;

#[macro_use]
mod config_macro;
mod cache_config;
mod configuration;
mod rpc;
#[cfg(test)]
mod tests;

pub use crate::configuration::Configuration;
use blockgen::BlockGenerator;
use core::{
    storage::StorageManager, vm_factory::VmFactory, ConsensusGraph,
    SynchronizationService, TransactionPool,
};
use http::Server as HttpServer;
use tcp::Server as TcpServer;

use ctrlc::CtrlC;
use db::SystemDB;
use parity_reactor::EventLoop;
use parking_lot::{Condvar, Mutex};
use secret_store::SecretStore;
use std::{
    any::Any,
    sync::{Arc, Weak},
    thread,
    time::{Duration, Instant},
};
use txgen::TransactionGenerator;

pub struct ClientHandle {
    pub event_loop: EventLoop,
    pub rpc_tcp_server: Option<TcpServer>,
    pub rpc_http_server: Option<HttpServer>,
    pub consensus: Arc<ConsensusGraph>,
    pub txpool: Arc<TransactionPool>,
    pub sync: Arc<SynchronizationService>,
    pub txgen: Arc<TransactionGenerator>,
    pub txgen_join_handle: Option<thread::JoinHandle<()>>,
    pub blockgen: Arc<BlockGenerator>,
    pub secret_store: Arc<SecretStore>,
    pub ledger_db: Weak<SystemDB>,
}

impl ClientHandle {
    pub fn into_be_dropped(self) -> (Weak<SystemDB>, Box<Any>) {
        (
            self.ledger_db,
            Box::new((
                self.event_loop,
                self.rpc_tcp_server,
                self.rpc_http_server,
                self.consensus,
                self.txpool,
                self.sync,
                self.txgen,
                self.blockgen,
                self.secret_store,
                self.txgen_join_handle,
            )),
        )
    }
}

pub struct Client {}

impl Client {
    // Start all key components of Conflux and pass out their handles
    pub fn start(
        conf: Configuration, exit: Arc<(Mutex<bool>, Condvar)>,
    ) -> Result<ClientHandle, String> {
        info!("Working directory: {:?}", std::env::current_dir());

        let network_config = conf.net_config();
        let _cache_config = conf.cache_config();

        let db_config = conf.db_config();
        let ledger_db = db::open_database(
            conf.raw_conf.db_dir.as_ref().unwrap(),
            &db_config,
        )
        .map_err(|e| format!("Failed to open database {:?}", e))?;

        let secret_store = Arc::new(SecretStore::new());
        let storage_manager = Arc::new(StorageManager::new(ledger_db.clone()));
        let genesis_block = storage_manager.initialize(secret_store.as_ref());
        debug!("Initialize genesis_block={:?}", genesis_block);

        let txpool = Arc::new(TransactionPool::with_capacity(
            100000,
            storage_manager.clone(),
        ));

        let vm = VmFactory::new(1024 * 32);
        let consensus = Arc::new(ConsensusGraph::with_genesis_block(
            genesis_block,
            storage_manager.clone(),
            vm.clone(),
            txpool.clone(),
            ledger_db.clone(),
        ));

        let sync_config = core::SynchronizationConfiguration {
            network: network_config,
            consensus: consensus.clone(),
        };
        let pow_config = conf.pow_config();
        let verification_config = conf.verification_config();
        let mut sync = core::SynchronizationService::new(
            sync_config,
            pow_config.clone(),
            verification_config,
        );
        sync.start().unwrap();
        let sync = Arc::new(sync);
        let sync_graph = sync.get_synchronization_graph();

        let txgen = Arc::new(TransactionGenerator::new(
            consensus.clone(),
            storage_manager.clone(),
            txpool.clone(),
            secret_store.clone(),
            sync.net_key_pair().ok(),
        ));

        let blockgen = Arc::new(BlockGenerator::new(
            sync_graph.clone(),
            txpool.clone(),
            sync.clone(),
            txgen.clone(),
            pow_config.clone(),
        ));

        let tx_conf = conf.tx_gen_config();
        let txgen_handle = if tx_conf.generate_tx {
            let txgen_clone = txgen.clone();
            Some(
                thread::Builder::new()
                    .name("txgen".into())
                    .spawn(move || {
                        TransactionGenerator::generate_transactions(
                            txgen_clone,
                            tx_conf,
                        )
                        .unwrap();
                    })
                    .expect("should succeed"),
            )
        } else {
            None
        };

        let event_loop = EventLoop::spawn();

        let rpc_deps = rpc::Dependencies {
            remote: event_loop.raw_remote(),
            storage_manager: storage_manager.clone(),
            consensus: consensus.clone(),
            sync: sync.clone(),
            block_gen: blockgen.clone(),
            tx_pool: txpool.clone(),
            exit,
        };
        let rpc_tcp_server = rpc::new_tcp(
            rpc::TcpConfiguration::new(conf.raw_conf.jsonrpc_tcp_port),
            &rpc_deps,
        )?;
        let rpc_http_server = rpc::new_http(
            rpc::HttpConfiguration::new(conf.raw_conf.jsonrpc_http_port),
            &rpc_deps,
        )?;

        Ok(ClientHandle {
            event_loop,
            ledger_db: Arc::downgrade(&ledger_db),
            rpc_http_server,
            rpc_tcp_server,
            txpool,
            txgen,
            txgen_join_handle: txgen_handle,
            blockgen,
            consensus,
            secret_store,
            sync,
        })
    }

    /// Use a Weak pointer to ensure that other Arc pointers are released
    fn wait_for_drop<T>(w: Weak<T>) {
        let sleep_duration = Duration::from_secs(1);
        let warn_timeout = Duration::from_secs(10);
        let max_timeout = Duration::from_secs(60);
        let instant = Instant::now();
        let mut warned = false;
        while instant.elapsed() < max_timeout {
            if w.upgrade().is_none() {
                return;
            }
            if !warned && instant.elapsed() > warn_timeout {
                warned = true;
                warn!("Shutdown is taking longer than expected.");
            }
            thread::sleep(sleep_duration);
        }
        warn!("Shutdown timeout reached, exiting uncleanly.");
    }

    pub fn close(handle: ClientHandle) -> i32 {
        let (ledger_db, to_drop) = handle.into_be_dropped();
        drop(to_drop);

        // Make sure ledger_db is properly dropped, so rocksdb can be closed
        // cleanly
        Client::wait_for_drop(ledger_db);
        0
    }

    pub fn run_until_closed(
        exit: Arc<(Mutex<bool>, Condvar)>, keep_alive: ClientHandle,
    ) -> i32 {
        CtrlC::set_handler({
            let e = exit.clone();
            move || {
                *e.0.lock() = true;
                e.1.notify_all();
            }
        });

        let mut lock = exit.0.lock();
        if !*lock {
            let _ = exit.1.wait(&mut lock);
        }
        Client::close(keep_alive)
    }
}

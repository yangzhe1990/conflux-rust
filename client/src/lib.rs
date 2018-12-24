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

use crate::configuration::Configuration;
use blockgen::BlockGenerator;
use clap::{App, Arg};
use core::{
    storage::StorageManager, vm_factory::VmFactory, ConsensusGraph,
    SynchronizationService, TransactionPool,
};
use http::Server as HttpServer;
use tcp::Server as TcpServer;

use ctrlc::CtrlC;
use db::SystemDB;
use log::LevelFilter;
use log4rs::{
    append::{console::ConsoleAppender, file::FileAppender},
    config::{Appender, Config as LogConfig, Logger, Root},
    encode::pattern::PatternEncoder,
};
use parity_reactor::EventLoop;
use parking_lot::{Condvar, Mutex};
use secret_store::SecretStore;
use std::{
    any::Any,
    io::{self as stdio, Write},
    process,
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

    pub fn close(handler: ClientHandle) -> i32 {
        let (ledger_db, to_drop) = handler.into_be_dropped();
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

    pub fn run() {
        let matches = App::new("conflux")
            .arg(
                Arg::with_name("port")
                    .long("port")
                    .value_name("PORT")
                    .help("Listen for p2p connections on PORT.")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("udp-port")
                    .long("udp-port")
                    .value_name("PORT")
                    .help("UDP port for peer discovery.")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("jsonrpc-tcp-port")
                    .long("jsonrpc-tcp-port")
                    .value_name("PORT")
                    .help("Specify the PORT for the TCP JSON-RPC API server.")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("jsonrpc-http-port")
                    .long("jsonrpc-http-port")
                    .value_name("PORT")
                    .help("Specify the PORT for the HTTP JSON-RPC API server.")
                    .takes_value(true),
            )
            .arg(
            Arg::with_name("log-file")
                .long("log-file")
                .value_name("FILE")
                .help("Specify the filename for the log. Stdout will be used by default if omitted.")
                .takes_value(true),
            )
            .arg(
                Arg::with_name("log-level")
                    .long("log-level")
                    .value_name("LEVEL")
                    .help("Can be error/warn/info/debug/trace. Default is the info level.")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("config")
                    .short("c")
                    .long("config")
                    .value_name("FILE")
                    .help("Sets a custom config file")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("bootnodes")
                    .long("bootnodes")
                    .value_name("NODES")
                    .help("Sets a custom list of bootnodes")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("netconf-dir")
                    .long("netconf-dir")
                    .value_name("NETCONF_DIR")
                    .help("Sets a custom directory for network configurations")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("public-address")
                    .long("public-address")
                    .value_name("ADDRESS")
                    .help("Sets a custom public address to be connected by others")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("ledger-cache-size")
                    .short("lcs")
                    .long("ledger-cache-size")
                    .value_name("SIZE")
                    .help("Sets the ledger cache size")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("db-cache-size")
                    .short("dcs")
                    .long("db-cache-size")
                    .value_name("SIZE")
                    .help("Sets the db cache size")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("enable-discovery")
                    .long("enable-discovery")
                    .value_name("BOOL")
                    .help("Enable discovery protocol")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("node-table-timeout")
                    .long("node-table-timeout")
                    .value_name("SEC")
                    .help("How often Conflux updates its peer table (default 300).")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("node-table-promotion-timeout")
                    .long("node-table-promotion-timeout")
                    .value_name("SEC")
                    .help("How long Conflux waits for promoting a peer to trustworthy (default 3 * 24 * 3600).")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("test-mode")
                    .long("test-mode")
                    .value_name("BOOL")
                    .help("Sets test mode for adding latency")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("db-compact-profile")
                    .long("db-compact-profile")
                    .value_name("ENUM")
                    .help("Sets the compaction profile of RocksDB.")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("db-dir")
                    .long("db-dir")
                    .value_name("PATH")
                    .help("Sets the root path of db.")
                    .takes_value(true),
            )
            .get_matches_from(std::env::args().collect::<Vec<_>>());

        let conf = Configuration::parse(&matches).unwrap();

        // If log_conf is provided, use it for log configuration and ignore
        // log_file and log_level. Otherwise, set stdout to INFO and set
        // all our crate log to log_level.
        let log_config = match conf.raw_conf.log_conf {
            Some(ref log_conf) => {
                log4rs::load_config_file(log_conf, Default::default()).unwrap()
            }
            None => {
                let mut conf_builder =
                    LogConfig::builder().appender(Appender::builder().build(
                        "stdout",
                        Box::new(ConsoleAppender::builder().build()),
                    ));
                let mut root_builder = Root::builder().appender("stdout");
                if let Some(ref log_file) = conf.raw_conf.log_file {
                    conf_builder =
                        conf_builder.appender(Appender::builder().build(
                            "logfile",
                            Box::new(
                                FileAppender::builder().encoder(Box::new(PatternEncoder::new("{d:23.23} {h({l}):5.5} {T:<20.20} {t:12.12} - {m}{n}"))).build(log_file).unwrap(),
                            ),
                        ));
                    root_builder = root_builder.appender("logfile");
                };
                // Should add new crate names here
                for crate_name in [
                    "blockgen", "core", "conflux", "db", "eth_key", "network",
                    "rpc", "txgen",
                ]
                .iter()
                {
                    conf_builder = conf_builder.logger(
                        Logger::builder()
                            .build(*crate_name, conf.raw_conf.log_level),
                    );
                }
                conf_builder
                    .build(root_builder.build(LevelFilter::Info))
                    .unwrap()
            }
        };
        log4rs::init_config(log_config).unwrap();

        let exit = Arc::new((Mutex::new(false), Condvar::new()));

        process::exit(match Client::start(conf, exit.clone()) {
            Ok(client_handler) => {
                Client::run_until_closed(exit, client_handler)
            }
            Err(err) => {
                writeln!(&mut stdio::stderr(), "{}", err).unwrap();
                1
            }
        });
    }
}

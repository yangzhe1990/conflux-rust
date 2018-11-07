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
extern crate ethkey;
extern crate primitives;
extern crate rand;
extern crate rpc as conflux_rpc;
extern crate secret_store;
extern crate txgen;

mod cache_config;
mod configuration;
mod rpc;

use blockgen::BlockGenerator;
use clap::{App, Arg};
use configuration::Configuration;
use core::{ConsensusGraph, StateManager, TransactionPool};
use ethkey::{public_to_address, Generator, Random};

use ctrlc::CtrlC;
use ethereum_types::U256;
use hash::keccak;
use log::LevelFilter;
use log4rs::{
    append::{console::ConsoleAppender, file::FileAppender},
    config::{Appender, Config as LogConfig, Logger, Root},
};
use parity_reactor::EventLoop;
use parking_lot::{Condvar, Mutex};
use primitives::{Block, BlockHeaderBuilder, Transaction};
use rand::{thread_rng, Rng};
use rlp::RlpStream;
use secret_store::SecretStore;
use std::{
    any::Any,
    collections::HashSet,
    io::{self as stdio, Write},
    process,
    sync::Arc,
    thread,
};
use txgen::TransactionGenerator;

fn make_genesis(num_addresses: i32, secret_store: Arc<SecretStore>) -> Block {
    let mut addresses = HashSet::new();
    let mut genesis_transactions = Vec::new();
    let mut rng = thread_rng();

    for _ in 0..num_addresses {
        let kp = Random.generate().unwrap();
        let address = public_to_address(kp.public());
        if addresses.contains(&address) {
            continue;
        }
        addresses.insert(address);

        let transaction = Transaction {
            nonce: U256::zero(),
            gas_price: U256::from(1_000_000_000_000_000u64),
            gas: U256::from(200u64),
            value: U256::from(rng.gen::<u64>()),
            receiver: address.clone(),
        };
        genesis_transactions.push(transaction.sign(kp.secret()));

        secret_store.insert(kp);
    }

    let mut stream = RlpStream::new_list(genesis_transactions.len());
    for transaction in &genesis_transactions {
        stream.append(transaction);
    }

    Block {
        block_header: BlockHeaderBuilder::new()
            .with_transactions_root(keccak(stream.out()))
            .build(),
        transactions: genesis_transactions,
    }
}

// Start all key components of Conflux and pass out their handles
fn start(
    conf: Configuration, exit: Arc<(Mutex<bool>, Condvar)>,
) -> Result<Box<Any>, String> {
    let network_config = conf.net_config();
    let _cache_config = conf.cache_config();

    let secret_store = Arc::new(SecretStore::new());
    let genesis_block = make_genesis(10, secret_store.clone());

    let state_manager = Arc::new(StateManager::default());

    let consensus = Arc::new(ConsensusGraph::with_genesis_block(
        genesis_block,
        state_manager.clone(),
    ));

    let sync_config = core::SynchronizationConfiguration {
        network: network_config,
        consensus: consensus.clone(),
    };
    let mut sync = core::SynchronizationService::new(sync_config);
    sync.start().unwrap();
    let sync = Arc::new(sync);

    let txpool = Arc::new(TransactionPool::with_capacity(10000));

    let blockgen = Arc::new(BlockGenerator::new(
        consensus.clone(),
        txpool.clone(),
        sync.clone(),
    ));

    let txgen = Arc::new(TransactionGenerator::new(
        consensus.clone(),
        state_manager.clone(),
        txpool.clone(),
        secret_store.clone(),
    ));
    let txgen_handle = thread::spawn(move || {
        TransactionGenerator::generate_transactions(txgen).unwrap();
    });

    let event_loop = EventLoop::spawn();

    let rpc_deps = rpc::Dependencies {
        remote: event_loop.raw_remote(),
        state_manager: state_manager.clone(),
        consensus: consensus.clone(),
        sync: sync.clone(),
        block_gen: blockgen.clone(),
        exit: exit,
    };
    let rpc_tcp_server = rpc::new_tcp(
        rpc::TcpConfiguration::new(conf.jsonrpc_tcp_port),
        &rpc_deps,
    )?;
    let rpc_http_server = rpc::new_http(
        rpc::HttpConfiguration::new(conf.jsonrpc_http_port),
        &rpc_deps,
    )?;

    Ok(Box::new((
        event_loop,
        rpc_tcp_server,
        rpc_http_server,
        sync,
        blockgen,
        secret_store.clone(),
        //block_gen_handle,
        txgen_handle,
    )))
}

fn main() {
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
        ).arg(
            Arg::with_name("log-file")
                 .long("log-file")
                 .value_name("FILE")
                 .help("Specify the filename for the log. Stdout will be used by default if omitted.")
                 .takes_value(true),
        ).arg(
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
        ).arg(
            Arg::with_name("ledger-cache-size")
                .short("lcs")
                .long("ledger-cache-size")
                .value_name("SIZE")
                .help("Sets the ledger cache size")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("test-mode")
                .long("test-mode")
                .value_name("BOOL")
                .help("Sets test mode for adding latency")
                .takes_value(true),
        )
        .get_matches_from(std::env::args().collect::<Vec<_>>());
    let conf = Configuration::parse(&matches).unwrap();

    let stdout = ConsoleAppender::builder().build();

    let log_config = if conf.log_file.is_none() {
        LogConfig::builder()
            .appender(Appender::builder().build("stdout", Box::new(stdout)))
            .logger(
                Logger::builder()
                    .appender("stdout")
                    .additive(false)
                    .build("network", conf.log_level),
            )
            .logger(
                Logger::builder()
                    .appender("stdout")
                    .additive(false)
                    .build("rpc", conf.log_level),
            )
            .build(Root::builder().appender("stdout").build(LevelFilter::Info))
            .unwrap()
    } else {
        let log_file = FileAppender::builder()
            .build(conf.log_file.as_ref().unwrap())
            .unwrap();

        LogConfig::builder()
            .appender(Appender::builder().build("stdout", Box::new(stdout)))
            .appender(Appender::builder().build("logfile", Box::new(log_file)))
            .logger(
                Logger::builder()
                    .appender("logfile")
                    .additive(false)
                    .build("network", conf.log_level),
            )
            .logger(
                Logger::builder()
                    .appender("logfile")
                    .additive(false)
                    .build("rpc", conf.log_level),
            )
            .logger(
                Logger::builder()
                    .appender("logfile")
                    .additive(false)
                    .build("sync", conf.log_level),
            )
            .logger(
                Logger::builder()
                    .appender("logfile")
                    .additive(false)
                    .build("discovery", conf.log_level),
            )
            .build(Root::builder().appender("stdout").build(LevelFilter::Info))
            .unwrap()
    };
    log4rs::init_config(log_config).unwrap();

    let exit = Arc::new((Mutex::new(false), Condvar::new()));

    process::exit(match start(conf, exit.clone()) {
        Ok(keep_alive) => {
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

            drop(keep_alive);

            0
        }
        Err(err) => {
            writeln!(&mut stdio::stderr(), "{}", err).unwrap();
            1
        }
    });
}

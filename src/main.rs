#![allow(deprecated)]

#[macro_use]
extern crate clap;
extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
extern crate ctrlc;
extern crate jsonrpc_core;
#[macro_use]
extern crate jsonrpc_macros;
extern crate jsonrpc_http_server as http;
extern crate jsonrpc_tcp_server as tcp;
extern crate parking_lot;
#[macro_use]
extern crate error_chain;
extern crate io;
extern crate mio;
extern crate parity_reactor;
#[macro_use]
extern crate log;
extern crate ethereum_types;
extern crate network;
extern crate simplelog;
extern crate slab;
extern crate toml;

extern crate blockgen;
extern crate core;
// extern crate vm;

mod configuration;
mod rpc;

use blockgen::BlockGenerator;
use clap::{App, Arg};
use configuration::Configuration;
use ctrlc::CtrlC;
use network::NetworkConfiguration;
use parity_reactor::EventLoop;
use parking_lot::{Condvar, Mutex};
use simplelog::{CombinedLogger, Config as LogConfig, TermLogger, WriteLogger};
use std::any::Any;
use std::fs::File;
use std::io::{self as stdio, Write};
use std::process;
use std::sync::Arc;
use std::thread;

// Start all key components of Conflux and pass out their handles
fn start(conf: Configuration) -> Result<Box<Any>, String> {
    let net_conf = match conf.port {
        Some(port) => network::NetworkConfiguration::new_with_port(port),
        None => network::NetworkConfiguration::default(),
    };

    let ledger = core::Ledger::new_ref();
    ledger.initialize_with_genesis();

    let execution_engine = core::ExecutionEngine::new_ref(ledger.clone());

    let sync_params = core::SyncParams {
        config: Default::default(),
        network_config: net_conf,
        ledger: ledger.clone(),
        execution_engine: execution_engine.clone(),
    };

    let mut sync_engine = core::SyncEngine::new(sync_params);
    sync_engine.start();
    let sync_engine_ref = Arc::new(sync_engine);

    let event_loop = EventLoop::spawn();

    let blockgen =
        Arc::new(BlockGenerator::new(ledger.clone(), sync_engine_ref.clone()));
    /*let bgen = blockgen.clone();
    let block_gen_handle = thread::spawn(move || {
        BlockGenerator::start_mining(bgen, 0);
    });*/

    let rpc_deps = rpc::Dependencies {
        remote: event_loop.raw_remote(),
        ledger: ledger.clone(),
        execution_engine: execution_engine.clone(),
        sync_engine: sync_engine_ref.clone(),
        block_gen: blockgen.clone(),
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
        sync_engine_ref,
        blockgen,
        //block_gen_handle,
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
        .get_matches_from(std::env::args().collect::<Vec<_>>());
    let conf = Configuration::parse(&matches).unwrap();

    if conf.log_file.is_none() {
        CombinedLogger::init(vec![
            TermLogger::new(conf.log_level.clone(), LogConfig::default())
                .unwrap(),
        ]).unwrap();
    } else {
        CombinedLogger::init(vec![WriteLogger::new(
            conf.log_level.clone(),
            LogConfig::default(),
            File::create(conf.log_file.clone().unwrap()).unwrap(),
        )]).unwrap();
    }

    let exit = Arc::new((Mutex::new(false), Condvar::new()));

    process::exit(match start(conf) {
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

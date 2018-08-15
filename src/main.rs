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
extern crate slab;

extern crate blockgen;
extern crate core;
// extern crate vm;

mod configuration;
mod rpc;

use clap::{App, Arg};
use configuration::Configuration;
use ctrlc::CtrlC;
use network::{NetworkConfiguration, NetworkService};
use parity_reactor::EventLoop;
use parking_lot::{Condvar, Mutex};
use std::any::Any;
use std::io::{self as stdio, Write};
use std::process;
use std::sync::Arc;
use std::{thread, time};

fn start(conf: Configuration) -> Result<Box<Any>, String> {
    let net_conf = match conf.port {
        Some(port) => network::NetworkConfiguration::new_with_port(port),
        None => network::NetworkConfiguration::default(),
    };

    let ledger = core::Ledger::new_ref();
    let execution_engine = core::ExecutionEngine::new_ref(ledger.clone());

    let sync_params = core::SyncParams {
        config: Default::default(),
        network_config: net_conf,
        ledger: ledger.clone(),
        execution_engine: execution_engine.clone(),
    };

    let mut sync_engine = core::SyncEngine::new(sync_params);
    sync_engine.start();

    let blockgen = blockgen::BlockGenerator::new(ledger.clone());

    let event_loop = EventLoop::spawn();

    let rpc_deps = rpc::Dependencies {
        remote: event_loop.raw_remote(),
        execution_engine: execution_engine,
    };
    let rpc_server =
        rpc::new_tcp(rpc::TcpConfiguration::new(conf.jsonrpc_port), &rpc_deps)?;

    Ok(Box::new((
        event_loop,
        rpc_server,
        Arc::new(sync_engine),
        // cfx_vm,
        blockgen,
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
            Arg::with_name("jsonrpc-port")
                .long("jsonrpc-port")
                .value_name("PORT")
                .help("Specify the PORT for the TCP JSON-RPC API server.")
                .takes_value(true),
        )
        .get_matches_from(std::env::args().collect::<Vec<_>>());
    let conf = Configuration::parse(&matches).unwrap();

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

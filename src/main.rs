#![allow(deprecated)]

#[macro_use]
extern crate clap;
extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
extern crate ctrlc;
extern crate jsonrpc_core;
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

extern crate core;
extern crate blockgen;
extern crate vm;

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
use core::LedgerCore;

fn start(conf: Configuration) -> Result<Box<Any>, String> {
    let event_loop = EventLoop::spawn();

    let rpc_deps = rpc::Dependencies {
        remote: event_loop.raw_remote(),
    };
    let rpc_server =
        rpc::new_tcp(rpc::TcpConfiguration::new(conf.jsonrpc_port), &rpc_deps)?;

    let net_conf = match conf.port {
        Some(port) => network::NetworkConfiguration::new_with_port(port),
        None => network::NetworkConfiguration::default(),
    };

    /*let net_svc =
        network::NetworkService::new(net_conf).map_err(|e| format!("{}", e))?;
    net_svc.start().map_err(|e| format!("{}", e))?;*/

    let cfx_vm = vm::ConfluxVM::new();

    let cfx_ledger_eng = core::ledger::LedgerEngine::new(cfx_vm.clone());

    let cfx_sync_params = core::Params {
        config: Default::default(),
        network_config: net_conf,
        ledger: cfx_ledger_eng.clone(),
    };

    let cfx_sync = core::ConfluxSync::new(cfx_sync_params).unwrap();

    let cfx_blockgen = blockgen::ConfluxBlockGenerator::new(cfx_sync.clone());

    cfx_sync.start();

    Ok(Box::new((event_loop, rpc_server, cfx_sync, cfx_vm, cfx_blockgen)))
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

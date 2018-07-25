#[macro_use]
extern crate clap;
extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
extern crate ctrlc;
extern crate jsonrpc_core;
extern crate jsonrpc_ipc_server as ipc;
extern crate jsonrpc_tcp_server as tcp;
extern crate parking_lot;

extern crate parity_reactor;

mod account;
mod configuration;
mod network;
mod rpc;

use clap::{App, Arg, SubCommand};
use configuration::Configuration;
use ctrlc::CtrlC;
use parity_reactor::EventLoop;
use parking_lot::{Condvar, Mutex};
use std::any::Any;
use std::io::{self as stdio, Write};
use std::process;
use std::sync::Arc;

fn start(_conf: Configuration) -> Result<Box<Any>, String> {
    let event_loop = EventLoop::spawn();

    let rpc_deps = rpc::Dependencies {
        remote: event_loop.raw_remote(),
    };
    let rpc_server = rpc::new_tcp(rpc::TcpConfiguration::default(), &rpc_deps)?;

    Ok(Box::new((event_loop, rpc_server)))
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
        .get_matches_from(std::env::args().collect::<Vec<_>>());
    let conf = Configuration::parse(&matches).unwrap();

    let exit = Arc::new((Mutex::new(false), Condvar::new()));

    process::exit(match start(conf) {
        Ok(_) => {
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
            0
        }
        Err(err) => {
            writeln!(&mut stdio::stderr(), "{}", err).unwrap();
            1
        }
    });
}

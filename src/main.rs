#[macro_use]
extern crate clap;
extern crate dir;
extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
extern crate jsonrpc_core;
extern crate jsonrpc_ipc_server as ipc;
extern crate jsonrpc_tcp_server as tcp;
extern crate parking_lot;
extern crate toml;

extern crate ctrlc;
extern crate panic_hook;
extern crate parity_reactor;
extern crate parity_version;

mod account;
mod cli;
mod configuration;
mod helpers;
mod rpc;
mod run;

use std::io::{self as stdio, Read, Write};
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use configuration::{Cmd, Configuration};
use ctrlc::CtrlC;
use parking_lot::{Condvar, Mutex};

enum ExecutionAction {
    Instant(Option<String>),
    Running(run::RunningClient),
}

#[derive(Debug)]
struct ExitStatus {
    should_exit: bool,
}

fn execute(conf: Configuration) -> Result<ExecutionAction, String> {
    let cmd = conf.into_command();
    match conf.into_command() {
        Cmd::Run(run_cmd) => {
            let outcome = run::execute(run_cmd)?;
            Ok(ExecutionAction::Running(outcome))
        }
        _ => Err("Unknown command".to_owned()),
    }
}

fn main() {
    panic_hook::set();

    let conf = {
        let mut args = std::env::args().skip(1).collect::<Vec<_>>();
        args.insert(0, "conflux".to_owned());

        Configuration::parse(&args).unwrap_or_else(|e| e.exit())
    };

    let exit = Arc::new((
        Mutex::new(ExitStatus { should_exit: false }),
        Condvar::new(),
    ));

    let res = match execute(conf) {
        Ok(result) => match result {
            ExecutionAction::Running(client) => {
                CtrlC::set_handler({
                    let e = exit.clone();
                    move || {
                        *e.0.lock() = ExitStatus { should_exit: true };
                        e.1.notify_all();
                    }
                });

                let mut lock = exit.0.lock();
                if !lock.should_exit {
                    let _ = exit.1.wait(&mut lock);
                }

                0
            }
            _ => 0,
        },
        Err(err) => {
            writeln!(&mut stdio::stderr(), "{}", err).expect("stderr availablel; qed");
            1
        }
    };

    process::exit(res);
}

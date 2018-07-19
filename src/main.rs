#[macro_use]
extern crate clap;
extern crate dir;
extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
extern crate toml;

extern crate panic_hook;
extern crate parity_version;

mod cli;
mod configuration;

use cli::{Args, ArgsError};
use configuration::Configuration;
use std::env;

fn main() {
    panic_hook::set();

    let mut args = std::env::args().skip(1).collect::<Vec<_>>();

    args.insert(0, "singularity".to_owned());
    let conf = Configuration::parse(&args).unwrap_or_else(|e| e.exit());

    println!("{:?}", conf);
}

mod config;
mod error;
mod host;
mod node_table;
mod service;

pub use self::config::NetworkConfiguration;
pub use self::error::{Error, ErrorKind};
pub use self::service::NetworkService;

#[derive(Clone)]
pub enum NetworkIoMessage {
    InitPublicInterface,
    NetworkStarted(String),
}

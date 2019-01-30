#![allow(deprecated)]
extern crate core;
extern crate elastic_array;
extern crate ethcore_bytes as bytes;
extern crate ethereum_types;
extern crate keylib;
extern crate io;
extern crate keccak_hash as hash;
#[macro_use]
extern crate log;
extern crate message;
extern crate network;
extern crate parking_lot;
extern crate primitives;
extern crate rand;
extern crate rlp;
extern crate secret_store;
#[macro_use]
extern crate error_chain;
extern crate db as ext_db;
extern crate kvdb;
extern crate slab;
#[macro_use]
extern crate lazy_static;
extern crate bit_set;
extern crate bn;
extern crate byteorder;
extern crate heapsize;
extern crate memory_cache;
extern crate num;
extern crate parity_crypto;

#[cfg(test)]
extern crate rustc_hex;
extern crate unexpected;

mod builtin;
pub mod cache_config;
mod cache_manager;
pub mod consensus;
pub mod db;
pub mod error;
mod evm;
pub mod executive;
pub mod machine;
pub mod pow;
pub(crate) mod snapshot;
pub mod state;
pub mod statedb;
pub mod storage;
pub mod sync;
pub mod transaction_pool;
pub mod verification;
pub mod vm;
pub mod vm_factory;

pub use crate::{
    consensus::{ConsensusGraph, SharedConsensusGraph},
    sync::{
        BestInformation, SharedSynchronizationGraph,
        SharedSynchronizationService, SynchronizationConfiguration,
        SynchronizationService,
    },
    transaction_pool::{SharedTransactionPool, TransactionPool},
};
pub use network::PeerInfo;

extern crate core;
extern crate elastic_array;
extern crate ethcore_bytes as bytes;
extern crate ethereum_types;
extern crate ethkey;
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
extern crate heapsize;
extern crate memory_cache;

#[cfg(test)]
extern crate rustc_hex;
extern crate triehash_ethereum as triehash;
extern crate unexpected;

mod cache_manager;
mod consensus;
pub mod db;
pub mod error;
mod evm;
mod executor;
pub mod pow;
pub(crate) mod snapshot;
pub(crate) mod storage;
mod sync;
pub mod transaction_pool;
pub mod verification;
mod vm;

pub use consensus::{ConsensusGraph, SharedConsensusGraph};
pub use executor::get_account;
pub use network::PeerInfo;
pub use storage::{
    state::{State, StateTrait},
    state_manager::{SharedStateManager, StateManager, StateManagerTrait},
};
pub use sync::{
    BestInformation, SharedSynchronizationGraph, SharedSynchronizationService,
    SynchronizationConfiguration, SynchronizationService,
};
pub use transaction_pool::{SharedTransactionPool, TransactionPool};

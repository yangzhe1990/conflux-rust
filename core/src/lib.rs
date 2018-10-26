// For merkle_patricia_trie.
#![feature(untagged_unions)]
#![feature(allocator_api)]
#![feature(alloc)]

// For merkle_patricia_trie.
extern crate alloc;
extern crate core;

extern crate ethcore_bytes as bytes;
extern crate ethereum_types;
extern crate ethkey;
extern crate io;
extern crate keccak_hash as hash;
#[macro_use]
extern crate lazy_static;
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
extern crate slab;

mod consensus;
pub mod execution_engine;
pub mod ledger;
mod cache_manager;
mod sync;
pub mod transaction_pool;

pub mod execution;
pub(crate) mod snapshot;
pub(crate) mod storage;

pub use self::{
    consensus::{ConsensusGraph, SharedConsensusGraph},
    sync::{
        SharedSynchronizationService, SynchronizationConfiguration,
        SynchronizationService,
    },
};
pub use execution_engine::{ExecutionEngine, ExecutionEngineRef};
pub use ledger::{Ledger, LedgerRef};
pub use network::PeerInfo;

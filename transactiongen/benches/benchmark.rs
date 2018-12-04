extern crate clap;
extern crate core;
extern crate db;
extern crate kvdb_rocksdb;
extern crate log;
extern crate network;
extern crate primitives;
extern crate secret_store;
extern crate toml;
extern crate txgen;

#[macro_use]
extern crate criterion;

#[macro_use]
mod config_macro;
mod cache_config;
mod configuration;

use self::{
    clap::{App, Arg},
    configuration::Configuration,
    db::SystemDB,
    secret_store::SecretStore,
};
use core::{ConsensusGraph, StateManager, TransactionPool};
use criterion::Criterion;
use primitives::{Block, BlockHeaderBuilder};
use std::{sync::Arc, thread};
use txgen::TransactionGenerator;

fn make_genesis() -> Block {
    Block {
        block_header: BlockHeaderBuilder::new().build(),
        transactions: Vec::new(),
    }
}

fn build_fake_environment() -> Arc<TransactionGenerator> {
    let conf = Configuration::default();
    let network_config = conf.net_config();
    let db_config = conf.db_config();

    let ledger_db =
        db::open_database(conf.raw_conf.db_dir.as_ref().unwrap(), &db_config)
            .unwrap();

    let state_manager = Arc::new(StateManager::default());

    let txpool =
        Arc::new(TransactionPool::with_capacity(10000, state_manager.clone()));

    let secret_store = Arc::new(SecretStore::new());
    let genesis_block = make_genesis();

    let consensus = Arc::new(ConsensusGraph::with_genesis_block(
        genesis_block,
        state_manager.clone(),
        txpool.clone(),
        ledger_db.clone(),
    ));

    let sync_config = core::SynchronizationConfiguration {
        network: network_config,
        consensus: consensus.clone(),
    };
    let mut sync = core::SynchronizationService::new(sync_config);
    //sync.start().unwrap();
    let sync = Arc::new(sync);

    Arc::new(TransactionGenerator::new(
        consensus.clone(),
        state_manager.clone(),
        txpool.clone(),
        secret_store.clone(),
    ))
}

fn txgen_benchmark(c: &mut Criterion) {
    c.bench_function("Randomly generate 100 transactions", |b| {
        let txgen = build_fake_environment();
        for _ in 0..100 {
            b.iter(|| {
                txgen.generate_transaction();
            });
        }
    });
}

criterion_group!(benches, txgen_benchmark);
criterion_main!(benches);

extern crate clap;
extern crate db;
extern crate kvdb_rocksdb;
extern crate network;
extern crate secret_store;
extern crate toml;

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
use super::BlockGenerator;
use core::{ConsensusGraph, StateManager, TransactionPool};
use primitives::{Block, BlockHeaderBuilder};
use std::{sync::Arc, thread};
use time;
use txgen::TransactionGenerator;

fn make_genesis() -> Block {
    Block {
        block_header: BlockHeaderBuilder::new().build(),
        transactions: Vec::new(),
    }
}

fn build_fake_environment() -> Arc<BlockGenerator> {
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
    let pow_config = conf.pow_config();
    let verification_config = conf.verification_config();
    let mut sync = core::SynchronizationService::new(
        sync_config,
        pow_config,
        verification_config,
    );
    //sync.start().unwrap();
    let sync = Arc::new(sync);

    let txgen = Arc::new(TransactionGenerator::new(
        consensus.clone(),
        state_manager.clone(),
        txpool.clone(),
        secret_store.clone(),
    ));

    Arc::new(BlockGenerator::new(
        consensus.clone(),
        txpool.clone(),
        sync.clone(),
        txgen.clone(),
    ))
}

#[test]
fn test_mining() {
    let bgen = build_fake_environment();
    let block_gen_handle = thread::spawn(move || {
        BlockGenerator::start_mining(bgen, 0);
    });
    println!("Start to sleep!");
    let sleep_duration = time::Duration::from_millis(20000);
    thread::sleep(sleep_duration);
}

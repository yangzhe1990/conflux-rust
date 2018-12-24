extern crate tempdir;

use self::tempdir::TempDir;
use super::super::{BlockGenerator, Client, ClientHandle, Configuration};
use parking_lot::{Condvar, Mutex};
use std::{sync::Arc, thread, time};

fn test_mining_inner(handler: &ClientHandle) {
    let bgen = handler.blockgen.clone();
    //println!("Pow Config: {:?}", bgen.pow_config());
    thread::spawn(move || {
        BlockGenerator::start_mining(bgen, 0);
    });
    let sync_graph = handler.sync.get_synchronization_graph();
    let best_block_hash = sync_graph.get_best_info().best_block_hash;
    let start_height = sync_graph.get_block_height(&best_block_hash).unwrap();
    let sleep_duration = time::Duration::from_millis(20000);
    thread::sleep(sleep_duration);
    let new_best_block_hash = sync_graph.get_best_info().best_block_hash;
    let end_height = sync_graph.get_block_height(&new_best_block_hash).unwrap();
    if end_height - start_height < 10 {
        panic!("Miner too few blocks ({})", end_height - start_height);
    }
}

#[test]
fn test_mining() {
    let mut conf = Configuration::default();
    conf.raw_conf.test_mode = true;
    let tmp_dir = TempDir::new("conflux-test").unwrap();
    conf.raw_conf.db_dir = Some(
        tmp_dir
            .path()
            .join("db")
            .into_os_string()
            .into_string()
            .unwrap(),
    );
    conf.raw_conf.netconf_dir = Some(
        tmp_dir
            .path()
            .join("config")
            .into_os_string()
            .into_string()
            .unwrap(),
    );

    let exit = Arc::new((Mutex::new(false), Condvar::new()));
    let handler = Client::start(conf, exit.clone()).unwrap();

    test_mining_inner(&handler);

    Client::close(handler);
}

extern crate tempdir;

use self::tempdir::TempDir;
use super::super::{BlockGenerator, Client, ClientHandle, Configuration};
use parking_lot::{Condvar, Mutex};
use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

fn test_mining_10_epochs_inner(handle: &ClientHandle) {
    let bgen = handle.blockgen.clone();
    //println!("Pow Config: {:?}", bgen.pow_config());
    thread::spawn(move || {
        BlockGenerator::start_mining(bgen, 0);
    });
    let sync_graph = handle.sync.get_synchronization_graph();
    let best_block_hash = sync_graph.get_best_info().best_block_hash;
    let start_height = sync_graph.get_block_height(&best_block_hash).unwrap();

    let sleep_duration = Duration::from_secs(1);
    let max_timeout = Duration::from_secs(60);

    let instant = Instant::now();
    while instant.elapsed() < max_timeout {
        let new_best_block_hash = sync_graph.get_best_info().best_block_hash;
        let end_height =
            sync_graph.get_block_height(&new_best_block_hash).unwrap();
        info!("{}", end_height - start_height);
        if end_height - start_height >= 10 {
            BlockGenerator::stop(handle.blockgen.clone());
            return;
        }
        thread::sleep(sleep_duration);
    }
    let new_best_block_hash = sync_graph.get_best_info().best_block_hash;
    let end_height = sync_graph.get_block_height(&new_best_block_hash).unwrap();
    BlockGenerator::stop(handle.blockgen.clone());
    panic!(
        "Mined too few blocks, delta height is only {}.",
        end_height - start_height
    );
}

#[test]
fn test_mining_10_epochs() {
    let mut conf = Configuration::default();
    conf.raw_conf.test_mode = false;
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
    let handle = Client::start(conf, exit.clone()).unwrap();

    test_mining_10_epochs_inner(&handle);

    Client::close(handle);
}

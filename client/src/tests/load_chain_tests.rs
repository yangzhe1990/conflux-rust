extern crate tempdir;

use self::tempdir::TempDir;
use super::super::{Client, Configuration};
use ethereum_types::H256;
use parking_lot::{Condvar, Mutex};
use std::sync::Arc;

#[test]
fn test_load_chain() {
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
    conf.raw_conf.load_test_chain =
        Some(r#"../test/blockchain_tests/general_1.json"#.to_owned());
    conf.raw_conf.port = Some(13000);
    conf.raw_conf.jsonrpc_http_port = Some(18000);

    let exit = Arc::new((Mutex::new(false), Condvar::new()));
    let handle = Client::start(conf, exit.clone()).unwrap();

    let expected =
        "0xb3f9375b0a878786c48fa099503a294007bdc9f615b4e86d8d47a08b318a1837";
    let best_block_hash: H256 =
        serde_json::from_str(&format!("{:?}", expected)).unwrap();
    assert_eq!(best_block_hash, handle.consensus.best_block_hash());

    Client::close(handle);
}
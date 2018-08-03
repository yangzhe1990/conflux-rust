extern crate ethereum_types;
extern crate parking_lot;
extern crate network;

mod api;

use network::Error;
use std::collections::HashMap;
use ethereum_types::{Address};
use api::AccountStoreInterface;
use std::sync::{Arc};
use parking_lot::RwLock;

pub struct AccountEntry {
    val: f64,
    nonce: u64,
}

pub struct AccountStore {
    table: RwLock<HashMap<Address, AccountEntry>>,
}

impl AccountStore {
    pub fn new() -> Result<Arc<AccountStore>, Error> {
        let map = HashMap::<Address, AccountEntry>::new();
        let ac = Arc::new(AccountStore {
            table: RwLock::new(map),
        });
        Ok(ac)
    }
}

impl AccountStoreInterface for AccountStore {
    fn update_entry(ac: &Address, ent: &AccountEntry) -> bool {
        true
    }

    fn update_value(ac: &Address, val: f64) -> bool {
        true
    }

    fn set_nonce(ac: &Address, nonce: u64) -> bool {
        true
    }

    fn get_value(ac: &Address) -> Option<f64> {
        None
    }

    fn get_nonce(ac: &Address) -> Option<u64> {
        None
    }

    fn inc_nonce(ac: &Address) -> bool {
        true
    }

    fn exist(ac: &Address) -> bool {
        true
    }
}

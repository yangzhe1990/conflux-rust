extern crate ethereum_types;
extern crate parking_lot;
extern crate network;

mod api;

use network::Error;
use std::collections::HashMap;
use std::collections::hash_map::Entry::{Occupied, Vacant};
use ethereum_types::{Address};
use api::AccountStoreInterface;
use std::sync::{Arc};
use parking_lot::RwLock;

/// TODO: use per account rwlock to improve concurrency
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
    fn update_entry(&self, ac: &Address, ent: AccountEntry) -> Option<AccountEntry> {
        let mut ac_table = self.table.write();
        ac_table.insert(ac.clone(), ent)
    }

    fn update_value(&self, ac: &Address, val: f64) -> bool {
        let mut ac_table = self.table.write();
        if let Some(entry) = ac_table.get_mut(ac) {
            entry.val = val;
            true
        } else {
            false
        }
    }

    fn set_nonce(&self, ac: &Address, nonce: u64) -> bool {
        let mut ac_table = self.table.write();
        if let Some(entry) = ac_table.get_mut(ac) {
            entry.nonce = nonce;
            true
        } else {
            false
        }
    }

    fn get_value(&self, ac: &Address) -> Option<f64> {
        let ac_table = self.table.read();
        if let Some(entry) = ac_table.get(ac) {
            Some(entry.val)
        } else {
            None
        }
    }

    fn get_nonce(&self, ac: &Address) -> Option<u64> {
        let ac_table = self.table.read();
        if let Some(entry) = ac_table.get(ac) {
            Some(entry.nonce)
        } else {
            None
        }        
    }

    fn inc_nonce(&self, ac: &Address) -> Option<u64> {
        let mut ac_table = self.table.write();
        if let Some(entry) = ac_table.get_mut(ac) {
            entry.nonce += 1;
            Some(entry.nonce)            
        } else {
            None
        }
    }

    fn exist(&self, ac: &Address) -> bool {
        let ac_table = self.table.read();
        ac_table.contains_key(ac)
    }
}

extern crate ethereum_types;
extern crate network;
extern crate parking_lot;

mod api;

use api::AccountStore as AccountStoreTrait;
use ethereum_types::Address;
use network::Error;
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;

pub struct AccountState {
    val: f64,
}

pub struct AccountEntry {
    latest_version: u64,
    nonce: u64,
    versioned_states: BTreeMap<u64, AccountState>,
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

impl AccountStoreTrait for AccountStore {
    fn update_entry(
        &self, ac: &Address, ent: AccountEntry,
    ) -> Option<AccountEntry> {
        let mut ac_table = self.table.write();
        ac_table.insert(ac.clone(), ent)
    }

    fn update_value(&self, ac: &Address, val: f64, ver: u64) -> bool {
        let mut ac_table = self.table.write();
        if let Some(entry) = ac_table.get_mut(ac) {
            if entry.latest_version < ver {
                entry.latest_version = ver;
            }
            entry
                .versioned_states
                .entry(ver)
                .or_insert(AccountState { val: 0.0 })
                .val = val;
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
            let ver = entry.latest_version;
            if let Some(state) = entry.versioned_states.get(&ver) {
                Some(state.val)
            } else {
                None
            }
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

    fn rollback_to(&self, ver: u64) {}

    fn get_snapshot(&self, ver: u64) -> HashMap<Address, AccountEntry> {
        HashMap::new()
    }
}

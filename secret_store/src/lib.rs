extern crate ethkey;
extern crate parking_lot;
extern crate rustc_hex;

use ethkey::{KeyPair, Secret};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use rustc_hex::ToHex;

pub struct StoreInner {
    account_vec: Vec<KeyPair>,
    secret_map: HashMap<String, usize>,
}

impl StoreInner {
    pub fn new() -> Self {
        StoreInner {
            account_vec: Vec::new(),
            secret_map: HashMap::new(),
        }
    }

    pub fn insert(&mut self, kp: KeyPair) -> bool {
        let secret_string = kp.secret().to_hex();
        if self.secret_map.contains_key(&secret_string) {
            return false;
        }

        let index = self.count();
        self.secret_map.insert(secret_string, index);
        self.account_vec.push(kp);
        true
    }

    pub fn count(&self) -> usize {
        self.account_vec.len()
    }

    pub fn get_keypair(&self, index: usize) -> KeyPair {
        self.account_vec[index].clone()
    }
}

pub struct SecretStore {
    store: RwLock<StoreInner>,
}

pub type SecretStoreRef = Arc<SecretStore>;

impl SecretStore {
    pub fn new() -> Self {
        SecretStore {
            store: RwLock::new(StoreInner::new()),
        }
    }

    pub fn insert(&self, kp: KeyPair) -> bool {
        self.store.write().insert(kp)
    }

    pub fn count(&self) -> usize {
        self.store.read().count()
    }

    pub fn get_keypair(&self, index: usize) -> KeyPair {
        self.store.read().get_keypair(index)
    }
}


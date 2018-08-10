use ethereum_types::Address;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use transaction::Transaction;

/// Single account in the system
pub struct Account {
    balance: f64,
    nonce: u64,
}

impl Account {
    fn new() -> Self {
        Account {
            balance: 0f64,
            nonce: 064,
        }
    }
}

/// Representation of the entire state of all accounts in the system.
pub struct State {
    accounts: Arc<RwLock<HashMap<Address, Account>>>,
}

impl State {
    pub fn new() -> State {
        State {
            accounts: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn verify(&self, txn: &Transaction) -> bool {
        let accounts = self.accounts.read();
        let sender = accounts.get(&txn.sender);
        if let Some(sender) = sender {
            if (sender.nonce != txn.nonce) || sender.balance < txn.value {
                false
            } else {
                true
            }
        } else {
            false
        }
    }

    fn remove_balance(&self, a: &Address, value: f64) {
        let mut accounts = self.accounts.write();
        let acc = accounts.get_mut(a);
        if let Some(acc) = acc {
            acc.balance -= value;
        }
    }

    fn add_balance(&self, a: &Address, value: f64) {
        let mut accounts = self.accounts.write();
        let acc = accounts.entry(*a).or_insert(Account::new());
        acc.balance += value;
    }

    pub fn execute(&self, txn: &Transaction) {
        if !self.verify(txn) {
            return;
        }
        self.remove_balance(&txn.sender, txn.value);
        self.add_balance(&txn.sender, txn.value);
    }
}

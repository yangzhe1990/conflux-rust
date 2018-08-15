use ethereum_types::Address;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use transaction::Transaction;

lazy_static! {
    pub static ref COINBASE_ADDRESS: Address = Address::zero();
}

/// Single account in the system
pub struct Account {
    balance: f64,
    nonce: u64,
}

impl Account {
    fn new() -> Self {
        Account {
            balance: 0f64,
            nonce: 0u64,
        }
    }

    pub fn balance(&self) -> f64 { self.balance }
}

/// Representation of the entire state of all accounts in the system.
pub struct State {
    pub accounts: Arc<RwLock<HashMap<Address, Account>>>,
}

impl State {
    pub fn new() -> State {
        let mut accounts: HashMap<Address, Account> = HashMap::new();
        accounts.insert(
            *COINBASE_ADDRESS,
            Account {
                balance: 1_000_000_000f64,
                nonce: 0,
            },
        );

        State {
            accounts: Arc::new(RwLock::new(accounts)),
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

    pub fn remove_balance(&self, a: &Address, value: f64) {
        let mut accounts = self.accounts.write();
        let acc = accounts.get_mut(a);
        if let Some(acc) = acc {
            acc.balance -= value;
        }
    }

    pub fn add_balance(&self, a: &Address, value: f64) {
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

    pub fn clear(&self) {}
}

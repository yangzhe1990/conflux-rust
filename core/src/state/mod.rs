use ethereum_types::Address;
use ethkey::{public_to_address, Generator, KeyPair, Random};
use network::Error;
use parking_lot::RwLock;
use rand::prelude::*;
use secret_store::SecretStoreRef;
use std::collections::{HashMap, HashSet};
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
pub struct AccountState {
    pub accounts: RwLock<HashMap<Address, Account>>,
}

pub type AccountStateRef = Arc<AccountState>;

impl AccountState {
    pub fn new() -> AccountState {
        let mut accounts: HashMap<Address, Account> = HashMap::new();
        accounts.insert(
            *COINBASE_ADDRESS,
            Account {
                balance: 1_000_000_000f64,
                nonce: 0,
            },
        );

        AccountState {
            accounts: RwLock::new(accounts),
        }
    }

    pub fn new_ref() -> AccountStateRef { Arc::new(Self::new()) }

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

    // This is for purpose of test
    pub fn import_random_accounts(
        &self, secret_store: SecretStoreRef,
    ) -> Result<(), Error> {
        let mut account_count: u32 = 0;
        let mut account_set: HashSet<Address> = HashSet::new();

        loop {
            account_count += 1;

            let kp = Random.generate()?;
            let account_address = public_to_address(kp.public());

            let mut rng = thread_rng();
            let mut balance: f64 = rng.gen();
            balance *= 10_000f64;

            if account_set.contains(&account_address) {
                account_count -= 1;
            } else {
                account_set.insert(account_address);
                secret_store.insert(kp);
                self.accounts.write().insert(
                    account_address,
                    Account {
                        balance: balance,
                        nonce: 0,
                    },
                );
            }

            if account_count >= 10 {
                break;
            }
        }

        Ok(())
    }
}

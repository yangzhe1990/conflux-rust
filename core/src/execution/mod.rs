use ethereum_types::{Address, H256, U256};
use ethkey::{public_to_address, Generator, Random};
use network::Error;
use parking_lot::RwLock;
use primitives::SignedTransaction;
use rand::prelude::*;
use secret_store::SharedSecretStore;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

lazy_static! {
    pub static ref TEST_ADDRESS: Address = Address::zero();
}

pub mod storage_key;

pub type EpochId = H256;

/// TODO: wrap primitives::Account for data, hold reference to the state
/// where transaction executes upon, and add methods to interact with accounts.
///
/// Single account in the system
pub struct Account {
    balance: U256,
    nonce: U256,
}

impl Account {
    fn new() -> Self {
        Account {
            balance: U256::zero(),
            nonce: U256::zero(),
        }
    }

    pub fn balance(&self) -> U256 { self.balance }
}

/// TODO: There is no reason to keep a collection of Accounts in memory because
/// Account isn't cacheable across epochs due to unpredicable block execution
/// order. Therefore caching at state level is the right place.
///
/// Representation of the entire state of all accounts in the system.
pub struct AccountState {
    pub accounts: RwLock<HashMap<Address, Account>>,
}

pub type AccountStateRef = Arc<AccountState>;

impl AccountState {
    pub fn new() -> AccountState {
        let mut accounts: HashMap<Address, Account> = HashMap::new();
        accounts.insert(
            *TEST_ADDRESS,
            Account {
                balance: U256::from(1_000_000_000u64),
                nonce: U256::zero(),
            },
        );

        AccountState {
            accounts: RwLock::new(accounts),
        }
    }

    pub fn new_ref() -> AccountStateRef { Arc::new(Self::new()) }

    fn verify(&self, txn: &SignedTransaction) -> bool {
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

    pub fn get_nonce(&self, a: &Address) -> Option<U256> {
        let accounts = self.accounts.read();
        let acc = accounts.get(a);
        if let Some(acc) = acc {
            return Some(acc.nonce);
        }

        None
    }

    pub fn get_balance(&self, a: &Address) -> Option<U256> {
        let accounts = self.accounts.read();
        let acc = accounts.get(a);
        if let Some(acc) = acc {
            return Some(acc.balance);
        }

        None
    }

    pub fn remove_balance(&self, a: &Address, value: U256) {
        let mut accounts = self.accounts.write();
        let acc = accounts.get_mut(a);
        if let Some(acc) = acc {
            acc.balance -= value;
        }
    }

    pub fn add_balance(&self, a: &Address, value: U256) {
        let mut accounts = self.accounts.write();
        let acc = accounts.entry(*a).or_insert(Account::new());
        acc.balance += value;
    }

    pub fn execute(&self, txn: &SignedTransaction) {
        if !self.verify(txn) {
            return;
        }
        self.remove_balance(&txn.sender, txn.value);
        self.add_balance(&txn.sender, txn.value);
    }

    pub fn clear(&self) {}

    // This is for purpose of test
    pub fn import_random_accounts(
        &self, secret_store: SharedSecretStore,
    ) -> Result<(), Error> {
        let mut account_count: u32 = 0;
        let mut account_set: HashSet<Address> = HashSet::new();

        loop {
            account_count += 1;

            let kp = Random.generate()?;
            let account_address = public_to_address(kp.public());

            let mut rng = thread_rng();
            let mut balance = U256::from(rng.gen::<u64>());

            if account_set.contains(&account_address) {
                account_count -= 1;
            } else {
                account_set.insert(account_address);
                secret_store.insert(kp);
                self.accounts.write().insert(
                    account_address,
                    Account {
                        balance: balance,
                        nonce: U256::zero(),
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

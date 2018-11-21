use super::{State, StateTrait};
use ethereum_types::{Address, U256};
use primitives::{Account, EpochId, SignedTransaction};
use rlp::{decode, encode};
use std::collections::HashMap;

pub mod storage_key;

pub fn get_account(state: &State, address: &Address) -> Option<Account> {
    state
        .get(address.as_ref())
        .map(|rlp| decode::<Account>(rlp.as_ref()).unwrap())
        .ok()
}

/// FIXME: Remove this once we finished the executor implementation
#[allow(dead_code)]
/// 'state always outlives 'executor
pub struct Executor<'executor, 'state: 'executor> {
    state: &'executor mut State<'state>,
    cache: HashMap<Address, Account>,
}

impl<'executor, 'state> Executor<'executor, 'state> {
    pub fn new(state: &'executor mut State<'state>) -> Self {
        Executor {
            state,
            cache: HashMap::new(),
        }
    }

    pub fn apply(&mut self, transaction: &SignedTransaction) {
        if !self.cache.contains_key(&transaction.sender) {
            self.cache.insert(
                transaction.sender.clone(),
                get_account(self.state, &transaction.sender)
                    .unwrap_or_default(),
            );
        }
        if !self.cache.contains_key(&transaction.receiver) {
            self.cache.insert(
                transaction.receiver.clone(),
                get_account(self.state, &transaction.receiver)
                    .unwrap_or_default(),
            );
        }

        // check nonce
        if !self.check_increment_nonce_in_cache(transaction) {
            return;
        }

        if transaction.value <= self.cache[&transaction.sender].balance {
            self.cache
                .entry(transaction.sender.clone())
                .and_modify(|account| account.balance -= transaction.value);
            self.cache
                .entry(transaction.receiver.clone())
                .and_modify(|account| account.balance += transaction.value);
        }
    }

    pub fn check_increment_nonce_in_cache(
        &mut self, transaction: &SignedTransaction,
    ) -> bool {
        let sender_account = self.cache.get_mut(&transaction.sender).unwrap();
        if transaction.nonce != sender_account.nonce {
            warn!("Transaction aborted due to outdated nonce. (tx nonce: {:?}, account nonce: {:?})", transaction.nonce, sender_account.nonce);
            return false;
        }
        let adder = U256::from(1);
        sender_account.nonce = sender_account.nonce + adder;
        true
    }

    pub fn commit(&mut self, epoch_id: EpochId) {
        for (k, v) in self.cache.iter() {
            self.state.set(k.as_ref(), encode(v).as_ref()).ok();
        }
        self.state.commit(epoch_id);
    }
}

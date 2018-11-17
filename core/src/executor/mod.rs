use super::{State, StateTrait};
use ethereum_types::{Address};
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

        if transaction.value <= self.cache[&transaction.sender].balance {
            self.cache
                .entry(transaction.sender.clone())
                .and_modify(|account| account.balance -= transaction.value);
            self.cache
                .entry(transaction.receiver.clone())
                .and_modify(|account| account.balance += transaction.value);
        }
    }

    pub fn commit(&mut self, epoch_id: EpochId) {
        for (k, v) in self.cache.iter() {
            self.state.set(k.as_ref(), encode(v).as_ref()).ok();
        }
        self.state.commit(epoch_id);
    }
}

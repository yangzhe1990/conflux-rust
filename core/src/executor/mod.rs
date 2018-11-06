use super::State;
use ethereum_types::{Address, U256};
use primitives::SignedTransaction;

pub mod storage_key;

pub fn get_balance(state: &State, address: &Address) -> Option<U256> {
    unimplemented!()
}

pub fn get_nonce(state: &State, address: &Address) -> Option<U256> {
    unimplemented!()
}

/// FIXME: Remove this once we finished the executor implementation
#[allow(dead_code)]
/// 'state always outlives 'executor
pub struct Executor<'executor, 'state: 'executor> {
    state: &'executor mut State<'state>,
}

impl<'executor, 'state> Executor<'executor, 'state> {
    pub fn new(state: &'executor mut State<'state>) -> Self {
        Executor { state }
    }

    pub fn apply(&self, _transaction: &SignedTransaction) {}
}

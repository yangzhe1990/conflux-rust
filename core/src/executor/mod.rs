use super::State;
use primitives::SignedTransaction;

pub mod storage_key;

/// 'state always outlives 'executor
pub struct Executor<'executor, 'state: 'executor> {
    state: &'executor mut State<'state>,
}

impl<'executor, 'state> Executor<'executor, 'state> {
    pub fn new(state: &'executor mut State<'state>) -> Self {
        Executor { state }
    }

    pub fn apply(&self, transaction: &SignedTransaction) {}
}

extern crate core;

use std::sync::Arc;
use core::TransactionExecutor;

/// The VM for processing transactions in Conflux
pub struct ConfluxVM {
}

impl ConfluxVM {
    pub fn new() -> ConfluxVM {
        ConfluxVM {}
    }
}

impl TransactionExecutor for ConfluxVM {
}

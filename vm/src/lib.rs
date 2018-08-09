extern crate core;

use core::ledger::BlockNumber;
use core::ledger::LedgerRef;
use core::LedgerExecutor;
use std::sync::Arc;

/// The VM for processing transactions in Conflux
pub struct ConfluxVM {}

impl ConfluxVM {
    pub fn new() -> ConfluxVM { ConfluxVM {} }
}

impl LedgerExecutor for ConfluxVM {
    fn on_new_best_block(&self, number: BlockNumber, ledger: LedgerRef) {}
}

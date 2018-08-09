extern crate core;

use core::TransactionExecutor;
use std::sync::Arc;

/// The VM for processing transactions in Conflux
pub struct ConfluxVM {}

impl ConfluxVM {
    pub fn new() -> ConfluxVM { ConfluxVM {} }
}

impl TransactionExecutor for ConfluxVM {}

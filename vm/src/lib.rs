extern crate core;

use std::sync::Arc;
use core::ExecEngineInterface;

/// The VM for processing transactions in Conflux
pub struct ConfluxVM {
}

impl ConfluxVM {
    pub fn new() -> Arc<ConfluxVM> {
        Arc::new(ConfluxVM {
        })
    }
}

impl ExecEngineInterface for ConfluxVM {
}

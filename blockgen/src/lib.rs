extern crate core;

use core::LedgerCore;
use std::sync::Arc;

/// The interface for a conflux block generator
pub trait BlockGenerator {
    /// Start the block generator to generate blocks actively
    fn start();

    /// Stop the block generator to generate blocks
    fn stop();

    /// Force the block generator to generate one block with the specificed payload size
    fn generate_block(payload_len: u32);
}

type SharedLedgerCore<T>
where T: LedgerCore
= Arc<T>;

pub struct ConfluxBlockGenerator<T> {
    core: SharedLedgerCore<T>,
}

impl<T> ConfluxBlockGenerator<T> {
    pub fn new(core: SharedLedgerCore<T>) -> ConfluxBlockGenerator<T> {
        ConfluxBlockGenerator { core: core }
    }
}

impl<T> BlockGenerator for ConfluxBlockGenerator<T> {
    fn start() {
        unimplemented!();
    }

    fn stop() {
        unimplemented!();
    }

    fn generate_block(payload_len: u32) {
        unimplemented!();
    }
}

extern crate core;

use core::LedgerRef;
use std::sync::Arc;

/// The interface for a conflux block generator
pub struct BlockGenerator;

impl BlockGenerator {
    pub fn new(ledger: LedgerRef) -> Self { BlockGenerator {} }

    /// Start the block generator to generate blocks actively
    pub fn start() {
        unimplemented!();
    }

    /// Stop the block generator to generate blocks
    pub fn stop() {
        unimplemented!();
    }

    /// Force the block generator to generate one block with the specificed payload size
    pub fn generate_block(payload_len: u32) {
        unimplemented!();
    }
}

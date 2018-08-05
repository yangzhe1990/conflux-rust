extern crate keccak_hash as hash;
extern crate ethereum_types;
extern crate common_types as types;
extern crate parity_bytes as bytes;
extern crate parking_lot;

pub mod encoded;
mod ledger;
mod executor;

use ethereum_types::{H256};
pub use types::*;
use parking_lot::RwLock;
use std::sync::{Arc};
use ledger::ledger::ConfluxLedger;
use executor::ExecEngineInterface;

pub trait LedgerEngineInterface: Send + Sync {
    /// Get gathered ledger information.
    fn ledger_info(&self) -> LedgerInfo;

    ///////////////////////////////////////////////////////

    /// Get block information 
    /// Get raw block header data by block id.
    fn block_header(&self, id: BlockId) -> Option<encoded::Header>;

    ///////////////////////////////////////////////////////

    /// Get block hash.
    fn block_hash(&self, id: BlockId) -> Option<H256>;

}

pub struct LedgerEngine {    
    ledger: RwLock<Arc<ConfluxLedger>>,
    executor: Arc<ExecEngineInterface>,
}

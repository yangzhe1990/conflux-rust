mod ledger;

use encoded;
use ethereum_types::{H256};
pub use types::*;
use parking_lot::RwLock;
use std::sync::{Arc};
use ledger::ledger::ConfluxLedger;
use vm::ExecEngineInterface;

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

impl LedgerEngine {
    fn block_hash(ledger: &ConfluxLedger, id: BlockId) -> Option<H256> {
		match id {
			BlockId::Hash(hash) => Some(hash),
			BlockId::Number(number) => ledger.block_hash(number),
			BlockId::Earliest => ledger.block_hash(0),
			BlockId::Latest => Some(ledger.best_block_hash()),
		}
	}
}

impl LedgerEngineInterface for LedgerEngine {
    fn ledger_info(&self) -> LedgerInfo {
        let ledger_info = self.ledger.read().ledger_info();
        ledger_info
    }

    fn block_header(&self, id: BlockId) -> Option<encoded::Header> {
        let ledger = self.ledger.read();
        Self::block_hash(&ledger, id).and_then(|hash| ledger.block_header_data(&hash))
    }

    fn block_hash(&self, id: BlockId) -> Option<H256> {
        let ledger = self.ledger.read();
        Self::block_hash(&ledger, id)
    }
}

use state::State;
use types::{BlockId, BlockNumber};
use LedgerRef;

use ethereum_types::H256;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

pub struct ExecutionEngine {
    ledger: LedgerRef,
    pub state: State,

    last_block_number: BlockNumber,
    block_hashes: Arc<RwLock<HashMap<BlockNumber, H256>>>,
}

pub type ExecutionEngineRef = Arc<ExecutionEngine>;

impl ExecutionEngine {
    pub fn new(ledger: LedgerRef) -> Self {
        ExecutionEngine {
            ledger: ledger,
            state: State::new(),
            last_block_number: 0,
            block_hashes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn new_ref(ledger: LedgerRef) -> ExecutionEngineRef {
        Arc::new(Self::new(ledger))
    }

    pub fn execute_up_to(&self, index: BlockNumber) {
        let mut block_hashes = self.block_hashes.write();

        let mut block_number = 0;
        while block_number <= index {
            let block_hash = self
                .ledger
                .block_hash(BlockId::Number(block_number))
                .unwrap();
            if block_hashes.get(&block_number) == Some(&block_hash) {
                block_number += 1;
            } else {
                break;
            }
        }

        if block_number == self.last_block_number {
            block_number += 1;
            while block_number <= index {
                let block_hash = self
                    .ledger
                    .block_hash(BlockId::Number(block_number))
                    .unwrap();
                block_hashes.insert(block_number, block_hash);

                let block = self.ledger.block_body_data(&block_hash).unwrap();
                for txn in &block.transactions {
                    self.state.execute(txn);
                }

                block_number += 1;
            }
        } else {
            self.state.clear();
            block_hashes.clear();

            for block_number in 0..(index + 1) {
                let block_hash = self
                    .ledger
                    .block_hash(BlockId::Number(block_number))
                    .unwrap();
                block_hashes.insert(block_number, block_hash);

                let block = self.ledger.block_body_data(&block_hash).unwrap();
                for txn in &block.transactions {
                    self.state.execute(txn);
                }
            }
        }
    }
}

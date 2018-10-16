use primitives::EpochNumber;
use state::AccountStateRef;
use LedgerRef;

use ethereum_types::H256;
use parking_lot::RwLock;
use std::{collections::HashMap, sync::Arc};

pub struct ExecutionEngine {
    ledger: LedgerRef,
    pub state: AccountStateRef,

    epoch_number: EpochNumber,
    epoch_hashes: Arc<RwLock<HashMap<EpochNumber, H256>>>,
}

pub type ExecutionEngineRef = Arc<ExecutionEngine>;

impl ExecutionEngine {
    pub fn new(ledger: LedgerRef, state: AccountStateRef) -> Self {
        ExecutionEngine {
            ledger: ledger,
            state: state,
            epoch_number: 0,
            epoch_hashes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn new_ref(
        ledger: LedgerRef, state: AccountStateRef,
    ) -> ExecutionEngineRef {
        Arc::new(Self::new(ledger, state))
    }

    pub fn execute_up_to(&self, number: EpochNumber) {
        let mut epoch_hashes = self.epoch_hashes.write();

        let mut epoch_number = 0;
        while epoch_number <= number {
            let epoch_hash = self.ledger.epoch_hash(epoch_number).unwrap();
            if epoch_hashes.get(&epoch_number) == Some(&epoch_hash) {
                epoch_number += 1;
            } else {
                break;
            }
        }

        if epoch_number == self.epoch_number {
            epoch_number += 1;
            while epoch_number <= number {
                let epoch_hash = self.ledger.epoch_hash(epoch_number).unwrap();
                epoch_hashes.insert(epoch_number, epoch_hash);

                let transactions = self
                    .ledger
                    .epoch_transactions_by_hash(&epoch_hash)
                    .unwrap();
                for transaction in &transactions {
                    self.state.execute(transaction);
                }
                epoch_number += 1;
            }
        } else {
            self.state.clear();
            epoch_hashes.clear();

            for epoch_number in 0..(number + 1) {
                let epoch_hash = self.ledger.epoch_hash(epoch_number).unwrap();
                epoch_hashes.insert(epoch_number, epoch_hash);

                let transactions = self
                    .ledger
                    .epoch_transactions_by_hash(&epoch_hash)
                    .unwrap();
                for transaction in &transactions {
                    self.state.execute(transaction);
                }
            }
        }
    }
}

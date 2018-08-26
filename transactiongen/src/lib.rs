extern crate core;
extern crate parking_lot;

use std::{thread, time};
use std::sync::Arc;
pub use core::execution_engine::{ExecutionEngine, ExecutionEngineRef};
use parking_lot::RwLock;

enum TransGenState {
    Start,
    Stop,
}

pub struct TransactionGenerator {
    state: RwLock<TransGenState>,
    exec_engine: ExecutionEngineRef,
}

impl TransactionGenerator {
    pub fn new(engine: ExecutionEngineRef) -> Self {
        TransactionGenerator {
            exec_engine: engine,
            state: RwLock::new(TransGenState::Start),
        }
    }

    pub fn generate_transactions(txgen: Arc<TransactionGenerator>) {
        let interval = time::Duration::from_millis(100);

        // Get initial account list


        loop {
            match *txgen.state.read() {
                TransGenState::Stop => return,
                _ => {}
            }

            // Randomly select sender and receiver.
            // Sender must exist in the account list.
            // Receiver can be not in the account list which 
            // leads to generate a new account
            

            // Randomly generate the to-be-transferred value
            // based on the balance of sender

            // Generate the transaction, sign it, and push into the transaction pool

            thread::sleep(interval);
        }
    }
}

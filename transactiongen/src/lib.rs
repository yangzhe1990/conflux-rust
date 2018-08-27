extern crate core;
extern crate ethkey;
extern crate network;
extern crate parking_lot;
extern crate rand;
extern crate secret_store;

pub use core::execution_engine::{ExecutionEngine, ExecutionEngineRef};
use core::state::AccountStateRef;
use ethkey::{public_to_address, Generator, KeyPair, Random};
use network::Error;
use parking_lot::RwLock;
use rand::prelude::*;
use secret_store::SecretStoreRef;
use std::sync::Arc;
use std::{thread, time};

enum TransGenState {
    Start,
    Stop,
}

pub struct TransactionGenerator {
    state: RwLock<TransGenState>,
    exec_engine: ExecutionEngineRef,
    secret_store: SecretStoreRef,
    account_state: AccountStateRef,
}

impl TransactionGenerator {
    pub fn new(
        engine: ExecutionEngineRef, secret_store: SecretStoreRef,
        account_state: AccountStateRef,
    ) -> Self
    {
        TransactionGenerator {
            exec_engine: engine,
            state: RwLock::new(TransGenState::Start),
            secret_store,
            account_state,
        }
    }

    pub fn generate_transactions(
        txgen: Arc<TransactionGenerator>,
    ) -> Result<(), Error> {
        let interval = time::Duration::from_millis(100);

        loop {
            match *txgen.state.read() {
                TransGenState::Stop => return Ok(()),
                _ => {}
            }

            // Randomly select sender and receiver.
            // Sender must exist in the account list.
            // Receiver can be not in the account list which
            // leads to generate a new account
            let account_count = txgen.secret_store.count();
            let mut sender_index: usize = random();
            sender_index %= account_count;
            let sender_kp = txgen.secret_store.get_keypair(sender_index);

            let mut receiver_index: usize = random();
            receiver_index =
                (sender_index + (receiver_index % account_count) + 1)
                    % account_count;

            let mut receiver_kp: KeyPair;
            if sender_index == receiver_index {
                // Create a new receiver account
                loop {
                    receiver_kp = Random.generate()?;
                    if txgen.secret_store.insert(receiver_kp.clone()) {
                        break;
                    }
                }
            } else {
                receiver_kp = txgen.secret_store.get_keypair(receiver_index);
            }

            // Randomly generate the to-be-transferred value
            // based on the balance of sender

            // Generate the transaction, sign it, and push into the transaction pool

            thread::sleep(interval);
        }

        Ok(())
    }
}

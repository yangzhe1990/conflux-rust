extern crate core;
extern crate ethereum_types;
extern crate ethkey;
extern crate network;
extern crate parking_lot;
extern crate rand;
extern crate secret_store;

pub use core::execution_engine::{ExecutionEngine, ExecutionEngineRef};
use core::state::AccountStateRef;
use core::transaction::Transaction;
use ethereum_types::Address;
use ethkey::{public_to_address, Generator, KeyPair, Random};
use network::Error;
use parking_lot::RwLock;
use rand::prelude::*;
use secret_store::SecretStoreRef;
use std::collections::HashMap;
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
        let mut nonce_map: HashMap<Address, u64> = HashMap::new();

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
            let sender_address = public_to_address(sender_kp.public());
            let sender_balance =
                txgen.account_state.get_balance(&sender_address);
            if sender_balance == None {
                thread::sleep(interval);
                continue;
            }
            let sender_balance = sender_balance.unwrap();

            let mut rng = thread_rng();
            let mut balance_to_transfer: f64 = rng.gen();
            balance_to_transfer *= sender_balance;

            // Generate nonce for the transaction
            let sender_state_nonce =
                txgen.account_state.get_nonce(&sender_address).unwrap();
            let entry = nonce_map
                .entry(sender_address)
                .or_insert(sender_state_nonce);
            if sender_state_nonce > *entry {
                *entry = sender_state_nonce;
            }

            let sender_nonce = *entry;
            *entry += 1;

            // Generate the transaction, sign it, and push into the transaction pool
            let receiver_address = public_to_address(receiver_kp.public());
            let tx = Transaction {
                nonce: sender_nonce,
                gas_price: 0.001,
                gas: 200,
                value: balance_to_transfer,
                receiver: receiver_address,
            };

            let signed_tx = tx.sign(sender_kp.secret());

            thread::sleep(interval);
        }

        Ok(())
    }
}

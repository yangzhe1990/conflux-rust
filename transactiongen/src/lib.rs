extern crate core;
extern crate ethereum_types;
extern crate ethkey;
extern crate network;
extern crate parking_lot;
extern crate primitives;
extern crate rand;
extern crate secret_store;

use core::{
    get_account, SharedConsensusGraph, SharedTransactionPool, State,
    StateManager, StateManagerTrait,
};
use ethereum_types::{Address, U256};
use ethkey::{public_to_address, Generator, KeyPair, Random};
use network::Error;
use parking_lot::RwLock;
use primitives::Transaction;
use rand::prelude::*;
use secret_store::SharedSecretStore;
use std::{collections::HashMap, sync::Arc, thread, time};

enum TransGenState {
    Start,
    Stop,
}

pub struct TransactionGenerator {
    consensus: SharedConsensusGraph,
    state_manager: Arc<StateManager>,
    txpool: SharedTransactionPool,
    secret_store: SharedSecretStore,
    state: RwLock<TransGenState>,
}

impl TransactionGenerator {
    pub fn new(
        consensus: SharedConsensusGraph, state_manager: Arc<StateManager>,
        txpool: SharedTransactionPool, secret_store: SharedSecretStore,
    ) -> Self
    {
        TransactionGenerator {
            consensus,
            state_manager,
            txpool,
            secret_store,
            state: RwLock::new(TransGenState::Start),
        }
    }

    pub fn generate_transactions(
        txgen: Arc<TransactionGenerator>,
    ) -> Result<(), Error> {
        let interval = time::Duration::from_millis(100);
        let mut nonce_map: HashMap<Address, U256> = HashMap::new();

        loop {
            match *txgen.state.read() {
                TransGenState::Stop => return Ok(()),
                _ => {}
            }

            let state = txgen
                .state_manager
                .get_state_at(txgen.consensus.best_block_hash());

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
            let sender_balance = get_account(&state, &sender_address)
                .map(|account| account.balance);
            if sender_balance == None {
                thread::sleep(interval);
                continue;
            }
            let sender_balance = sender_balance.unwrap();

            let mut rng = thread_rng();
            let mut balance_to_transfer: U256 = U256::from(rng.gen::<u64>());
            balance_to_transfer *= sender_balance;

            // Generate nonce for the transaction
            let sender_state_nonce = get_account(&state, &sender_address)
                .map(|account| account.nonce)
                .unwrap();
            let entry = nonce_map
                .entry(sender_address)
                .or_insert(sender_state_nonce);
            if sender_state_nonce > *entry {
                *entry = sender_state_nonce;
            }

            let sender_nonce = *entry;
            *entry += U256::one();

            // Generate the transaction, sign it, and push into the transaction
            // pool
            let receiver_address = public_to_address(receiver_kp.public());
            let tx = Transaction {
                nonce: sender_nonce,
                gas_price: U256::from(1_000_000_000_000_000u64),
                gas: U256::from(200u64),
                value: balance_to_transfer,
                receiver: receiver_address,
            };

            let signed_tx = tx.sign(sender_kp.secret());
            txgen.txpool.add(signed_tx);

            thread::sleep(interval);
        }

        Ok(())
    }
}

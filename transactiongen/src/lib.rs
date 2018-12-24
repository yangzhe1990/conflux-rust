extern crate core;
extern crate ethcore_bytes as bytes;
extern crate ethereum_types;
extern crate ethkey;
extern crate network;
extern crate parking_lot;
extern crate primitives;
extern crate rand;
extern crate secret_store;
#[macro_use]
extern crate log;

use crate::bytes::Bytes;
use core::{
    state::State,
    statedb::StateDb,
    storage::{StorageManager, StorageManagerTrait},
    SharedConsensusGraph, SharedTransactionPool,
};
use ethereum_types::{Address, H512, U256, U512};
use ethkey::{public_to_address, Generator, KeyPair, Random};
use network::Error;
use parking_lot::RwLock;
use primitives::{transaction::Action, SignedTransaction, Transaction};
use rand::prelude::*;
use secret_store::{SecretStore, SharedSecretStore};
use std::{collections::HashMap, sync::Arc, thread, time};

#[allow(unused)]
enum TransGenState {
    Start,
    Stop,
}

pub struct TransactionGeneratorConfig {
    pub generate_tx: bool,
    pub period: time::Duration,
}

impl TransactionGeneratorConfig {
    pub fn new(generate_tx: bool, period_ms: u64) -> Self {
        TransactionGeneratorConfig {
            generate_tx,
            period: time::Duration::from_millis(period_ms),
        }
    }
}

pub struct TransactionGenerator {
    pub consensus: SharedConsensusGraph,
    storage_manager: Arc<StorageManager>,
    txpool: SharedTransactionPool,
    secret_store: SharedSecretStore,
    state: RwLock<TransGenState>,
    key_pair: Option<KeyPair>,
}

pub type SharedTransactionGenerator = Arc<TransactionGenerator>;

impl TransactionGenerator {
    pub fn new(
        consensus: SharedConsensusGraph, storage_manager: Arc<StorageManager>,
        txpool: SharedTransactionPool, secret_store: SharedSecretStore,
        key_pair: Option<KeyPair>,
    ) -> Self
    {
        TransactionGenerator {
            consensus,
            storage_manager,
            txpool,
            secret_store,
            state: RwLock::new(TransGenState::Start),
            key_pair,
        }
    }

    pub fn get_best_state(&self) -> State {
        State::new(
            StateDb::new(
                self.storage_manager
                    .get_state_at(self.consensus.best_block_hash())
                    .unwrap(),
            ),
            0.into(),
            Default::default(),
        )
    }

    pub fn generate_transaction(&self) -> SignedTransaction {
        // Generate new address with 10% probability
        trace!("start generating");
        let is_send_to_new_address = (rand::thread_rng().gen_range(0, 10) == 0)
            || (self.secret_store.count() < 10);
        let receiver_address = match is_send_to_new_address {
            false => {
                let account_count = self.secret_store.count();
                let index: usize = random::<usize>() % account_count;
                let kp = self.secret_store.get_keypair(index);
                public_to_address(kp.public())
            }
            true => {
                let kp = Random.generate().expect("Fail to generate KeyPair.");
                self.secret_store.insert(kp.clone());
                public_to_address(kp.public())
            }
        };

        trace!("try to get state");
        let account_count = self.secret_store.count();
        let sender_index: usize = random::<usize>() % account_count;
        let sender_kp = self.secret_store.get_keypair(sender_index);
        let sender_address = public_to_address(sender_kp.public());

        let state = self.get_best_state();

        debug!(
            "account_count:{} sender_addr:{:?} epoch_id:{:?}",
            account_count,
            sender_address,
            self.consensus.best_block_hash()
        );
        let sender_balance = state.balance(&sender_address).unwrap_or(0.into());

        let sender_nonce = state.nonce(&sender_address).unwrap_or(0.into());

        let mut balance_to_transfer: U256 = 0.into();
        if sender_balance > 0.into() {
            balance_to_transfer = U256::from(
                U512::from(H512::random()) % U512::from(sender_balance),
            );
        }
        trace!("before signing");

        let tx = Transaction {
            nonce: sender_nonce,
            gas_price: U256::from(100u64),
            gas: U256::from(1u64),
            value: balance_to_transfer,
            action: Action::Call(receiver_address),
            data: Bytes::new(),
        };
        let r = tx.sign(sender_kp.secret());
        trace!("after signing");
        r
    }

    pub fn generate_transactions(
        txgen: Arc<TransactionGenerator>, tx_config: TransactionGeneratorConfig,
    ) -> Result<(), Error> {
        let mut nonce_map: HashMap<Address, U256> = HashMap::new();

        let key_pair = txgen.key_pair.clone().expect("should exist");
        let secret_store = SecretStore::new();
        //        let mut balance_map = HashMap::new();
        //        balance_map
        //            .insert(public_to_address(key_pair.public()),
        // U256::from(10000000));
        trace!(
            "tx_gen address={:?} pub_key={:?}",
            public_to_address(key_pair.public()),
            key_pair.public()
        );
        trace!("{:?} {:?}", tx_config.generate_tx, tx_config.period);
        secret_store.insert(key_pair);
        loop {
            match *txgen.state.read() {
                TransGenState::Stop => return Ok(()),
                _ => {}
            }

            let state = State::new(
                StateDb::new(
                    txgen
                        .storage_manager
                        .get_state_at(txgen.consensus.best_block_hash())
                        .unwrap(),
                ),
                0.into(),
                Default::default(),
            );

            // Randomly select sender and receiver.
            // Sender must exist in the account list.
            // Receiver can be not in the account list which
            // leads to generate a new account
            let account_count = secret_store.count();
            let mut sender_index: usize = random();
            sender_index %= account_count;
            let sender_kp = secret_store.get_keypair(sender_index);

            // Randomly generate the to-be-transferred value
            // based on the balance of sender
            let sender_address = public_to_address(sender_kp.public());
            let sender_balance = state.balance(&sender_address).ok();

            trace!(
                "choose sender addr={:?} balance={:?}",
                sender_address, sender_balance
            );
            if sender_balance == None {
                thread::sleep(tx_config.period);
                continue;
            }
            if sender_balance.unwrap() < U256::from(200) {
                continue;
            }

            let mut receiver_kp: KeyPair;
            let mut receiver_index: usize = random();
            receiver_index %= account_count;
            if sender_index == receiver_index {
                // Create a new receiver account
                loop {
                    receiver_kp = Random.generate()?;
                    if secret_store.insert(receiver_kp.clone()) {
                        break;
                    }
                }
            } else {
                receiver_kp = secret_store.get_keypair(receiver_index);
            }
            let balance_to_transfer = U256::from(100);
            // Generate nonce for the transaction
            let sender_state_nonce = state.nonce(&sender_address).unwrap();
            let entry = nonce_map
                .entry(sender_address)
                .or_insert(sender_state_nonce);
            if sender_state_nonce > *entry {
                *entry = sender_state_nonce;
            }
            let sender_nonce = *entry;
            *entry += U256::one();

            let mut receiver_index: usize = random();
            receiver_index =
                (sender_index + (receiver_index % account_count) + 1)
                    % account_count;
            let mut receiver_kp: KeyPair;
            if sender_index == receiver_index {
                // Create a new receiver account
                loop {
                    receiver_kp = Random.generate()?;
                    if secret_store.insert(receiver_kp.clone()) {
                        break;
                    }
                }
            } else {
                receiver_kp = secret_store.get_keypair(receiver_index);
            }
            let receiver_address = public_to_address(receiver_kp.public());
            debug!(
                "receiver={:?} value={:?} nonce={:?}",
                receiver_address, balance_to_transfer, sender_nonce
            );
            // Generate the transaction, sign it, and push into the transaction
            // pool
            let tx = Transaction {
                nonce: sender_nonce,
                gas_price: U256::from(100u64),
                gas: U256::from(1u64),
                value: balance_to_transfer,
                action: Action::Call(receiver_address),
                data: Bytes::new(),
            };
            //            balance_map
            //                .entry(sender_address)
            //                .and_modify(|b| *b -= balance_to_transfer);
            //            
            // *balance_map.entry(receiver_address).or_insert(0.into()) +=
            //                balance_to_transfer;

            let signed_tx = tx.sign(sender_kp.secret());
            //            txgen.txpool.add_pending(signed_tx.clone());
            let mut tx_to_insert = Vec::new();
            tx_to_insert.push(signed_tx.transaction);
            txgen.txpool.insert_new_transactions(
                txgen.consensus.best_block_hash(),
                tx_to_insert,
            );
            thread::sleep(tx_config.period);
        }
    }
}

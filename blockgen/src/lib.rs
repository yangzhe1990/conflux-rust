extern crate common_types as types;
extern crate core;
extern crate ethereum_types;
extern crate keccak_hash as hash;
extern crate parking_lot;
extern crate rlp;

use core::block::Block;
use core::header::Header;
use core::sync_ctx::SyncContext;
use core::transaction::Transaction;
use core::LedgerRef;
use core::SyncEngineRef;
use ethereum_types::{Address, H256, U256};
use hash::{keccak, KECCAK_NULL_RLP};
use rlp::RlpStream;
use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::Arc;
use std::{thread, time};
use types::*;

enum MiningState {
    Start,
    Stop,
}

/// The interface for a conflux block generator
pub struct BlockGenerator {
    ledger: LedgerRef,
    sync: SyncEngineRef,
    state: RwLock<MiningState>,
}

pub type BlockGeneratorRef = Arc<BlockGenerator>;

impl BlockGenerator {
    pub fn new(ledger: LedgerRef, sync: SyncEngineRef) -> Self {
        BlockGenerator {
            ledger,
            sync,
            state: RwLock::new(MiningState::Start),
        }
    }

    /// Stop the block generator to generate blocks
    pub fn stop(&mut self) { *self.state.write() = MiningState::Stop; }

    pub fn set_problem(&self) {}

    pub fn generate_block(&self, tx_len: usize) {
        let best_block_info = self.ledger.best_block();
        let best_hash = best_block_info.header.hash();
        let best_number = best_block_info.header.number();
        let mut total_difficulty = best_block_info.total_difficulty;

        let mut header = Header::new();
        header.set_parent_hash(best_hash);
        header.set_timestamp(0);
        header.set_number(best_number + 1);
        header.set_author(Address::default());
        header.set_state_root(KECCAK_NULL_RLP);
        header.set_difficulty(10.into());
        total_difficulty = total_difficulty + 10.into();

        let mut txs: Vec<Transaction> = Vec::new();
        let mut tx_rlp = RlpStream::new_list(tx_len);
        for _i in 0..tx_len {
            let tx = Transaction {
                nonce: 0,
                value: 0.0,
                sender: Address::default(),
                receiver: Address::default(),
            };
            tx_rlp.append(&tx);
            txs.push(tx);
        }
        header.set_transactions_root(keccak(tx_rlp.out()));
        header.compute_hash();
        let hash = header.hash();

        let body = Block {
            hash: hash,
            transactions: txs,
        };

        self.ledger.add_block_header_by_hash(&hash, header);
        self.ledger.add_block_body_by_hash(&hash, body);
        self.ledger.add_child(&best_hash, &hash);

        let mut blocks_to_adjust: VecDeque<H256> = VecDeque::new();
        blocks_to_adjust.push_back(hash);
        self.ledger.adjust_main_chain(blocks_to_adjust);

        let mut hashes: Vec<H256> = Vec::new();
        hashes.push(hash);

        let mut total_difficulties: Vec<U256> = Vec::new();
        total_difficulties.push(total_difficulty);

        self.sync.new_blocks(&hashes[..], &total_difficulties[..]);
    }

    pub fn start_mining(bg: Arc<BlockGenerator>, payload_len: u32) {
        let target_interval_count: u32 = 5;
        let mut current_interval_count: u32 = 0;
        let mut current_mining_hash: Option<H256> = None;
        let mut current_mining_number: BlockNumber = 0;
        let mut current_total_difficulty: U256 = 0.into();
        let one_second = time::Duration::from_millis(1000);

        loop {
            match *bg.state.read() {
                MiningState::Stop => return,
                _ => {}
            }

            // check if mined a block
            if current_interval_count == target_interval_count {
                // mined one block
                if let Some(parent_hash) = current_mining_hash {
                    let mut header = Header::new();
                    header.set_parent_hash(parent_hash);
                    header.set_timestamp(0);
                    header.set_number(current_mining_number + 1);
                    header.set_author(Address::default());
                    header.set_transactions_root(KECCAK_NULL_RLP);
                    header.set_state_root(KECCAK_NULL_RLP);
                    header.set_difficulty(10.into());
                    let total_difficulty = current_total_difficulty + 10.into();

                    header.compute_hash();
                    let hash = header.hash();

                    let mut txs: Vec<Transaction> = Vec::new();
                    for _i in 0..100 {
                        txs.push(Transaction {
                            nonce: 0,
                            value: 0.0,
                            sender: Address::default(),
                            receiver: Address::default(),
                        });
                    }

                    let body = Block {
                        hash: hash,
                        transactions: txs,
                    };

                    bg.ledger.add_block_header_by_hash(&hash, header);
                    bg.ledger.add_block_body_by_hash(&hash, body);
                    bg.ledger.add_child(&parent_hash, &hash);

                    let mut blocks_to_adjust: VecDeque<
                        H256,
                    > = VecDeque::new();
                    blocks_to_adjust.push_back(hash);
                    bg.ledger.adjust_main_chain(blocks_to_adjust);

                    let mut hashes: Vec<H256> = Vec::new();
                    hashes.push(hash);

                    let mut total_difficulties: Vec<U256> = Vec::new();
                    total_difficulties.push(total_difficulty);

                    bg.sync.new_blocks(&hashes[..], &total_difficulties[..]);

                    // start to mine new block
                    current_interval_count = 0;
                    let best_block_info = bg.ledger.best_block();
                    current_mining_hash = Some(best_block_info.header.hash());
                    current_mining_number = best_block_info.header.number();
                    current_total_difficulty = best_block_info.total_difficulty;
                    bg.set_problem();
                    thread::sleep(one_second);
                    current_interval_count += 1;
                    continue;
                } else {
                    panic!("What are you mining?");
                }
            }

            let best_block_info = bg.ledger.best_block();
            let best_hash = best_block_info.header.hash();
            let best_number = best_block_info.header.number();
            let total_difficulty = best_block_info.total_difficulty;

            if let Some(hash) = current_mining_hash {
                if hash == best_hash {
                    // mining on the current best block
                    if current_interval_count < target_interval_count {
                        // still mining
                        thread::sleep(one_second);
                        current_interval_count += 1;
                        continue;
                    } else {
                        panic!("How could this be?");
                    }
                }
            }

            // main chain changed
            // or, mining process just starts
            // start to mine new block
            current_interval_count = 0;
            current_mining_hash = Some(best_hash);
            current_mining_number = best_number;
            current_total_difficulty = total_difficulty;
            bg.set_problem();
            thread::sleep(one_second);
            current_interval_count += 1;
        }
    }
}

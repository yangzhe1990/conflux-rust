extern crate common_types as types;
extern crate core;
extern crate ethereum_types;
extern crate ethkey;
extern crate keccak_hash as hash;
extern crate parking_lot;
extern crate rand;
extern crate rlp;

use core::block::Block;
use core::execution_engine::ExecutionEngineRef;
use core::block_header::{BlockHeader, BlockHeaderBuilder};
use core::transaction::{SignedTransaction, Transaction};
use core::transaction_pool::{TransactionPool, TransactionPoolRef};
use core::LedgerRef;
use core::SyncEngineRef;
use ethereum_types::{Address, H256, U256};
use ethkey::Secret;
use hash::{keccak, KECCAK_NULL_RLP};
use parking_lot::RwLock;
use rand::prelude::*;
use rlp::RlpStream;
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
    txpool: TransactionPoolRef,
    engine: ExecutionEngineRef,
    sync: SyncEngineRef,
    state: RwLock<MiningState>,
}

pub type BlockGeneratorRef = Arc<BlockGenerator>;

impl BlockGenerator {
    pub fn new(
        ledger: LedgerRef, txpool: TransactionPoolRef,
        engine: ExecutionEngineRef, sync: SyncEngineRef,
    ) -> Self
    {
        BlockGenerator {
            ledger,
            txpool,
            engine,
            sync,
            state: RwLock::new(MiningState::Start),
        }
    }

    /// Stop the block generator to generate blocks
    pub fn stop(&mut self) { *self.state.write() = MiningState::Stop; }

    pub fn set_problem(&self) {}

    pub fn generate_block(&self, num_txs: usize) {
        let best_block_info = self.ledger.best_block();
        let best_hash = best_block_info.header.hash();
        let best_number = best_block_info.header.number();
        let mut total_difficulty = best_block_info.total_difficulty;

        let mut txs: Vec<SignedTransaction> = Vec::new();
        let mut tx_rlp = RlpStream::new_list(num_txs);
        for _i in 0..num_txs {
            let tx = Transaction {
                nonce: U256::zero(),
                gas_price: U256::from(1_000_000_000_000_000u64),
                gas: U256::from(200u64),
                value: U256::zero(),
                receiver: Address::default(),
            };
            let secret = Secret::zero();
            let tx = tx.sign(&secret);
            tx_rlp.append(&tx);
            txs.push(tx);
        }
        let mut header = BlockHeaderBuilder::new()
            .with_parent_hash(best_hash)
            .with_timestamp(random::<u64>())
            .with_number(best_number + 1)
            .with_author(Address::default())
            .with_state_root(KECCAK_NULL_RLP)
            .with_difficulty(10.into())
            .with_transactions_root(keccak(tx_rlp.out()))
            .build();
        total_difficulty = total_difficulty + U256::from(10u64);

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

    pub fn start_mining(bg: Arc<BlockGenerator>, _payload_len: u32) {
        let target_interval_count: u32 = 5;
        let mut current_interval_count: u32 = 0;
        let mut parent_hash: H256 = 0.into();
        let mut parent_number: BlockNumber = 0;
        let mut parent_total_difficulty: U256 = 0.into();
        let mut current_total_difficulty: U256 = 0.into();
        let mut current_mining_header_builder = BlockHeaderBuilder::new();
        let mut current_mining_body = Block {
            hash: 0.into(),
            transactions: Vec::new(),
        };
        let one_second = time::Duration::from_millis(1000);

        loop {
            match *bg.state.read() {
                MiningState::Stop => return,
                _ => {}
            }

            if current_interval_count == 0 {
                // start to mine new block
                let best_block_info = bg.ledger.best_block();
                let best_hash = best_block_info.header.hash();
                let best_number = best_block_info.header.number();
                let total_difficulty = best_block_info.total_difficulty;

                parent_hash = best_hash;
                parent_number = best_number;

                let mut txs: Vec<SignedTransaction> = Vec::new();
                for _i in 0..100 {
                    let tx = bg.txpool.fetch_transaction();
                    if let Some(tx) = tx {
                        txs.push(tx);
                    } else {
                        break;
                    }
                }

                let txs = txs;
                let mut tx_rlp = RlpStream::new_list(txs.len());
                for tx in txs.iter() {
                    tx_rlp.append(tx);
                }

                current_mining_header_builder
                    .with_transactions_root(keccak(tx_rlp.out()))
                    .with_parent_hash(parent_hash)
                    .with_timestamp(0)
                    .with_number(parent_number + 1)
                    .with_author(Address::default())
                    .with_state_root(KECCAK_NULL_RLP)
                    .with_difficulty(10.into());
                current_total_difficulty = total_difficulty + U256::from(10u64);

                let mut current_mining_header = current_mining_header_builder.build();
                current_mining_header.compute_hash();
                let hash = current_mining_header.hash();
                current_mining_body.hash = hash;
                current_mining_body.transactions = txs;

                bg.set_problem();
                thread::sleep(one_second);
                current_interval_count += 1;
                continue;
            }

            // check if mined a block
            if current_interval_count == target_interval_count {
                // mined one block
                let current_mining_header = current_mining_header_builder.build();
                let hash = current_mining_header.hash();
                bg.ledger
                    .add_block_header_by_hash(&hash, current_mining_header);
                bg.ledger.add_block_body_by_hash(&hash, current_mining_body);
                bg.ledger.add_child(&parent_hash, &hash);

                let mut blocks_to_adjust: VecDeque<H256> = VecDeque::new();
                blocks_to_adjust.push_back(hash);
                bg.ledger.adjust_main_chain(blocks_to_adjust);
                let block_number = bg.ledger.best_block_number();
                bg.engine.execute_up_to(block_number);

                let mut hashes: Vec<H256> = Vec::new();
                hashes.push(hash);

                let mut total_difficulties: Vec<U256> = Vec::new();
                total_difficulties.push(current_total_difficulty);

                bg.sync.new_blocks(&hashes[..], &total_difficulties[..]);

                // start to mine new block
                current_mining_header_builder = BlockHeaderBuilder::new();
                current_mining_body = Block {
                    hash: 0.into(),
                    transactions: Vec::new(),
                };
                current_interval_count = 0;
                continue;
            }

            let best_block_info = bg.ledger.best_block();
            let best_hash = best_block_info.header.hash();
            let best_number = best_block_info.header.number();
            let total_difficulty = best_block_info.total_difficulty;

            if current_mining_header_builder.build().hash() == best_hash {
                thread::sleep(one_second);
                current_interval_count += 1;
                continue;
            }

            // main chain changed
            current_interval_count = 0;
        }
    }
}

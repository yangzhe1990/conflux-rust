extern crate core;
extern crate ethereum_types;
extern crate ethkey;
extern crate keccak_hash as hash;
extern crate parking_lot;
extern crate primitives;
extern crate rand;
extern crate rlp;

use core::{
    execution_engine::ExecutionEngineRef,
    transaction_pool::SharedTransactionPool, LedgerRef,
    SharedSynchronizationService,
};
use ethereum_types::{Address, H256, U256, U512};
use ethkey::Secret;
use hash::{keccak, KECCAK_NULL_RLP};
use parking_lot::RwLock;
use primitives::*;
use rlp::RlpStream;
use std::{
    collections::VecDeque,
    sync::{mpsc, Arc, Mutex},
    thread, time,
};

enum MiningState {
    Start,
    Stop,
}

#[derive(Debug, Copy, Clone, PartialOrd, PartialEq)]
pub struct ProofOfWorkProblem {
    block_hash: H256,
    difficulty: U256,
    boundary: H256,
}

#[derive(Debug, Copy, Clone)]
pub struct ProofOfWorkAnswer {
    nonce: u64,
}

/// The interface for a conflux block generator
pub struct BlockGenerator {
    ledger: LedgerRef,
    txpool: SharedTransactionPool,
    engine: ExecutionEngineRef,
    sync: SharedSynchronizationService,
    state: RwLock<MiningState>,
    workers: Mutex<Vec<(Worker, mpsc::Sender<ProofOfWorkProblem>)>>,
}

pub struct Worker {
    thread: thread::JoinHandle<()>,
}

fn difficulty_to_boundary(difficulty: &U256) -> H256 {
    difficulty_to_boundary_aux(difficulty).into()
}

fn difficulty_to_boundary_aux<T: Into<U512>>(difficulty: T) -> U256 {
    let difficulty = difficulty.into();
    assert!(!difficulty.is_zero());
    if difficulty == U512::one() {
        U256::max_value()
    } else {
        // difficulty > 1, so result should never overflow 256 bits
        U256::from((U512::one() << 256) / difficulty)
    }
}

pub fn compute(nonce: &u64, block_hash: &H256) -> H256 {
    let mut rlp = RlpStream::new_list(2);
    rlp.append(block_hash).append(nonce);
    keccak(rlp.out())
}

pub fn validate(
    problem: &ProofOfWorkProblem, answer: &ProofOfWorkAnswer,
) -> bool {
    let nonce = answer.nonce;
    let hash = compute(&nonce, &problem.block_hash);
    hash < problem.boundary
}

pub type BlockGeneratorRef = Arc<BlockGenerator>;

impl Worker {
    pub fn new(
        bg: Arc<BlockGenerator>, sender: mpsc::Sender<ProofOfWorkAnswer>,
        receiver: mpsc::Receiver<ProofOfWorkProblem>,
    ) -> Self
    {
        let bg_handle = bg.clone();

        let thread = thread::spawn(move || {
            let sleep_duration = time::Duration::from_millis(100);
            let mut problem: Option<ProofOfWorkProblem> = None;

            loop {
                match *bg_handle.state.read() {
                    MiningState::Stop => return,
                    _ => {}
                }

                // check if there is a new problem
                let new_problem = receiver.try_recv();
                if new_problem.is_ok() {
                    problem = Some(new_problem.unwrap());
                }
                // check if there is a problem to be solved
                if problem.is_some() {
                    let boundary = problem.unwrap().boundary;
                    let block_hash = problem.unwrap().block_hash;

                    for _i in 0..10000000 {
                        //TODO: adjust the number of times
                        let nonce = rand::random();
                        let hash = compute(&nonce, &block_hash);
                        if hash < boundary {
                            // problem solved
                            sender
                                .send(ProofOfWorkAnswer { nonce })
                                .expect("Failed to send the PoW answer.");
                            problem = None;
                            break;
                        }
                    }
                } else {
                    thread::sleep(sleep_duration);
                }
            }
        });
        Worker { thread }
    }
}

impl BlockGenerator {
    pub fn new(
        ledger: LedgerRef, txpool: SharedTransactionPool,
        engine: ExecutionEngineRef, sync: SharedSynchronizationService,
    ) -> Self
    {
        BlockGenerator {
            ledger,
            txpool,
            engine,
            sync,
            state: RwLock::new(MiningState::Start),
            workers: Mutex::new(Vec::new()),
        }
    }

    /// Stop mining
    pub fn stop(bg: Arc<BlockGenerator>) {
        let mut write = bg.state.write();
        *write = MiningState::Stop;
    }

    pub fn set_problem(bg: Arc<BlockGenerator>, problem: ProofOfWorkProblem) {
        for item in bg.workers.lock().unwrap().iter() {
            item.1
                .send(problem)
                .expect("Failed to send the PoW problem.")
        }
    }

    pub fn generate_block(&self, num_txs: usize) {
        // get the best block
        let best_block_hash = self.ledger.best_epoch_hash();
        let best_block_info =
            self.ledger.block_by_hash(&best_block_hash).unwrap();

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
            let secret :Secret = "46b9e861b63d3509c88b7817275a30d22d62c8cd8fa6486ddee35ef0d8e0495f".parse().unwrap();
            let tx = tx.sign(&secret);
            tx_rlp.append(&tx);
            txs.push(tx);
        }
        let mut header = BlockHeaderBuilder::new()
            .with_transactions_root(keccak(tx_rlp.out()))
            .with_parent_hash(best_block_info.hash())
            .with_timestamp(0) //TODO: get timestamp
            .with_author(Address::default()) //TODO: get author
            .with_deferred_state_root(KECCAK_NULL_RLP) //TODO: get deferred state root
            .with_difficulty(10.into())
            .with_referee_hashes(Vec::new()) //TODO: get referee hashes
            .build();
        header.compute_hash();
        let hash = header.hash();

        let block = Block {
            block_header: header.clone(),
            transactions: txs,
        };

        // adjust the ledger
        self.ledger.add_block_header_by_hash(&hash, header.clone());
        self.ledger.add_block_by_hash(&hash, block);
        self.ledger.add_child(&best_block_info.hash(), &hash);
        let mut blocks_to_adjust: VecDeque<H256> = VecDeque::new();
        blocks_to_adjust.push_back(hash);
        self.ledger.adjust_main_chain(blocks_to_adjust);

        // execute
        let best_epoch_number = self.ledger.best_epoch_number();
        self.engine.execute_up_to(best_epoch_number);

        // sync
        let mut hashes: Vec<H256> = Vec::new();
        hashes.push(hash);
        self.sync.announce_new_blocks(&hashes[..]);
    }

    pub fn start_new_worker(
        num_worker: u32, bg: Arc<BlockGenerator>,
    ) -> mpsc::Receiver<ProofOfWorkAnswer> {
        let (tx, rx) = mpsc::channel();
        let mut workers = bg.workers.lock().unwrap();
        for _ in 0..num_worker {
            let (sender_handle, receiver_handle) = mpsc::channel();
            workers.push((
                Worker::new(bg.clone(), tx.clone(), receiver_handle),
                sender_handle,
            ));
        }
        rx
    }

    pub fn start_mining(bg: Arc<BlockGenerator>, _payload_len: u32) {
        let mut current_mining_header_builder = BlockHeaderBuilder::new();
        let mut current_mining_block = Block {
            block_header: BlockHeader::new(),
            transactions: Vec::new(),
        };
        let mut current_problem: Option<ProofOfWorkProblem> = None;

        let sleep_duration = time::Duration::from_millis(100);

        let receiver: mpsc::Receiver<ProofOfWorkAnswer> =
            BlockGenerator::start_new_worker(1, bg.clone());

        loop {
            match *bg.state.read() {
                MiningState::Stop => return,
                _ => {}
            }

            // get the best block
            let best_block_hash = bg.ledger.best_epoch_hash();
            let best_block_info =
                bg.ledger.block_by_hash(&best_block_hash).unwrap();

            // check if the main chain changed
            if current_problem.is_some()
                && *current_mining_block.block_header.parent_hash()
                    == best_block_info.hash()
            {
                // check if mined one new block
                let mut new_answer = receiver.try_recv();
                loop {
                    // check if the block received valid
                    if new_answer.is_ok()
                        && !validate(
                            &current_problem.unwrap(),
                            &new_answer.unwrap(),
                        ) {
                        new_answer = receiver.try_recv();
                    } else {
                        break;
                    }
                }

                if new_answer.is_ok() {
                    let answer = new_answer.unwrap();
                    let hash = current_mining_block.hash();

                    // adjust the ledger
                    let current_mining_header =
                        current_mining_header_builder.build();
                    bg.ledger
                        .add_block_header_by_hash(&hash, current_mining_header);
                    bg.ledger.add_block_by_hash(&hash, current_mining_block);
                    bg.ledger.add_child(&best_block_info.hash(), &hash);
                    let mut blocks_to_adjust: VecDeque<
                        H256,
                    > = VecDeque::new();
                    blocks_to_adjust.push_back(hash);
                    bg.ledger.adjust_main_chain(blocks_to_adjust);

                    // execute
                    let best_epoch_number = bg.ledger.best_epoch_number();
                    bg.engine.execute_up_to(best_epoch_number);

                    // sync
                    let mut hashes: Vec<H256> = Vec::new();
                    hashes.push(hash);
                    bg.sync.announce_new_blocks(&hashes[..]);

                    //initialize for the next block
                    current_mining_header_builder = BlockHeaderBuilder::new();
                    current_mining_block = Block {
                        block_header: BlockHeader::new(),
                        transactions: Vec::new(),
                    };
                    continue;
                }
                // wait for 100 millis and check again
                thread::sleep(sleep_duration);
                continue;
            }

            // build a new block
            let txs = bg.txpool.pack_transactions(10);
            let mut tx_rlp = RlpStream::new_list(txs.len());
            for tx in txs.iter() {
                tx_rlp.append(tx);
            }
            // TODO: adjust difficulty
            let new_difficulty = *best_block_info.block_header.difficulty();
            current_mining_header_builder
                .with_transactions_root(keccak(tx_rlp.out()))
                .with_parent_hash(best_block_info.hash())
                .with_timestamp(0) //TODO: get timestamp
                .with_author(Address::default()) //TODO: get author
                .with_deferred_state_root(KECCAK_NULL_RLP) //TODO: get deferred state root
                .with_difficulty(new_difficulty)
                .with_referee_hashes(Vec::new()); //TODO: get referee hashes

            let mut current_mining_header =
                current_mining_header_builder.build();
            current_mining_header.compute_hash();
            current_mining_block = Block {
                block_header: current_mining_header,
                transactions: txs,
            };

            // set a mining problem
            let problem = ProofOfWorkProblem {
                block_hash: current_mining_block.hash(),
                difficulty: new_difficulty,
                boundary: difficulty_to_boundary(&new_difficulty),
            };
            BlockGenerator::set_problem(bg.clone(), problem);
            current_problem = Some(problem);
        }
    }
}

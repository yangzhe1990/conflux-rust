extern crate core;
extern crate ethereum_types;
extern crate ethkey;
extern crate keccak_hash as hash;
extern crate parking_lot;
extern crate primitives;
extern crate rand;
extern crate rlp;
extern crate txgen;

#[macro_use]
extern crate log;

use core::{
    SharedConsensusGraph, SharedSynchronizationService, SharedTransactionPool,
};
use ethereum_types::{Address, H256, U256, U512};
use ethkey::Secret;
use hash::{keccak, KECCAK_NULL_RLP};
use parking_lot::RwLock;
use primitives::*;
use rlp::RlpStream;
use std::{
    sync::{mpsc, Arc, Mutex},
    thread, time,
};
use txgen::SharedTransactionGenerator;

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
pub struct ProofOfWorkSolution {
    nonce: u64,
}

/// The interface for a conflux block generator
pub struct BlockGenerator {
    consensus: SharedConsensusGraph,
    txpool: SharedTransactionPool,
    txgen: SharedTransactionGenerator,
    sync: SharedSynchronizationService,
    state: RwLock<MiningState>,
    workers: Mutex<Vec<(Worker, mpsc::Sender<ProofOfWorkProblem>)>>,
}

pub struct Worker {
    #[allow(dead_code)]
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
    problem: &ProofOfWorkProblem, solution: &ProofOfWorkSolution,
) -> bool {
    let nonce = solution.nonce;
    let hash = compute(&nonce, &problem.block_hash);
    hash < problem.boundary
}

impl Worker {
    pub fn new(
        bg: Arc<BlockGenerator>, sender: mpsc::Sender<ProofOfWorkSolution>,
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
                                .send(ProofOfWorkSolution { nonce })
                                .expect("Failed to send the PoW solution.");
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
        consensus: SharedConsensusGraph, txpool: SharedTransactionPool,
        sync: SharedSynchronizationService, txgen: SharedTransactionGenerator,
    ) -> Self
    {
        BlockGenerator {
            consensus,
            txpool,
            txgen,
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

    /// Send new PoW problem to workers
    pub fn send_problem(bg: Arc<BlockGenerator>, problem: ProofOfWorkProblem) {
        for item in bg.workers.lock().unwrap().iter() {
            item.1
                .send(problem)
                .expect("Failed to send the PoW problem.")
        }
    }

    /// Assemble a new block without nonce
    pub fn assemble_new_block(&self, num_txs: usize) -> Block {
        // get the best block
        let best_block_hash = self.consensus.best_block_hash();
        let transactions = self.txpool.pack_transactions(num_txs);
        let mut tx_rlp = RlpStream::new_list(transactions.len());
        for tx in transactions.iter() {
            tx_rlp.append(tx);
        }
        let block_header = BlockHeaderBuilder::new()
            .with_transactions_root(keccak(tx_rlp.out()))
            .with_parent_hash(best_block_hash)
            .with_timestamp(0) //TODO: get timestamp
            .with_author(Address::default()) //TODO: get author
            .with_deferred_state_root(KECCAK_NULL_RLP) //TODO: get deferred state root
            .with_difficulty(10.into()) //TODO: adjust difficulty
            .with_referee_hashes(Vec::new()) //TODO: get referee hashes
            .with_nonce(0)
            .build();

        Block {
            block_header,
            transactions,
        }
    }

    /// Update and sync a new block
    pub fn on_mined_block(&self, block: Block) {
        self.sync.on_mined_block(block);
    }

    /// Check if we need to mine on a new block
    pub fn is_mining_block_outdated(&self, block: &Block) -> bool {
        // 1st Check: if the parent block changed
        let best_block_hash = self.consensus.best_block_hash();
        if best_block_hash != *block.block_header.parent_hash() {
            return true;
        }
        // TODO: 2nd check: if the referee hashes changed
        // TODO: 3rd check: if we want to pack a new set of transactions
        false
    }

    /// Generate a block with fake transactions
    pub fn generate_block(&self, num_txs: usize) -> H256 {
        for _ in 0..num_txs {
            let tx = self.txgen.generate_transaction();
            //            let tx = Transaction {
            //                nonce: U256::zero(),
            //                gas_price: U256::from(1_000_000_000_000_000u64),
            //                gas: U256::from(200u64),
            //                value: U256::zero(),
            //                receiver: Address::default(),
            //            };
            //            let secret :Secret =
            // "46b9e861b63d3509c88b7817275a30d22d62c8cd8fa6486ddee35ef0d8e0495f".
            // parse().unwrap();            let tx =
            // tx.sign(&secret);
            self.txpool.add(tx);
        }
        let block = self.assemble_new_block(num_txs);
        let hash = block.hash();
        debug!(target:"sync", "generate_block with block header:{:?}, hash:{:?}", block.block_header, block.hash());
        self.on_mined_block(block);
        hash
    }

    /// Start num_worker new workers
    pub fn start_new_worker(
        num_worker: u32, bg: Arc<BlockGenerator>,
    ) -> mpsc::Receiver<ProofOfWorkSolution> {
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
        let mut current_mining_block = Block::default();
        let mut current_problem: Option<ProofOfWorkProblem> = None;
        let sleep_duration = time::Duration::from_millis(1000);

        let receiver: mpsc::Receiver<ProofOfWorkSolution> =
            BlockGenerator::start_new_worker(1, bg.clone());

        loop {
            match *bg.state.read() {
                MiningState::Stop => return,
                _ => {}
            }

            if bg.is_mining_block_outdated(&current_mining_block) {
                // TODO: #transations TBD
                current_mining_block = bg.assemble_new_block(10);

                // set a mining problem
                let current_difficulty =
                    current_mining_block.block_header.difficulty();
                let problem = ProofOfWorkProblem {
                    block_hash: current_mining_block
                        .block_header
                        .problem_hash(),
                    difficulty: *current_difficulty,
                    boundary: difficulty_to_boundary(current_difficulty),
                };
                BlockGenerator::send_problem(bg.clone(), problem);
                current_problem = Some(problem);
            } else {
                // check if the problem solved
                let mut new_solution = receiver.try_recv();
                loop {
                    // check if the block received valid
                    if new_solution.is_ok()
                        && !validate(
                            &current_problem.unwrap(),
                            &new_solution.unwrap(),
                        ) {
                        new_solution = receiver.try_recv();
                    } else {
                        break;
                    }
                }
                if new_solution.is_ok() {
                    let solution = new_solution.unwrap();
                    current_mining_block.block_header.set_nonce(solution.nonce);
                    bg.on_mined_block(current_mining_block);
                    current_mining_block = Block::default();
                    current_problem = None;
                } else {
                    // wait a moment and check again
                    thread::sleep(sleep_duration);
                    continue;
                }
            }
        }
    }
}

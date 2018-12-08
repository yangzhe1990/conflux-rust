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

#[cfg(test)]
mod tests;

use core::{
    pow::*, SharedConsensusGraph, SharedSynchronizationService,
    SharedTransactionPool,
};
use ethereum_types::{Address, H256};
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

impl Worker {
    pub fn new(
        bg: Arc<BlockGenerator>, sender: mpsc::Sender<ProofOfWorkSolution>,
        receiver: mpsc::Receiver<ProofOfWorkProblem>,
    ) -> Self
    {
        let bg_handle = bg.clone();

        let thread = thread::Builder::new()
            .name("blockgen".into())
            .spawn(move || {
                #[cfg(test)]
                {
                    println!("A new worker start mining!");
                }
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

                        #[cfg(test)]
                        {
                            let difficulty = problem.unwrap().difficulty;
                            println!(
                                "Start a new round with difficulty {}!",
                                difficulty
                            );
                            if difficulty > 500000.into() {
                                println!("Difficulty is too high to mine!");
                            }
                        }

                        for _i in 0..100000 {
                            //TODO: adjust the number of times
                            let nonce = rand::random();
                            let hash = compute(nonce, &block_hash);
                            if hash < boundary {
                                // problem solved
                                sender
                                    .send(ProofOfWorkSolution { nonce })
                                    .expect("Failed to send the PoW solution.");
                                problem = None;
                                break;
                            }
                        }
                        #[cfg(test)]
                        {
                            if problem.is_some() {
                                println!("Sad, we failed in this round.");
                            }
                        }
                    } else {
                        thread::sleep(sleep_duration);
                    }
                }
            })
            .expect("only one blockgen thread, so it should not fail");
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
        let parent_height =
            self.consensus.get_block_height(&best_block_hash).unwrap();
        let transactions = self.txpool.pack_transactions(num_txs);
        let mut tx_rlp = RlpStream::new_list(transactions.len());
        for tx in transactions.iter() {
            tx_rlp.append(tx);
        }
        let mut referee = self.consensus.terminal_block_hashes();
        referee.retain(|r| *r != best_block_hash);
        let block_header = BlockHeaderBuilder::new()
            .with_transactions_root(keccak(tx_rlp.out()))
            .with_parent_hash(best_block_hash)
            .with_height(parent_height + 1)
            .with_timestamp(0) //TODO: get timestamp
            .with_author(Address::default()) //TODO: get author
            .with_deferred_state_root(KECCAK_NULL_RLP) //TODO: get deferred state root
            .with_difficulty(4.into()) //TODO: adjust difficulty
            .with_referee_hashes(referee) //TODO: get referee hashes
            .with_nonce(0) // TODO: gen nonce from pow
            .build();

        Block {
            block_header,
            transactions,
        }
    }

    /// Update and sync a new block
    pub fn on_mined_block(&self, block: Block) {
        #[cfg(test)]
        {
            println!("Mined a block!");
            return;
        }
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
    pub fn generate_block_with_transactions(&self, num_txs: usize) -> H256 {
        for _ in 0..num_txs {
            let tx = self.txgen.generate_transaction();
            self.txpool.add_pending(tx);
        }
        self.generate_block(num_txs)
    }

    /// Generate a block with transactions in the pool
    pub fn generate_block(&self, num_txs:usize) -> H256{
        let mut block = self.assemble_new_block(num_txs);
        // TODO Get difficulty from PoWConfig
        let test_diff = 4.into();
        let problem = ProofOfWorkProblem{
            block_hash: block.block_header.problem_hash(),
            difficulty: test_diff,
            boundary: difficulty_to_boundary(&test_diff),
        };
        loop{
            let nonce = rand::random();
            if validate(&problem, &ProofOfWorkSolution{nonce}) {
                block.block_header.set_nonce(nonce);
                break;
            }
        }
        let hash = block.hash();
        debug!(
            "generate_block with block header:{:?} tx_number:{}",
            block.block_header, block.transactions.len()
        );
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

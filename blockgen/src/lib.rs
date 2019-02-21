use cfxcore::{
    consensus::DEFERRED_STATE_EPOCH_COUNT, pow::*,
    transaction_pool::DEFAULT_MAX_BLOCK_GAS_LIMIT, SharedSynchronizationGraph,
    SharedSynchronizationService, SharedTransactionPool,
};
use ethereum_types::{Address, H256};
use log::{debug, trace, warn};
use parking_lot::RwLock;
use primitives::*;
use std::{
    sync::{mpsc, Arc, Mutex},
    thread, time,
};
use txgen::SharedTransactionGenerator;

pub struct BlockGeneratorConfig {
    pub test_chain_path: Option<String>,
}

enum MiningState {
    Start,
    Stop,
}

/// The interface for a conflux block generator
pub struct BlockGenerator {
    pow_config: ProofOfWorkConfig,
    graph: SharedSynchronizationGraph,
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
                            if difficulty > 500000.into() {
                                warn!("Difficulty is too high to mine!");
                            }
                        }

                        for _i in 0..100000 {
                            //TODO: adjust the number of times
                            let nonce = rand::random();
                            let hash = compute(nonce, &block_hash);
                            if hash < boundary {
                                // problem solved
                                match sender.send(ProofOfWorkSolution { nonce })
                                {
                                    Ok(_) => {}
                                    Err(e) => {
                                        warn!("{}", e);
                                    }
                                }
                                problem = None;
                                break;
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
        graph: SharedSynchronizationGraph, txpool: SharedTransactionPool,
        sync: SharedSynchronizationService, txgen: SharedTransactionGenerator,
        pow_config: ProofOfWorkConfig,
    ) -> Self
    {
        BlockGenerator {
            pow_config,
            graph,
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

    fn assemble_new_block_impl(
        &self, parent_hash: H256, referee: Vec<H256>,
        deferred_state_root: H256, deferred_receipts_root: H256,
        num_txs: usize,
    ) -> Block
    {
        let parent_height =
            self.graph.block_height_by_hash(&parent_hash).unwrap();
        let transactions = self
            .txpool
            .pack_transactions(num_txs, self.txgen.get_best_state());
        trace!("{} txs packed", transactions.len());

        let block_header = BlockHeaderBuilder::new()
            .with_transactions_root(Block::compute_transaction_root(
                &transactions,
            ))
            .with_parent_hash(parent_hash)
            .with_height(parent_height + 1)
            .with_timestamp(0) //TODO: get timestamp
            .with_author(Address::default()) //TODO: get author
            .with_deferred_state_root(deferred_state_root) //TODO: get deferred state root
            .with_deferred_receipts_root(deferred_receipts_root)
            .with_difficulty(self.pow_config.initial_difficulty.into()) //TODO: adjust difficulty
            .with_referee_hashes(referee) //TODO: get referee hashes
            .with_nonce(0) // TODO: gen nonce from pow
            .with_gas_limit(DEFAULT_MAX_BLOCK_GAS_LIMIT.into())
            .build();

        Block {
            block_header,
            transactions,
        }
    }

    /// Assemble a new block with specified parent and referee, this is for test
    /// only
    pub fn assemble_new_fixed_block(
        &self, parent_hash: H256, referee: Vec<H256>, num_txs: usize,
    ) -> Block {
        let (state_root, receipts_root) =
            self.graph.consensus.compute_deferred_state_for_block(
                &parent_hash,
                DEFERRED_STATE_EPOCH_COUNT as usize - 1,
            );

        self.assemble_new_block_impl(
            parent_hash,
            referee,
            state_root,
            receipts_root,
            num_txs,
        )
    }

    /// Assemble a new block without nonce
    pub fn assemble_new_block(&self, num_txs: usize) -> Block {
        // get the best block
        let best_info = self.graph.get_best_info();
        let best_block_hash = best_info.best_block_hash;
        let mut referee = best_info.terminal_block_hashes;
        referee.retain(|r| *r != best_block_hash);

        debug_assert_eq!(
            best_block_hash,
            self.txgen.consensus.best_block_hash()
        );
        self.assemble_new_block_impl(
            best_block_hash,
            referee,
            best_info.deferred_state_root,
            best_info.deferred_receipts_root,
            num_txs,
        )
    }

    /// Update and sync a new block
    pub fn on_mined_block(&self, block: Block) {
        self.sync.on_mined_block(block);
    }

    /// Check if we need to mine on a new block
    pub fn is_mining_block_outdated(&self, block: &Block) -> bool {
        // 1st Check: if the parent block changed
        let best_block_hash = self.graph.get_best_info().best_block_hash;
        if best_block_hash != *block.block_header.parent_hash() {
            return true;
        }
        // TODO: 2nd check: if the referee hashes changed
        // TODO: 3rd check: if we want to pack a new set of transactions
        false
    }

    /// Generate a block with fake transactions
    pub fn generate_block_with_transactions(&self, num_txs: usize) -> H256 {
        let mut txs = Vec::new();
        for _ in 0..num_txs {
            let tx = self.txgen.generate_transaction();
            txs.push(tx);
        }
        self.txpool.insert_new_transactions(
            self.graph.consensus.best_state_block_hash(),
            txs.into_iter().map(|tx| tx.transaction).collect(),
        );
        self.generate_block(num_txs)
    }

    pub fn generate_fixed_block(
        &self, parent_hash: H256, referee: Vec<H256>, num_txs: usize,
    ) -> H256 {
        let block =
            self.assemble_new_fixed_block(parent_hash, referee, num_txs);
        self.generate_block_impl(block)
    }

    /// Generate a block with transactions in the pool
    pub fn generate_block(&self, num_txs: usize) -> H256 {
        let block = self.assemble_new_block(num_txs);
        self.generate_block_impl(block)
    }

    fn generate_block_impl(&self, block_init: Block) -> H256 {
        let mut block = block_init;
        let test_diff = self.pow_config.initial_difficulty.into();
        let problem = ProofOfWorkProblem {
            block_hash: block.block_header.problem_hash(),
            difficulty: test_diff,
            boundary: difficulty_to_boundary(&test_diff),
        };
        loop {
            let nonce = rand::random();
            if validate(&problem, &ProofOfWorkSolution { nonce }) {
                block.block_header.set_nonce(nonce);
                break;
            }
        }
        let hash = block.block_header.compute_hash();
        debug!(
            "generate_block with block header:{:?} tx_number:{}",
            block.block_header,
            block.transactions.len()
        );
        self.on_mined_block(block);
        hash
    }

    pub fn pow_config(&self) -> ProofOfWorkConfig {
        return self.pow_config.clone();
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
                        )
                    {
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

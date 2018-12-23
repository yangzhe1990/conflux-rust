use ethereum_types::{H256, U256};
use executive::{ExecutionError, Executive};
use ext_db::SystemDB;
use machine::new_byzantium_test_machine;
use parking_lot::RwLock;
use primitives::Block;
use slab::Slab;
use state::State;
use statedb::StateDb;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    iter::FromIterator,
    sync::Arc,
};
use storage::{StorageManager, StorageManagerTrait};
use transaction_pool::SharedTransactionPool;
use vm::{EnvInfo, Spec};
use vm_factory::VmFactory;

const NULL: usize = !0;

pub struct ConsensusGraphNodeData {
    pub epoch_number: RefCell<usize>,
}

unsafe impl Sync for ConsensusGraphNodeData {}

impl ConsensusGraphNodeData {
    pub fn new(epoch_number: usize) -> Self {
        ConsensusGraphNodeData {
            epoch_number: RefCell::new(epoch_number),
        }
    }
}

pub struct ConsensusGraphNode {
    pub hash: H256,
    pub total_difficulty: U256,
    pub parent: usize,
    pub children: Vec<usize>,
    pub referees: Vec<usize>,
    pub data: ConsensusGraphNodeData,
}

pub struct ConsensusGraphInner {
    pub arena: Slab<ConsensusGraphNode>,
    pub indices: HashMap<H256, usize>,
    pub pivot_chain: Vec<usize>,
    /// Track the block where the tx is successfully executed
    pub block_for_transaction: HashMap<H256, (bool, usize)>,
    genesis_block_index: usize,
    storage_manager: Arc<StorageManager>,
    vm: VmFactory,
}

impl ConsensusGraphInner {
    pub fn with_genesis_block(
        genesis_block: &Block, storage_manager: Arc<StorageManager>,
        vm: VmFactory,
    ) -> Self
    {
        let mut inner = ConsensusGraphInner {
            arena: Slab::new(),
            indices: HashMap::new(),
            pivot_chain: Vec::new(),
            block_for_transaction: HashMap::new(),
            genesis_block_index: NULL,
            storage_manager,
            vm,
        };
        inner.genesis_block_index =
            inner.insert(genesis_block, &mut HashSet::new());
        *inner.arena[inner.genesis_block_index]
            .data
            .epoch_number
            .borrow_mut() = 0;
        inner.pivot_chain.push(inner.genesis_block_index);

        inner
    }

    pub fn insert(
        &mut self, block: &Block, terminal_hashes: &mut HashSet<H256>,
    ) -> usize {
        let hash = block.hash();

        let parent = if *block.block_header.parent_hash() != H256::default() {
            self.indices
                .get(block.block_header.parent_hash())
                .cloned()
                .unwrap()
        } else {
            NULL
        };
        let referees: Vec<usize> = block
            .block_header
            .referee_hashes()
            .iter()
            .map(|hash| self.indices.get(hash).cloned().unwrap())
            .collect();
        for referee in &referees {
            terminal_hashes.remove(&self.arena[*referee].hash);
        }
        let index = self.arena.insert(ConsensusGraphNode {
            hash,
            total_difficulty: block.block_header.difficulty().clone(),
            parent,
            children: Vec::new(),
            referees,
            data: ConsensusGraphNodeData::new(NULL),
        });
        self.indices.insert(hash, index);

        if parent != NULL {
            terminal_hashes.remove(&self.arena[parent].hash);
            terminal_hashes.insert(hash);
            self.arena[parent].children.push(index);
        }

        index
    }

    pub fn on_new_block(
        &mut self, txpool: &SharedTransactionPool, block: &Block,
        block_by_hash: &HashMap<H256, Block>,
        terminal_hashes: &mut HashSet<H256>,
    ) -> H256
    {
        let mut me = self.insert(block, terminal_hashes);
        loop {
            me = self.arena[me].parent;
            self.arena[me].total_difficulty += *block.block_header.difficulty();
            if me == self.genesis_block_index {
                break;
            }
        }

        let mut new_pivot_chain = Vec::new();
        me = self.genesis_block_index;
        loop {
            new_pivot_chain.push(me);
            if self.arena[me].children.len() == 0 {
                break;
            }
            let heaviest = self.arena[me]
                .children
                .iter()
                .max_by_key(|index| {
                    (
                        self.arena[**index].total_difficulty,
                        self.arena[**index].hash,
                    )
                })
                .cloned()
                .unwrap();
            me = heaviest;
        }

        let mut fork_at = 0;
        while fork_at < self.pivot_chain.len()
            && fork_at < new_pivot_chain.len()
        {
            if self.pivot_chain[fork_at] != new_pivot_chain[fork_at] {
                break;
            }
            fork_at += 1;
        }

        if fork_at < self.pivot_chain.len() {
            let enqueue_if_obsolete = |queue: &mut VecDeque<usize>, index| {
                let mut epoch_number =
                    self.arena[index].data.epoch_number.borrow_mut();
                if *epoch_number != NULL && *epoch_number >= fork_at {
                    *epoch_number = NULL;
                    queue.push_back(index);
                }
            };

            let mut queue = VecDeque::new();
            enqueue_if_obsolete(&mut queue, *self.pivot_chain.last().unwrap());
            while let Some(me) = queue.pop_front() {
                for referee in self.arena[me].referees.clone() {
                    enqueue_if_obsolete(&mut queue, referee);
                }
                enqueue_if_obsolete(&mut queue, self.arena[me].parent);
            }
        }

        while fork_at < new_pivot_chain.len() {
            // First, identify all the blocks in the current epoch
            let mut queue = Vec::new();
            {
                let copy_of_fork_at = fork_at;
                let enqueue_if_new = |queue: &mut Vec<usize>, index| {
                    let mut epoch_number =
                        self.arena[index].data.epoch_number.borrow_mut();
                    if *epoch_number == NULL {
                        *epoch_number = copy_of_fork_at;
                        queue.push(index);
                    }
                };

                let mut at = 0;
                enqueue_if_new(&mut queue, new_pivot_chain[fork_at]);
                while at < queue.len() {
                    let me = queue[at];
                    for referee in &self.arena[me].referees {
                        enqueue_if_new(&mut queue, *referee);
                    }
                    enqueue_if_new(&mut queue, self.arena[me].parent);
                    at += 1;
                }
            }

            // Second, sort all the blocks based on their topological order
            // and break ties with block hash
            let index_set: HashSet<usize> =
                HashSet::from_iter(queue.iter().cloned());
            let mut num_incoming_edges = HashMap::new();

            for me in &queue {
                num_incoming_edges.entry(*me).or_insert(0);
                let parent = self.arena[*me].parent;
                if index_set.contains(&parent) {
                    *num_incoming_edges.entry(parent).or_insert(0) += 1;
                }
                for referee in &self.arena[*me].referees {
                    if index_set.contains(referee) {
                        *num_incoming_edges.entry(*referee).or_insert(0) += 1;
                    }
                }
            }

            let mut candidates = HashSet::new();
            let mut reversed_indices = Vec::new();

            for me in &queue {
                if num_incoming_edges[me] == 0 {
                    candidates.insert(*me);
                }
            }
            while !candidates.is_empty() {
                let me = candidates
                    .iter()
                    .max_by_key(|index| self.arena[**index].hash)
                    .cloned()
                    .unwrap();
                candidates.remove(&me);
                reversed_indices.push(me);

                let parent = self.arena[me].parent;
                if index_set.contains(&parent) {
                    num_incoming_edges.entry(parent).and_modify(|e| *e -= 1);
                    if num_incoming_edges[&parent] == 0 {
                        candidates.insert(parent);
                    }
                }
                for referee in &self.arena[me].referees {
                    if index_set.contains(referee) {
                        num_incoming_edges
                            .entry(*referee)
                            .and_modify(|e| *e -= 1);
                        if num_incoming_edges[referee] == 0 {
                            candidates.insert(*referee);
                        }
                    }
                }
            }

            // Third, apply transactions in the determined total order
            let mut state = State::new(
                StateDb::new(
                    self.storage_manager
                        .get_state_at(
                            self.arena[new_pivot_chain[fork_at - 1]].hash,
                        )
                        .unwrap(),
                ),
                0.into(),
                self.vm.clone(),
            );
            let spec = Spec::new_byzantium();
            let machine = new_byzantium_test_machine();
            for index in reversed_indices.iter().rev() {
                let block =
                    block_by_hash.get(&self.arena[*index].hash).unwrap();
                let env = EnvInfo {
                    number: 0, // TODO: replace 0 with correct cardinal number
                    author: block.block_header.author().clone(),
                    timestamp: block.block_header.timestamp(),
                    difficulty: block.block_header.difficulty().clone(),
                    gas_used: U256::zero(),
                    gas_limit: U256::from(block.block_header.gas_limit()),
                };
                for transaction in &block.transactions {
                    let mut ex =
                        Executive::new(&mut state, &env, &machine, &spec);
                    let r = ex.transact(transaction);
                    match r {
                        Err(ExecutionError::NotEnoughBaseGas {
                            required: _,
                            got: _,
                        })
                        | Err(ExecutionError::SenderMustExist {})
                        | Err(ExecutionError::InvalidNonce {
                            expected: _,
                            got: _,
                        })
                        | Err(ExecutionError::NotEnoughBaseGas {
                            required: _,
                            got: _,
                        }) => {
                            warn!("transaction execution error without inc_nonce: transaction={:?}, err={:?}", transaction, r);
                        }
                        _ => {
                            trace!("transaction executed: transaction={:?}, result={:?}", transaction, r);
                            self.block_for_transaction
                                .insert(transaction.hash(), (true, *index));
                        }
                    }
                }
            }
            state.commit_and_notify(
                self.arena[new_pivot_chain[fork_at]].hash,
                txpool,
            );

            fork_at += 1;
        }

        self.pivot_chain = new_pivot_chain;
        self.best_block_hash()
    }

    pub fn best_block_hash(&self) -> H256 {
        self.arena[*self.pivot_chain.last().unwrap()].hash
    }

    pub fn get_block_for_tx_execution(
        &self, tx_hash: &H256,
    ) -> Option<(bool, H256)> {
        self.block_for_transaction
            .get(tx_hash)
            .map(|(success, index)| (*success, self.arena[*index].hash))
    }
}

pub struct ConsensusGraph {
    pub blocks: Arc<RwLock<HashMap<H256, Block>>>,
    pub inner: RwLock<ConsensusGraphInner>,
    genesis_block_hash: H256,
    pub txpool: SharedTransactionPool,
    // This db is used to persist information related to
    // ledger structure, like block- or transaction-related
    // stuffs.
    pub ledger_db: Arc<SystemDB>,
    pub invalid_blocks: RwLock<HashSet<H256>>,
}

pub type SharedConsensusGraph = Arc<ConsensusGraph>;

impl ConsensusGraph {
    pub fn with_genesis_block(
        genesis_block: Block, state_mananger: Arc<StorageManager>,
        vm: VmFactory, txpool: SharedTransactionPool, ledger_db: Arc<SystemDB>,
    ) -> Self
    {
        let genesis_block_hash = genesis_block.hash();

        let mut blocks = HashMap::new();
        blocks.insert(genesis_block_hash, genesis_block.clone());

        ConsensusGraph {
            inner: RwLock::new(ConsensusGraphInner::with_genesis_block(
                &genesis_block,
                state_mananger,
                vm,
            )),
            blocks: Arc::new(RwLock::new(blocks)),
            genesis_block_hash,
            txpool,
            ledger_db,
            invalid_blocks: RwLock::new(HashSet::new()),
        }
    }

    pub fn genesis_block(&self) -> Block {
        let blocks = self.blocks.read();
        blocks.get(&self.genesis_block_hash).unwrap().clone()
    }

    pub fn contains_block(&self, hash: &H256) -> bool {
        self.blocks.read().contains_key(hash)
    }

    pub fn verified_invalid(&self, hash: &H256) -> bool {
        self.invalid_blocks.read().contains(hash)
    }

    pub fn invalidate_block(&self, hash: &H256) {
        self.invalid_blocks.write().insert(hash.clone());
    }

    pub fn get_block_total_difficulty(&self, hash: &H256) -> Option<U256> {
        let r = self.inner.read();
        if let Some(idx) = r.indices.get(hash) {
            Some(r.arena[*idx].total_difficulty)
        } else {
            None
        }
    }

    pub fn get_block_height(&self, hash: &H256) -> Option<u64> {
        let blocks = self.blocks.read();
        if let Some(block) = blocks.get(hash) {
            Some(block.block_header.height())
        } else {
            None
        }
    }

    pub fn get_block_epoch_number(&self, hash: &H256) -> Option<usize> {
        let r = self.inner.read();
        if let Some(idx) = r.indices.get(hash) {
            Some(r.arena[*idx].data.epoch_number.borrow().clone())
        } else {
            None
        }
    }

    pub fn on_new_block(
        &self, hash: &H256, terminal_hashes: &mut HashSet<H256>,
    ) -> H256 {
        let blocks = self.blocks.read();
        let block = blocks.get(hash).unwrap();

        debug!(
            "insert new block into consensus: hash={:?}, block_header={:?} tx_count={}",
            block.hash(),
            block.block_header,
            block.transactions.len(),
        );

        for tx in block.transactions.iter() {
            self.txpool.remove_pending(&tx);
            self.txpool.remove_ready(tx.clone());
        }

        self.inner.write().on_new_block(
            &self.txpool,
            block,
            &*blocks,
            terminal_hashes,
        )
    }

    pub fn best_block_hash(&self) -> H256 {
        self.inner.read().best_block_hash()
    }

    pub fn block_count(&self) -> usize { self.blocks.read().len() }

    pub fn get_block_for_tx_execution(
        &self, tx_hash: &H256,
    ) -> Option<(bool, H256)> {
        self.inner.read().get_block_for_tx_execution(tx_hash)
    }
}

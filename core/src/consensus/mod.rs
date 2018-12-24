use crate::{
    executive::{ExecutionError, Executive},
    ext_db::SystemDB,
    machine::new_byzantium_test_machine,
    state::State,
    statedb::StateDb,
    storage::{state::StateTrait, StorageManager, StorageManagerTrait},
    sync::SynchronizationGraphInner,
    transaction_pool::SharedTransactionPool,
    vm::{EnvInfo, Spec},
    vm_factory::VmFactory,
};
use ethereum_types::{H256, U256};
use parking_lot::RwLock;
use primitives::Block;
use slab::Slab;
use std::{
    cell::RefCell,
    cmp::min,
    collections::{HashMap, HashSet, VecDeque},
    iter::FromIterator,
    sync::Arc,
};

const DEFERRED_STATE_EPOCH_COUNT: u64 = 100;

const NULL: usize = !0;

pub struct ConsensusGraphNodeData {
    pub epoch_number: RefCell<usize>,
    pub partial_invalid: bool,
    pub anticone: HashSet<usize>,
}

unsafe impl Sync for ConsensusGraphNodeData {}

impl ConsensusGraphNodeData {
    pub fn new(epoch_number: usize) -> Self {
        ConsensusGraphNodeData {
            epoch_number: RefCell::new(epoch_number),
            partial_invalid: false,
            anticone: HashSet::new(),
        }
    }
}

pub struct ConsensusGraphNode {
    pub hash: H256,
    pub height: u64,
    pub difficulty: U256,
    pub total_difficulty: U256,
    pub parent: usize,
    pub children: Vec<usize>,
    pub referrers: Vec<usize>,
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
    genesis_block_state_root: H256,
    parental_terminals: HashSet<usize>,
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
            genesis_block_state_root: genesis_block
                .block_header
                .deferred_state_root()
                .clone(),
            parental_terminals: HashSet::new(),
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
            height: block.block_header.height(),
            difficulty: *block.block_header.difficulty(),
            total_difficulty: block.block_header.difficulty().clone(),
            parent,
            children: Vec::new(),
            referees,
            referrers: Vec::new(),
            data: ConsensusGraphNodeData::new(NULL),
        });
        self.indices.insert(hash, index);

        if parent != NULL {
            self.parental_terminals.remove(&parent);
            self.parental_terminals.insert(index);
            terminal_hashes.remove(&self.arena[parent].hash);
            terminal_hashes.insert(hash);
            self.arena[parent].children.push(index);
            let referees = self.arena[index].referees.clone();
            for referee in referees {
                self.arena[referee].referrers.push(index);
            }
        }

        index
    }

    fn check_correct_parent(
        &self, me_in_consensus: usize,
        sync_graph: &mut SynchronizationGraphInner,
    ) -> bool
    {
        struct ForkPointInfo {
            pivot_index: usize,
            fork_total_difficulty: U256,
        }

        let _me_in_sync = *sync_graph
            .indices
            .get(&self.arena[me_in_consensus].hash)
            .unwrap();

        let mut fork_points: HashMap<usize, ForkPointInfo> = HashMap::new();
        let mut pivot_points: HashMap<usize, U256> = HashMap::new();
        let mut min_fork_height = u64::max_value();

        let anticone = &self.arena[me_in_consensus].data.anticone;
        let mut anticone_parents = HashSet::new();
        for index in anticone {
            let parent = self.arena[*index].parent;
            debug_assert!(parent != NULL);
            if !anticone_parents.contains(&parent) {
                anticone_parents.insert(parent);
            }
        }

        let terminal_anticone_parent = anticone_parents
            .union(&self.parental_terminals)
            .cloned()
            .collect::<HashSet<_>>();
        let fork_terminals = terminal_anticone_parent
            .difference(anticone)
            .cloned()
            .collect::<HashSet<_>>();

        for terminal in fork_terminals {
            let mut me = me_in_consensus;
            let mut fork = terminal;
            while self.arena[me].height > self.arena[fork].height {
                me = self.arena[me].parent;
            }
            if me == fork {
                //FIXME: Maybe we should treat this as invalid block.
                continue;
            }
            while self.arena[fork].height > self.arena[me].height {
                fork = self.arena[fork].parent;
            }
            debug_assert!(fork != me);
            let mut prev_fork = NULL;
            let mut prev_me = NULL;
            while fork != me {
                prev_fork = fork;
                prev_me = me;
                debug_assert!(self.arena[fork].height == self.arena[me].height);
                fork = self.arena[fork].parent;
                me = self.arena[me].parent;
            }
            fork_points.entry(prev_fork).or_insert(ForkPointInfo {
                pivot_index: prev_me,
                fork_total_difficulty: self.arena[prev_fork].total_difficulty,
            });
            pivot_points
                .entry(prev_me)
                .or_insert(self.arena[prev_me].total_difficulty);

            min_fork_height = min(min_fork_height, self.arena[prev_me].height);
        }

        if fork_points.is_empty() {
            debug_assert!(pivot_points.is_empty());
            return true;
        }

        // Remove difficulty contribution of anticone for fork points
        for index in anticone {
            let difficulty = self.arena[*index].difficulty;
            let mut upper = self.arena[*index].parent;
            debug_assert!(upper != NULL);
            loop {
                if self.arena[upper].height < min_fork_height {
                    break;
                }

                if let Some(fork_info) = fork_points.get_mut(&upper) {
                    debug_assert!(!pivot_points.contains_key(&upper));
                    fork_info.fork_total_difficulty -= difficulty;
                    break;
                } else if pivot_points.contains_key(&upper) {
                    let height = self.arena[upper].height;
                    for (pivot_index, pivot_total_difficulty) in
                        pivot_points.iter_mut()
                    {
                        if self.arena[*pivot_index].height <= height {
                            *pivot_total_difficulty -= difficulty;
                        }
                    }
                    break;
                }
                upper = self.arena[upper].parent;
            }
        }

        // Check the pivot selection decision.
        for (index, fork_info) in fork_points {
            if (fork_info.fork_total_difficulty, self.arena[index].hash)
                > (
                    pivot_points.get(&fork_info.pivot_index).unwrap().clone(),
                    self.arena[fork_info.pivot_index].hash,
                ) {
                return false;
            }
        }

        true
    }

    pub fn compute_anticone(&mut self, me: usize) {
        let parent = self.arena[me].parent;
        debug_assert!(parent != NULL);
        debug_assert!(self.arena[me].children.is_empty());
        debug_assert!(self.arena[me].referrers.is_empty());

        // Compute future set of parent
        let mut parent_futures: HashSet<usize> = HashSet::new();
        let mut queue: VecDeque<usize> = VecDeque::new();
        let mut visited: HashSet<usize> = HashSet::new();
        queue.push_back(parent);
        while let Some(index) = queue.pop_front() {
            if visited.contains(&index) {
                continue;
            }
            if index != parent && index != me {
                parent_futures.insert(index);
            }

            visited.insert(index);
            for child in &self.arena[index].children {
                queue.push_back(*child);
            }
            for referrer in &self.arena[index].referrers {
                queue.push_back(*referrer);
            }
        }

        let anticone = {
            let parent_anticone = &self.arena[parent].data.anticone;
            let mut my_past: HashSet<usize> = HashSet::new();
            debug_assert!(queue.is_empty());
            queue.push_back(me);
            while let Some(index) = queue.pop_front() {
                if my_past.contains(&index) {
                    continue;
                }

                debug_assert!(index != parent);
                if index != me {
                    my_past.insert(index);
                }

                let idx_parent = self.arena[index].parent;
                debug_assert!(idx_parent != NULL);
                if parent_anticone.contains(&idx_parent)
                    || parent_futures.contains(&idx_parent)
                {
                    queue.push_back(idx_parent);
                }

                for referee in &self.arena[index].referees {
                    if parent_anticone.contains(referee)
                        || parent_futures.contains(referee)
                    {
                        queue.push_back(*referee);
                    }
                }
            }
            parent_futures
                .union(parent_anticone)
                .cloned()
                .collect::<HashSet<_>>()
                .difference(&my_past)
                .cloned()
                .collect::<HashSet<_>>()
        };

        for index in &anticone {
            self.arena[*index].data.anticone.insert(me);
        }

        self.arena[me].data.anticone = anticone;
    }

    pub fn on_new_block(
        &mut self, txpool: &SharedTransactionPool, block: &Block,
        block_by_hash: &HashMap<H256, Block>,
        sync_graph: &mut SynchronizationGraphInner,
    ) -> (H256, H256)
    {
        let new = self.insert(block, &mut sync_graph.terminal_block_hashes);
        self.compute_anticone(new);

        if self.arena[self.arena[new].parent].data.partial_invalid {
            self.arena[new].data.partial_invalid = true;
            trace!(
                "Partially invalid due to partially invalid parent. {:?}",
                block.block_header.clone()
            );
            return (
                self.best_block_hash(),
                self.deferred_state_root_following_best_block(),
            );
        }

        // Check whether the new block select the correct parent block
        if self.arena[new].parent != *self.pivot_chain.last().unwrap() {
            if !self.check_correct_parent(new, sync_graph) {
                self.arena[new].data.partial_invalid = true;
                trace!(
                    "Partially invalid due to picking incorrect parent. {:?}",
                    block.block_header.clone()
                );
                return (
                    self.best_block_hash(),
                    self.deferred_state_root_following_best_block(),
                );
            }
        }

        let mut me = new;
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
            if let Some(heaviest) = self.arena[me]
                .children
                .iter()
                .filter(|&index| !self.arena[*index].data.partial_invalid)
                .max_by_key(|index| {
                    (
                        self.arena[**index].total_difficulty,
                        self.arena[**index].hash,
                    )
                })
                .cloned()
            {
                me = heaviest;
            } else {
                break;
            }
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
            // FIXME: We may want to propagate the error up
            state
                .commit_and_notify(
                    self.arena[new_pivot_chain[fork_at]].hash,
                    txpool,
                )
                .unwrap();

            fork_at += 1;
        }

        if *new_pivot_chain.last().unwrap() == new {
            debug_assert!(new_pivot_chain.len() >= 2);
            let expected_state_root = self
                .deferred_state_root(
                    &new_pivot_chain[0..new_pivot_chain.len() - 1],
                )
                .unwrap();
            let actual_state_root = *block.block_header.deferred_state_root();
            if expected_state_root != actual_state_root {
                let difficulty = *block.block_header.difficulty();
                self.arena[new].data.partial_invalid = true;
                let mut me = new;
                loop {
                    me = self.arena[me].parent;
                    self.arena[me].total_difficulty -= difficulty;
                    if me == self.genesis_block_index {
                        break;
                    }
                }
                trace!("Partially invalid in pivot chain due to incorrect state root. {:?}", block.block_header.clone());
                return (
                    self.best_block_hash(),
                    self.deferred_state_root_following_best_block(),
                );
            }
        } else {
            debug_assert!(
                block.block_header.height() == self.arena[new].height
            );
            let state_root_valid = if block.block_header.height()
                < DEFERRED_STATE_EPOCH_COUNT
            {
                *block.block_header.deferred_state_root()
                    == self.genesis_block_state_root
            } else {
                let mut deferred = new;
                for _ in 0..DEFERRED_STATE_EPOCH_COUNT {
                    deferred = self.arena[deferred].parent;
                }
                debug_assert!(
                    block.block_header.height() - DEFERRED_STATE_EPOCH_COUNT
                        == self.arena[deferred].height
                );

                let height = self.arena[deferred].height as usize;
                if height < new_pivot_chain.len()
                    && new_pivot_chain[height] == deferred
                {
                    *block.block_header.deferred_state_root()
                        == self
                            .storage_manager
                            .get_state_at(self.arena[deferred].hash)
                            .unwrap()
                            .get_state_root()
                            .unwrap()
                            .unwrap()
                } else {
                    //FIXME: Verify the deferred state root in this costly
                    // case.
                    true
                }
            };

            if !state_root_valid {
                let difficulty = *block.block_header.difficulty();
                self.arena[new].data.partial_invalid = true;
                let mut me = new;
                loop {
                    me = self.arena[me].parent;
                    self.arena[me].total_difficulty -= difficulty;
                    if me == self.genesis_block_index {
                        break;
                    }
                }
                trace!(
                    "Partially invalid in fork due to incorrect parent. {:?}",
                    block.block_header.clone()
                );
                return (
                    self.best_block_hash(),
                    self.deferred_state_root_following_best_block(),
                );
            }
        }

        self.pivot_chain = new_pivot_chain;
        (
            self.best_block_hash(),
            self.deferred_state_root_following_best_block(),
        )
    }

    pub fn best_block_hash(&self) -> H256 {
        self.arena[*self.pivot_chain.last().unwrap()].hash
    }

    pub fn deferred_state_root(&self, chain: &[usize]) -> Option<H256> {
        let chain_len = chain.len();
        let index = if chain_len < DEFERRED_STATE_EPOCH_COUNT as usize {
            0
        } else {
            chain_len - DEFERRED_STATE_EPOCH_COUNT as usize
        };
        let state = self
            .storage_manager
            .get_state_at(self.arena[chain[index]].hash)
            .unwrap();
        state.get_state_root().unwrap()
    }

    pub fn deferred_state_root_following_best_block(&self) -> H256 {
        self.deferred_state_root(&self.pivot_chain).unwrap()
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
        &self, hash: &H256, sync_graph: &mut SynchronizationGraphInner,
    ) -> (H256, H256) {
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
            sync_graph,
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

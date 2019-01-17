use crate::{
    cache_config::CacheConfig,
    cache_manager::CacheManager,
    db::{COL_BLOCKS, COL_MISC},
    executive::{ExecutionError, Executive},
    ext_db::SystemDB,
    machine::new_byzantium_test_machine,
    state::{CleanupMode, State},
    statedb::StateDb,
    storage::{state::StateTrait, StorageManager, StorageManagerTrait},
    sync::SynchronizationGraphInner,
    transaction_pool::SharedTransactionPool,
    vm::{EnvInfo, Spec},
    vm_factory::VmFactory,
};
use ethereum_types::{Address, H256, U256, U512};
use heapsize::HeapSizeOf;
use lru::LruCache;
use parking_lot::{Mutex, RwLock};
use primitives::{Block, BlockHeader, SignedTransaction};
use rlp::{Rlp, RlpStream};
use slab::Slab;
use std::{
    cell::RefCell,
    cmp::min,
    collections::{HashMap, HashSet, VecDeque},
    iter::FromIterator,
    sync::Arc,
};

pub const DEFERRED_STATE_EPOCH_COUNT: u64 = 5;
const REWARD_EPOCH_COUNT: u64 = 100;
const ANTICONE_PENALTY_UPPER_EPOCH_COUNT: u64 = 10;
const ANTICONE_PENALTY_RATIO: u64 = 100;
const BASE_MINING_REWARD: u64 = 1000000;

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
    pub block_for_transaction: LruCache<H256, (bool, usize)>,
    genesis_block_index: usize,
    genesis_block_state_root: H256,
    parental_terminals: HashSet<usize>,
    indices_in_epochs: HashMap<usize, Vec<usize>>,
    block_fees: HashMap<usize, U256>,
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
            block_for_transaction: LruCache::new(10000),
            genesis_block_index: NULL,
            genesis_block_state_root: genesis_block
                .block_header
                .deferred_state_root()
                .clone(),
            parental_terminals: HashSet::new(),
            indices_in_epochs: HashMap::new(),
            block_fees: HashMap::new(),
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
                )
            {
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

        debug!(
            "Block {} anticone size {}",
            self.arena[me].hash,
            anticone.len()
        );
        self.arena[me].data.anticone = anticone;
    }

    fn topological_sort(&self, queue: &Vec<usize>) -> Vec<usize> {
        let index_set: HashSet<usize> =
            HashSet::from_iter(queue.iter().cloned());
        let mut num_incoming_edges = HashMap::new();

        for me in queue {
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

        for me in queue {
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
                    num_incoming_edges.entry(*referee).and_modify(|e| *e -= 1);
                    if num_incoming_edges[referee] == 0 {
                        candidates.insert(*referee);
                    }
                }
            }
        }
        reversed_indices.reverse();
        reversed_indices
    }

    pub fn call_virtual(
        &self, tx: &SignedTransaction,
    ) -> Result<Vec<u8>, ExecutionError> {
        let spec = Spec::new_byzantium();
        let machine = new_byzantium_test_machine();
        let mut state = State::new(
            StateDb::new(
                self.storage_manager
                    .get_state_at(self.best_state_block_hash())
                    .unwrap(),
            ),
            0.into(),
            self.vm.clone(),
        );
        let env = EnvInfo {
            number: 0, // TODO: replace 0 with correct cardinal number
            author: Default::default(),
            timestamp: Default::default(),
            difficulty: Default::default(),
            gas_used: U256::zero(),
            gas_limit: tx.gas.clone(),
        };
        let mut ex = Executive::new(&mut state, &env, &machine, &spec);
        let r = ex.transact(tx);
        trace!("Execution result {:?}", r);
        r.map(|r| r.output)
    }

    pub fn best_block_hash(&self) -> H256 {
        self.arena[*self.pivot_chain.last().unwrap()].hash
    }

    pub fn best_state_block_hash(&self) -> H256 {
        let pivot_len = self.pivot_chain.len();
        let index = if pivot_len < DEFERRED_STATE_EPOCH_COUNT as usize {
            0
        } else {
            pivot_len - DEFERRED_STATE_EPOCH_COUNT as usize
        };

        self.arena[self.pivot_chain[index]].hash
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
        trace!(
            "get state, hash ({:?}), chain len: {}, chain index: {}",
            self.arena[chain[index]].hash,
            chain_len,
            index
        );
        state.get_state_root().unwrap()
    }

    pub fn deferred_state_root_following_best_block(&self) -> H256 {
        self.deferred_state_root(&self.pivot_chain).unwrap()
    }

    pub fn get_block_for_tx_execution(
        &self, tx_hash: &H256,
    ) -> Option<(bool, H256)> {
        self.block_for_transaction
            .peek(tx_hash)
            .map(|(success, index)| (*success, self.arena[*index].hash))
    }

    pub fn check_block_confirmation(&self, _block_hash: &H256) -> bool {
        // TODO check if the block is confirmed safely
        true
    }
}

#[derive(Debug, Hash, Eq, PartialEq, Clone)]
enum CacheId {
    Block(H256),
}

#[derive(Debug)]
pub struct CacheSize {
    /// Blocks cache size.
    pub blocks: usize,
}

impl CacheSize {
    /// Total amount used by the cache.
    pub fn total(&self) -> usize { self.blocks }
}

pub struct ConsensusGraph {
    pub blocks: Arc<RwLock<HashMap<H256, Arc<Block>>>>,
    pub inner: RwLock<ConsensusGraphInner>,
    genesis_block: Arc<Block>,
    pub txpool: SharedTransactionPool,
    // This db is used to persist information related to
    // ledger structure, like block- or transaction-related
    // stuffs.
    pub db: Arc<SystemDB>,
    cache_man: Mutex<CacheManager<CacheId>>,
    pub invalid_blocks: RwLock<HashSet<H256>>,
}

pub type SharedConsensusGraph = Arc<ConsensusGraph>;

impl ConsensusGraph {
    pub fn with_genesis_block(
        genesis_block: Block, state_mananger: Arc<StorageManager>,
        vm: VmFactory, txpool: SharedTransactionPool, db: Arc<SystemDB>,
        cache_config: CacheConfig,
    ) -> Self
    {
        let mb = 1024 * 1024;
        let max_cache_size = cache_config.ledger_mb() * mb;
        let pref_cache_size = max_cache_size * 3 * mb / 4;
        // 400 is the average size of the key. TODO(ming): make sure this again.
        let cache_man =
            CacheManager::new(pref_cache_size, max_cache_size, 4 * mb);

        let consensus_graph = ConsensusGraph {
            inner: RwLock::new(ConsensusGraphInner::with_genesis_block(
                &genesis_block,
                state_mananger,
                vm,
            )),
            blocks: Arc::new(RwLock::new(HashMap::new())),
            genesis_block: Arc::new(genesis_block),
            txpool,
            db,
            cache_man: Mutex::new(cache_man),
            invalid_blocks: RwLock::new(HashSet::new()),
        };

        consensus_graph
            .insert_block_to_kv(consensus_graph.genesis_block(), true);

        consensus_graph
    }

    pub fn block_header_by_hash(&self, hash: &H256) -> Option<BlockHeader> {
        let result = self.block_by_hash(hash)?;
        Some(result.block_header.clone())
    }

    pub fn block_by_hash_from_db(&self, hash: &H256) -> Option<Block> {
        let block = self.db.key_value().get(COL_BLOCKS, hash)
            .expect("Low level database error when fetching block. Some issue with disk?")?;
        let rlp = Rlp::new(&block);
        let mut block = rlp.as_val::<Block>().expect("Wrong block rlp format!");
        block
            .recover_public(&mut *self.txpool.transaction_pubkey_cache.write())
            .expect("Failed to recover public!");
        Some(block)
    }

    pub fn block_by_hash(&self, hash: &H256) -> Option<Arc<Block>> {
        // Check cache first
        {
            let read = self.blocks.read();
            if let Some(v) = read.get(hash) {
                return Some(v.clone());
            }
        }

        let block = self.block_by_hash_from_db(hash)?;
        let block = Arc::new(block);

        let mut write = self.blocks.write();
        write.insert(*hash, block.clone());

        self.cache_man.lock().note_used(CacheId::Block(*hash));
        Some(block)
    }

    pub fn insert_block_to_kv(&self, block: Arc<Block>, persistent: bool) {
        let hash = block.hash();

        if persistent {
            let mut dbops = self.db.key_value().transaction();
            dbops.put(COL_BLOCKS, &hash, &rlp::encode(block.as_ref()));
            self.db.key_value().write_buffered(dbops);
        }

        self.blocks.write().insert(hash, block);
        self.cache_man.lock().note_used(CacheId::Block(hash));
    }

    pub fn remove_block_from_kv(&self, hash: &H256) {
        self.blocks.write().remove(hash);
        let mut dbops = self.db.key_value().transaction();
        dbops.delete(COL_BLOCKS, hash);
        self.db.key_value().write_buffered(dbops);
    }

    pub fn block_height_by_hash(&self, hash: &H256) -> Option<u64> {
        let result = self.block_by_hash(hash)?;
        Some(result.block_header.height())
    }

    pub fn genesis_block(&self) -> Arc<Block> { self.genesis_block.clone() }

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

    pub fn get_block_epoch_number(&self, hash: &H256) -> Option<usize> {
        let r = self.inner.read();
        if let Some(idx) = r.indices.get(hash) {
            Some(r.arena[*idx].data.epoch_number.borrow().clone())
        } else {
            None
        }
    }

    fn process_epoch_transactions(
        &self, state: &mut State, arena: &Slab<ConsensusGraphNode>,
        epoch_blocks: &Vec<usize>, block_fees: &mut HashMap<usize, U256>,
        mut block_for_transaction: Option<&mut LruCache<H256, (bool, usize)>>,
        re_pending: bool, to_pending: &mut Vec<Arc<SignedTransaction>>,
    )
    {
        let spec = Spec::new_byzantium();
        let machine = new_byzantium_test_machine();
        for index in epoch_blocks.iter() {
            let block = self.block_by_hash(&arena[*index].hash).unwrap();
            debug!(
                "process txs in block: hash={:?}, tx count={:?}",
                block.hash(),
                block.transactions.len()
            );
            let env = EnvInfo {
                number: 0, // TODO: replace 0 with correct cardinal number
                author: block.block_header.author().clone(),
                timestamp: block.block_header.timestamp(),
                difficulty: block.block_header.difficulty().clone(),
                gas_used: U256::zero(),
                gas_limit: U256::from(block.block_header.gas_limit()),
            };
            let mut accumulated_fee: U256 = 0.into();
            let mut ex = Executive::new(state, &env, &machine, &spec);
            let mut n_invalid_nonce = 0;
            let mut n_ok = 0;
            let mut n_other = 0;
            for transaction in &block.transactions {
                let r = ex.transact(transaction);
                match r {
                    Err(ExecutionError::NotEnoughBaseGas {
                        required: _,
                        got: _,
                    })
                    | Err(ExecutionError::SenderMustExist {})
                    | Err(ExecutionError::Internal(_)) => {
                        warn!(
                            "tx execution error: transaction={:?}, err={:?}",
                            transaction, r
                        );
                    }
                    Err(ExecutionError::InvalidNonce { expected, got }) => {
                        n_invalid_nonce += 1;
                        trace!("tx execution InvalidNonce without inc_nonce: transaction={:?}, err={:?}", transaction.clone(), r);
                        if re_pending && got > expected {
                            trace!(
                                "To re-add transaction ({:?}) to pending pool",
                                transaction.clone()
                            );
                            to_pending.push(transaction.clone());
                        }
                    }
                    Ok(executed) => {
                        n_ok += 1;
                        trace!("tx executed successfully: transaction={:?}, result={:?}, in block {:?}", transaction, executed, arena[*index].hash.clone());
                        accumulated_fee += executed.fee;
                        if let Some(ref mut block_for_transaction) =
                            block_for_transaction
                        {
                            block_for_transaction
                                .put(transaction.hash(), (true, *index));
                        }
                    }
                    _ => {
                        n_other += 1;
                        trace!("tx executed: transaction={:?}, result={:?}, in block {:?}", transaction, r, arena[*index].hash.clone());
                        if let Some(ref mut block_for_transaction) =
                            block_for_transaction
                        {
                            block_for_transaction
                                .put(transaction.hash(), (true, *index));
                        }
                    }
                }
            }
            debug!(
                "n_invalid_nonce={}, n_ok={}, n_other={}",
                n_invalid_nonce, n_ok, n_other
            );
            block_fees.insert(*index, accumulated_fee);
        }
        debug!("Finish processing tx for epoch");
    }

    /// This is a very expensive call to force the engine to recompute the state
    /// root of a given block
    pub fn compute_state_for_block(
        &self, block_hash: &H256, inner: &ConsensusGraphInner,
    ) -> H256 {
        // If we already computed the state of the block before, we should not
        // do it again FIXME: propagate the error up
        let cached_state = inner
            .storage_manager
            .get_state_at(block_hash.clone())
            .unwrap();
        if cached_state.does_exist() {
            return cached_state.get_state_root().unwrap().unwrap();
        }
        // FIXME: propagate the error up
        let me: usize = inner.indices.get(block_hash).unwrap().clone();
        let block_height = inner.arena[me].height as usize;
        let mut fork_height = block_height;
        let mut chain: Vec<usize> = Vec::new();
        let mut idx = me;
        while fork_height > 0
            && (fork_height >= inner.pivot_chain.len()
                || inner.pivot_chain[fork_height] != idx)
        {
            chain.push(idx);
            fork_height -= 1;
            idx = inner.arena[idx].parent;
        }
        // Because we have genesis at height 0, this should always be true
        debug_assert!(inner.pivot_chain[fork_height] == idx);
        chain.push(idx);
        chain.reverse();
        let mut epoch_number_map: HashMap<usize, usize> = HashMap::new();
        let mut block_fees: HashMap<usize, U256> = HashMap::new();
        let mut indices_in_epochs: HashMap<usize, Vec<usize>> = HashMap::new();

        // Construct epochs
        for fork_at in 1..chain.len() {
            // First, identify all the blocks in the current epoch of the
            // hypothetical pivot chain
            let mut queue = Vec::new();
            {
                let new_epoch_number = fork_at + fork_height;
                let enqueue_if_new =
                    |queue: &mut Vec<usize>,
                     epoch_number_map: &mut HashMap<usize, usize>,
                     index| {
                        let epoch_number =
                            inner.arena[index].data.epoch_number.borrow();
                        if (*epoch_number == NULL
                            || *epoch_number > fork_height)
                            && !epoch_number_map.contains_key(&index)
                        {
                            epoch_number_map.insert(index, new_epoch_number);
                            queue.push(index);
                        }
                    };

                let mut at = 0;
                enqueue_if_new(
                    &mut queue,
                    &mut epoch_number_map,
                    chain[fork_at],
                );
                while at < queue.len() {
                    let me = queue[at];
                    for referee in &inner.arena[me].referees {
                        enqueue_if_new(
                            &mut queue,
                            &mut epoch_number_map,
                            *referee,
                        );
                    }
                    enqueue_if_new(
                        &mut queue,
                        &mut epoch_number_map,
                        inner.arena[me].parent,
                    );
                    at += 1;
                }
            }

            // Second, sort all the blocks based on their topological order
            // and break ties with block hash
            let reversed_indices = inner.topological_sort(&queue);

            debug!(
                "Construct epoch_id={}, block_count={}",
                inner.arena[chain[fork_at]].hash,
                reversed_indices.len()
            );

            indices_in_epochs.insert(chain[fork_at], reversed_indices);
        }

        let mut last_state_height =
            if inner.pivot_chain.len() > DEFERRED_STATE_EPOCH_COUNT as usize {
                inner.pivot_chain.len() - DEFERRED_STATE_EPOCH_COUNT as usize
            } else {
                0
            };

        last_state_height += 1;
        while last_state_height <= fork_height {
            let reversed_indices = inner
                .indices_in_epochs
                .get(&inner.pivot_chain[last_state_height])
                .unwrap();

            let mut state = State::new(
                StateDb::new(
                    inner
                        .storage_manager
                        .get_state_at(
                            inner.arena
                                [inner.pivot_chain[last_state_height - 1]]
                                .hash,
                        )
                        .unwrap(),
                ),
                0.into(),
                inner.vm.clone(),
            );
            debug!(
                "Process tx epoch_id={}, block_count={}",
                inner.arena[inner.pivot_chain[last_state_height]].hash,
                reversed_indices.len()
            );
            self.process_epoch_transactions(
                &mut state,
                &inner.arena,
                reversed_indices,
                &mut block_fees,
                None,
                false,
                &mut Vec::new(),
            );

            // Calculate the block reward for blocks inside the epoch
            // All transaction fees are shared among blocks inside one epoch
            if last_state_height > REWARD_EPOCH_COUNT as usize {
                let epoch_num = last_state_height - REWARD_EPOCH_COUNT as usize;
                let anticone_penalty_epoch_upper =
                    epoch_num + ANTICONE_PENALTY_UPPER_EPOCH_COUNT as usize;
                let mut pivot_block_upper =
                    inner.pivot_chain[anticone_penalty_epoch_upper];
                if anticone_penalty_epoch_upper > fork_height {
                    pivot_block_upper =
                        chain[anticone_penalty_epoch_upper - fork_height];
                }
                let penalty_upper_anticone =
                    &inner.arena[pivot_block_upper].data.anticone;
                let pivot_index = inner.pivot_chain[epoch_num];
                debug_assert!(
                    epoch_num == inner.arena[pivot_index].height as usize
                );
                let block_fee_closure = |index: usize| -> U256 {
                    inner.block_fees.get(&index).unwrap().clone()
                };
                let indices_in_epoch =
                    inner.indices_in_epochs.get(&pivot_index).unwrap();
                self.process_rewards_and_fees(
                    &mut state,
                    indices_in_epoch,
                    pivot_index,
                    penalty_upper_anticone,
                    inner,
                    block_fee_closure,
                );
            }

            // FIXME: We may want to propagate the error up
            state
                .commit(inner.arena[inner.pivot_chain[last_state_height]].hash)
                .unwrap();
            last_state_height += 1;
        }

        for fork_at in 1..chain.len() {
            let reversed_indices =
                indices_in_epochs.get(&chain[fork_at]).unwrap();

            // Third, apply transactions in the determined total order
            let mut state = State::new(
                StateDb::new(
                    inner
                        .storage_manager
                        .get_state_at(inner.arena[chain[fork_at - 1]].hash)
                        .unwrap(),
                ),
                0.into(),
                inner.vm.clone(),
            );
            debug!(
                "Process tx epoch_id={}, block_count={}",
                inner.arena[chain[fork_at]].hash,
                reversed_indices.len()
            );
            self.process_epoch_transactions(
                &mut state,
                &inner.arena,
                reversed_indices,
                &mut block_fees,
                None,
                false,
                &mut Vec::new(),
            );

            // Calculate the block reward for blocks inside the epoch
            // All transaction fees are shared among blocks inside one epoch
            if fork_height + fork_at > REWARD_EPOCH_COUNT as usize {
                let epoch_num =
                    fork_height + fork_at - REWARD_EPOCH_COUNT as usize;
                let anticone_penalty_epoch_upper =
                    epoch_num + ANTICONE_PENALTY_UPPER_EPOCH_COUNT as usize;
                let mut pivot_block_upper =
                    inner.pivot_chain[anticone_penalty_epoch_upper];
                if anticone_penalty_epoch_upper > fork_height {
                    pivot_block_upper =
                        chain[anticone_penalty_epoch_upper - fork_height];
                }
                let penalty_upper_anticone =
                    &inner.arena[pivot_block_upper].data.anticone;
                let mut pivot_index = inner.pivot_chain[epoch_num];
                let mut in_branch = false;
                if epoch_num > fork_height {
                    pivot_index = chain[epoch_num - fork_height];
                    in_branch = true;
                }
                debug_assert!(
                    epoch_num == inner.arena[pivot_index].height as usize
                );
                let block_fee_closure = |index: usize| -> U256 {
                    match in_branch {
                        true => block_fees.get(&index).unwrap(),
                        false => inner.block_fees.get(&index).unwrap(),
                    }
                    .clone()
                };
                let indices_in_epoch = match in_branch {
                    true => indices_in_epochs.get(&pivot_index).unwrap(),
                    false => inner.indices_in_epochs.get(&pivot_index).unwrap(),
                };
                self.process_rewards_and_fees(
                    &mut state,
                    indices_in_epoch,
                    pivot_index,
                    penalty_upper_anticone,
                    inner,
                    block_fee_closure,
                );
            }

            // FIXME: We may want to propagate the error up
            state.commit(inner.arena[chain[fork_at]].hash).unwrap();
        }

        // FIXME: Propagate errors upward
        inner
            .storage_manager
            .get_state_at(inner.arena[me].hash)
            .unwrap()
            .get_state_root()
            .unwrap()
            .unwrap()
    }

    pub fn compute_deferred_state_for_block(
        &self, block_hash: &H256, delay: usize,
    ) -> H256 {
        let inner = &*self.inner.read();

        // FIXME: Propagate errors upward
        let mut idx = inner.indices.get(block_hash).unwrap().clone();
        for _i in 0..delay {
            if idx == inner.genesis_block_index {
                break;
            }
            idx = inner.arena[idx].parent;
        }
        self.compute_state_for_block(&inner.arena[idx].hash, inner)
    }

    fn check_block_full_validity(
        &self, new: usize, block: &Block, inner: &ConsensusGraphInner,
        sync_graph: &mut SynchronizationGraphInner,
    ) -> bool
    {
        if inner.arena[inner.arena[new].parent].data.partial_invalid {
            warn!(
                "Partially invalid due to partially invalid parent. {:?}",
                block.block_header.clone()
            );
            return false;
        }

        // Check whether the new block select the correct parent block
        if inner.arena[new].parent != *inner.pivot_chain.last().unwrap() {
            if !inner.check_correct_parent(new, sync_graph) {
                warn!(
                    "Partially invalid due to picking incorrect parent. {:?}",
                    block.block_header.clone()
                );
                return false;
            }
        }

        // Check if the state root is correct or not
        // TODO: We may want to optimize this because now on the chain switch we
        // are going to compute state twice
        let state_root_valid =
            if block.block_header.height() < DEFERRED_STATE_EPOCH_COUNT {
                *block.block_header.deferred_state_root()
                    == inner.genesis_block_state_root
            } else {
                let mut deferred = new;
                for _ in 0..DEFERRED_STATE_EPOCH_COUNT {
                    deferred = inner.arena[deferred].parent;
                }
                debug_assert!(
                    block.block_header.height() - DEFERRED_STATE_EPOCH_COUNT
                        == inner.arena[deferred].height
                );

                /*let height = inner.arena[deferred].height as usize;
                if height < inner.pivot_chain.len()
                    && inner.pivot_chain[height] == deferred*/
                if inner
                    .storage_manager
                    .contains_state(inner.arena[deferred].hash)
                {
                    *block.block_header.deferred_state_root()
                        == inner
                            .storage_manager
                            .get_state_at(inner.arena[deferred].hash)
                            .unwrap()
                            .get_state_root()
                            .unwrap()
                            .unwrap()
                } else {
                    // Call the expensive function to check this state root
                    *block.block_header.deferred_state_root()
                        == self.compute_state_for_block(
                            &inner.arena[deferred].hash,
                            inner,
                        )
                }
            };

        if !state_root_valid {
            warn!(
                "Partially invalid in fork due to incorrect state root. {:?}",
                block.block_header.clone()
            );
            return false;
        }
        return true;
    }

    fn process_rewards_and_fees<F>(
        &self, state: &mut State, indices_in_epoch: &Vec<usize>,
        pivot_index: usize, penalty_upper_anticone: &HashSet<usize>,
        inner: &ConsensusGraphInner, block_fee_fn: F,
    ) where
        F: Fn(usize) -> U256,
    {
        let difficulty = inner.arena[pivot_index].difficulty;
        let mut epoch_accum_fee: U256 = 0.into();
        let mut rewards: Vec<(Address, U256)> = Vec::new();

        for index in indices_in_epoch {
            let block_fee = block_fee_fn(*index);
            assert!(U256::max_value() - epoch_accum_fee > block_fee);
            epoch_accum_fee += block_fee;

            if inner.arena[*index].data.partial_invalid {
                continue;
            }

            let mut reward: U512 =
                if inner.arena[*index].difficulty == difficulty {
                    BASE_MINING_REWARD.into()
                } else {
                    0.into()
                };

            if reward > 0.into() {
                let anticone_size = inner.arena[*index]
                    .data
                    .anticone
                    .difference(penalty_upper_anticone)
                    .cloned()
                    .collect::<HashSet<_>>()
                    .len();
                let penalty = (reward * U512::from(anticone_size))
                    / U512::from(ANTICONE_PENALTY_RATIO);
                if penalty > reward {
                    reward = 0.into();
                } else {
                    reward -= penalty;
                }
            }

            debug_assert!(reward <= U512::from(U256::max_value()));
            let reward = U256::from(reward);
            let author = self
                .block_by_hash(&inner.arena[*index].hash)
                .unwrap()
                .block_header
                .author()
                .clone();
            rewards.push((author, reward));
        }
        debug!("Give rewards reward={:?}", rewards);

        if !rewards.is_empty() {
            let block_count = U256::from(rewards.len());
            let quotient: U256 = epoch_accum_fee / block_count;
            let mut remainder: U256 =
                epoch_accum_fee - (block_count * quotient);
            for (_, reward) in &mut rewards {
                *reward += quotient;
                if !remainder.is_zero() {
                    *reward += 1.into();
                    remainder -= 1.into();
                }
            }

            for (address, reward) in rewards {
                state
                    .add_balance(&address, &reward, CleanupMode::ForceCreate)
                    .unwrap();
            }
        }
    }

    pub fn on_new_block(
        &self, hash: &H256, sync_graph: &mut SynchronizationGraphInner,
    ) -> (H256, H256) {
        let block = self.block_by_hash(hash).unwrap();

        info!(
            "insert new block into consensus: block_header={:?} tx_count={}",
            block.block_header,
            block.transactions.len(),
        );

        for tx in block.transactions.iter() {
            self.txpool.remove_pending(tx.as_ref());
            self.txpool.remove_ready(tx.clone());
        }
        info!("Transaction pool size={}", self.txpool.len());

        let mut inner = &mut *self.inner.write();

        let new =
            inner.insert(block.as_ref(), &mut sync_graph.terminal_block_hashes);
        inner.compute_anticone(new);

        let fully_valid = self.check_block_full_validity(
            new,
            block.as_ref(),
            inner,
            sync_graph,
        );
        if !fully_valid {
            inner.arena[new].data.partial_invalid = true;
            return (
                inner.best_block_hash(),
                inner.deferred_state_root_following_best_block(),
            );
        }
        debug!("Block {} is fully valid", inner.arena[new].hash);

        // Update the total difficulty for the new block
        let mut me = new;
        loop {
            me = inner.arena[me].parent;
            inner.arena[me].total_difficulty +=
                *block.block_header.difficulty();
            if me == inner.genesis_block_index {
                break;
            }
        }

        // Compute the new pivot chain
        let mut new_pivot_chain = Vec::new();
        me = inner.genesis_block_index;
        loop {
            new_pivot_chain.push(me);
            if let Some(heaviest) = inner.arena[me]
                .children
                .iter()
                .filter(|&index| !inner.arena[*index].data.partial_invalid)
                .max_by_key(|index| {
                    (
                        inner.arena[**index].total_difficulty,
                        inner.arena[**index].hash,
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
        while fork_at < inner.pivot_chain.len()
            && fork_at < new_pivot_chain.len()
        {
            if inner.pivot_chain[fork_at] != new_pivot_chain[fork_at] {
                break;
            }
            fork_at += 1;
        }

        if fork_at < inner.pivot_chain.len() {
            let enqueue_if_obsolete = |queue: &mut VecDeque<usize>, index| {
                let mut epoch_number =
                    inner.arena[index].data.epoch_number.borrow_mut();
                if *epoch_number != NULL && *epoch_number >= fork_at {
                    *epoch_number = NULL;
                    queue.push_back(index);
                }
            };

            let mut queue = VecDeque::new();
            enqueue_if_obsolete(&mut queue, *inner.pivot_chain.last().unwrap());
            while let Some(me) = queue.pop_front() {
                for referee in inner.arena[me].referees.clone() {
                    enqueue_if_obsolete(&mut queue, referee);
                }
                enqueue_if_obsolete(&mut queue, inner.arena[me].parent);
            }
        }

        assert!(fork_at != 0);

        // Construct epochs
        let mut pivot_index = fork_at;
        while pivot_index < new_pivot_chain.len() {
            // First, identify all the blocks in the current epoch
            let mut queue = Vec::new();
            {
                let copy_of_fork_at = pivot_index;
                let enqueue_if_new = |queue: &mut Vec<usize>, index| {
                    let mut epoch_number =
                        inner.arena[index].data.epoch_number.borrow_mut();
                    if *epoch_number == NULL {
                        *epoch_number = copy_of_fork_at;
                        queue.push(index);
                    }
                };

                let mut at = 0;
                enqueue_if_new(&mut queue, new_pivot_chain[pivot_index]);
                while at < queue.len() {
                    let me = queue[at];
                    for referee in &inner.arena[me].referees {
                        enqueue_if_new(&mut queue, *referee);
                    }
                    enqueue_if_new(&mut queue, inner.arena[me].parent);
                    at += 1;
                }
            }

            // Second, sort all the blocks based on their topological order
            // and break ties with block hash
            let reversed_indices = inner.topological_sort(&queue);

            debug!(
                "Construct epoch_id={}, block_count={}",
                inner.arena[new_pivot_chain[pivot_index]].hash,
                reversed_indices.len()
            );

            inner
                .indices_in_epochs
                .insert(new_pivot_chain[pivot_index], reversed_indices);

            pivot_index += 1;
        }

        let mut to_pending = Vec::new();
        let to_state_pos =
            if new_pivot_chain.len() < DEFERRED_STATE_EPOCH_COUNT as usize {
                0 as usize
            } else {
                new_pivot_chain.len() - DEFERRED_STATE_EPOCH_COUNT as usize + 1
            };

        let mut state_at = fork_at;
        if fork_at + DEFERRED_STATE_EPOCH_COUNT as usize
            > inner.pivot_chain.len()
        {
            if inner.pivot_chain.len() > DEFERRED_STATE_EPOCH_COUNT as usize {
                state_at = inner.pivot_chain.len()
                    - DEFERRED_STATE_EPOCH_COUNT as usize;
            } else {
                state_at = 1;
            }
        }

        // Apply transactions in the determined total order
        while state_at < to_state_pos {
            let mut state = State::new(
                StateDb::new(
                    inner
                        .storage_manager
                        .get_state_at(
                            inner.arena[new_pivot_chain[state_at - 1]].hash,
                        )
                        .unwrap(),
                ),
                0.into(),
                inner.vm.clone(),
            );

            let reversed_indices = inner
                .indices_in_epochs
                .get(&new_pivot_chain[state_at])
                .unwrap();

            debug!(
                "Process tx epoch_id={}, block_count={}",
                inner.arena[new_pivot_chain[state_at]].hash,
                reversed_indices.len()
            );
            self.process_epoch_transactions(
                &mut state,
                &inner.arena,
                reversed_indices,
                &mut inner.block_fees,
                Some(&mut inner.block_for_transaction),
                true,
                &mut to_pending,
            );

            // Calculate the block reward for blocks inside the epoch
            // All transaction fees are shared among blocks inside one epoch
            if state_at > REWARD_EPOCH_COUNT as usize {
                let epoch_num = state_at - REWARD_EPOCH_COUNT as usize;
                let anticone_penalty_epoch_upper =
                    epoch_num + ANTICONE_PENALTY_UPPER_EPOCH_COUNT as usize;
                let penalty_upper_anticone = &inner.arena
                    [new_pivot_chain[anticone_penalty_epoch_upper]]
                    .data
                    .anticone;
                let pivot_index = new_pivot_chain[epoch_num];
                debug_assert!(
                    epoch_num == inner.arena[pivot_index].height as usize
                );
                debug_assert!(
                    epoch_num
                        == *inner.arena[pivot_index].data.epoch_number.borrow()
                );
                let indices_in_epoch =
                    inner.indices_in_epochs.get(&pivot_index).unwrap();
                self.process_rewards_and_fees(
                    &mut state,
                    indices_in_epoch,
                    pivot_index,
                    penalty_upper_anticone,
                    inner,
                    |index| -> U256 { *inner.block_fees.get(&index).unwrap() },
                );
            }

            // FIXME: We may want to propagate the error up
            state
                .commit_and_notify(
                    inner.arena[new_pivot_chain[state_at]].hash,
                    &self.txpool,
                )
                .unwrap();

            state_at += 1;
        }

        if state_at > 1 {
            state_at -= 1;
            let state = inner
                .storage_manager
                .get_state_at(inner.arena[new_pivot_chain[state_at]].hash)
                .unwrap();
            self.txpool.recycle_future_transactions(to_pending, state);
        }

        inner.pivot_chain = new_pivot_chain;
        (
            inner.best_block_hash(),
            inner.deferred_state_root_following_best_block(),
        )
    }

    pub fn best_block_hash(&self) -> H256 {
        self.inner.read().best_block_hash()
    }

    pub fn best_state_block_hash(&self) -> H256 {
        self.inner.read().best_state_block_hash()
    }

    pub fn block_count(&self) -> usize { self.inner.read().indices.len() }

    pub fn get_block_for_tx_execution(
        &self, tx_hash: &H256,
    ) -> Option<(bool, H256)> {
        self.inner.read().get_block_for_tx_execution(tx_hash)
    }

    pub fn check_block_confirmation(&self, block_hash: &H256) -> bool {
        self.inner.read().check_block_confirmation(block_hash)
    }

    pub fn call_virtual(
        &self, tx: &SignedTransaction,
    ) -> Result<Vec<u8>, ExecutionError> {
        self.inner.write().call_virtual(tx)
    }

    /// Get current cache size.
    pub fn cache_size(&self) -> CacheSize {
        CacheSize {
            blocks: self.blocks.read().heap_size_of_children(),
        }
    }

    pub fn persist_terminals(&self) {
        let terminals = {
            let inner = self.inner.read();
            let mut terminals =
                Vec::with_capacity(inner.parental_terminals.len());
            for index in &inner.parental_terminals {
                terminals.push(inner.arena[*index].hash);
            }
            terminals
        };

        let mut rlp_stream = RlpStream::new();
        rlp_stream.begin_list(terminals.len());
        for hash in terminals {
            rlp_stream.append(&hash);
        }

        let mut dbops = self.db.key_value().transaction();
        dbops.put(COL_MISC, b"terminals", &rlp_stream.drain());
        self.db.key_value().write_buffered(dbops);
        // FIXME This is just a workaround!!!
        self.db.key_value().flush().unwrap();
    }

    pub fn block_cache_gc(&self) {
        let current_size = self.cache_size().total();

        let mut blocks = self.blocks.write();

        let mut cache_man = self.cache_man.lock();
        cache_man.collect_garbage(current_size, |ids| {
            for id in &ids {
                match *id {
                    CacheId::Block(ref h) => {
                        blocks.remove(h);
                    }
                }
            }

            blocks.shrink_to_fit();

            blocks.heap_size_of_children()
        });
    }
}

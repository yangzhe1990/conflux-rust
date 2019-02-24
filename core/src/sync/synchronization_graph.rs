use crate::{
    cache_manager::CacheManager,
    consensus::SharedConsensusGraph,
    db::COL_MISC,
    error::{BlockError, Error, ErrorKind},
    hash::{KECCAK_EMPTY_LIST_RLP, KECCAK_NULL_RLP},
    machine::new_machine,
    pow::ProofOfWorkConfig,
    verification::*,
};
use ethereum_types::{H256, U256};
use heapsize::HeapSizeOf;
use parking_lot::{Mutex, RwLock};
use primitives::{block::CompactBlock, Block, BlockHeader};
use rlp::Rlp;
use slab::Slab;
use std::{
    cmp::{max, min},
    collections::{HashMap, HashSet, VecDeque},
    ops::DerefMut,
    sync::Arc,
};
use unexpected::{Mismatch, OutOfBounds};

const NULL: usize = !0;
const BLOCK_INVALID: u8 = 0;
const BLOCK_HEADER_ONLY: u8 = 1;
const BLOCK_HEADER_PARENTAL_TREE_READY: u8 = 2;
#[allow(dead_code)]
const BLOCK_HEADER_GRAPH_READY: u8 = 3;
const BLOCK_GRAPH_READY: u8 = 4;

pub struct BestInformation {
    pub best_block_hash: H256,
    pub current_difficulty: U256,
    pub terminal_block_hashes: Vec<H256>,
    pub deferred_state_root: H256,
    pub deferred_receipts_root: H256,
}

pub struct SynchronizationGraphNode {
    pub block_header: Arc<BlockHeader>,
    /// The status of graph connectivity in the current block view.
    pub graph_status: u8,
    /// Whether the block body is ready.
    pub block_ready: bool,
    /// The index of the parent of the block.
    pub parent: usize,
    /// The indices of the children of the block.
    pub children: Vec<usize>,
    /// The indices of the blocks referenced by the block.
    pub referees: Vec<usize>,
    /// The number of blocks referenced by the block but
    /// haven't been inserted in synchronization graph.
    pub pending_referee_count: usize,
    /// The indices of the blocks referencing the block.
    pub referrers: Vec<usize>,
    /// The indices set of the blocks in the epoch when the current
    /// block is as pivot chain block. This set does not contain
    /// the block itself.
    pub blockset_in_own_view_of_epoch: HashSet<usize>,
    /// The minimum epoch number of the block in the view of other
    /// blocks including itself.
    pub min_epoch_in_other_views: u64,
}

pub struct SynchronizationGraphInner {
    pub arena: Slab<SynchronizationGraphNode>,
    pub indices: HashMap<H256, usize>,
    genesis_block_index: usize,
    children_by_hash: HashMap<H256, Vec<usize>>,
    referrers_by_hash: HashMap<H256, Vec<usize>>,
    pow_config: ProofOfWorkConfig,
    current_difficulty: U256,
    best_block_hash: H256,
    pub terminal_block_hashes: HashSet<H256>,
    pub deferred_state_root: H256,
    pub deferred_receipts_root: H256,
}

impl SynchronizationGraphInner {
    pub fn with_genesis_block(
        genesis_header: Arc<BlockHeader>, pow_config: ProofOfWorkConfig,
    ) -> Self {
        let mut inner = SynchronizationGraphInner {
            arena: Slab::new(),
            indices: HashMap::new(),
            genesis_block_index: NULL,
            children_by_hash: HashMap::new(),
            referrers_by_hash: HashMap::new(),
            pow_config,
            current_difficulty: pow_config.initial_difficulty.into(),
            best_block_hash: genesis_header.hash(),
            terminal_block_hashes: HashSet::new(),
            deferred_state_root: KECCAK_NULL_RLP,
            deferred_receipts_root: KECCAK_EMPTY_LIST_RLP,
        };
        inner.deferred_state_root =
            genesis_header.deferred_state_root().clone();
        inner.genesis_block_index = inner.insert(genesis_header);
        inner.terminal_block_hashes.insert(inner.best_block_hash);
        debug!(
            "genesis_block_index in sync graph: {}",
            inner.genesis_block_index
        );

        inner
    }

    pub fn terminal_block_hashes(&self) -> Vec<H256> {
        self.terminal_block_hashes
            .iter()
            .map(|hash| hash.clone())
            .collect()
    }

    pub fn insert_invalid(&mut self, header: Arc<BlockHeader>) -> usize {
        let hash = header.hash();
        let me = self.arena.insert(SynchronizationGraphNode {
            graph_status: BLOCK_INVALID,
            block_ready: false,
            parent: NULL,
            children: Vec::new(),
            referees: Vec::new(),
            pending_referee_count: 0,
            referrers: Vec::new(),
            blockset_in_own_view_of_epoch: HashSet::new(),
            min_epoch_in_other_views: header.height(),
            block_header: header,
        });
        self.indices.insert(hash, me);

        if let Some(children) = self.children_by_hash.remove(&hash) {
            for child in &children {
                self.arena[*child].parent = me;
            }
            self.arena[me].children = children;
        }
        if let Some(referrers) = self.referrers_by_hash.remove(&hash) {
            for referrer in &referrers {
                let ref mut node_referrer = self.arena[*referrer];
                node_referrer.referees.push(me);
                debug_assert!(node_referrer.pending_referee_count > 0);
                if node_referrer.pending_referee_count > 0 {
                    node_referrer.pending_referee_count =
                        node_referrer.pending_referee_count - 1;
                }
            }
            self.arena[me].referrers = referrers;
        }

        me
    }

    /// Return the index of the inserted block.
    pub fn insert(&mut self, header: Arc<BlockHeader>) -> usize {
        let hash = header.hash();
        let me = self.arena.insert(SynchronizationGraphNode {
            graph_status: if *header.parent_hash() == H256::default() {
                BLOCK_GRAPH_READY
            } else {
                BLOCK_HEADER_ONLY
            },
            block_ready: *header.parent_hash() == H256::default(),
            parent: NULL,
            children: Vec::new(),
            referees: Vec::new(),
            pending_referee_count: 0,
            referrers: Vec::new(),
            blockset_in_own_view_of_epoch: HashSet::new(),
            min_epoch_in_other_views: header.height(),
            block_header: header.clone(),
        });
        self.indices.insert(hash, me);

        let parent_hash = header.parent_hash().clone();
        if parent_hash != H256::default() {
            if let Some(parent) = self.indices.get(&parent_hash).cloned() {
                self.arena[me].parent = parent;
                self.arena[parent].children.push(me);
            } else {
                self.children_by_hash
                    .entry(parent_hash)
                    .or_insert(Vec::new())
                    .push(me);
            }
        }
        for referee_hash in header.referee_hashes() {
            if let Some(referee) = self.indices.get(referee_hash).cloned() {
                self.arena[me].referees.push(referee);
                self.arena[referee].referrers.push(me);
            } else {
                self.arena[me].pending_referee_count =
                    self.arena[me].pending_referee_count + 1;
                self.referrers_by_hash
                    .entry(*referee_hash)
                    .or_insert(Vec::new())
                    .push(me);
            }
        }

        if let Some(children) = self.children_by_hash.remove(&hash) {
            for child in &children {
                self.arena[*child].parent = me;
            }
            self.arena[me].children = children;
        }
        if let Some(referrers) = self.referrers_by_hash.remove(&hash) {
            for referrer in &referrers {
                let ref mut node_referrer = self.arena[*referrer];
                node_referrer.referees.push(me);
                debug_assert!(node_referrer.pending_referee_count > 0);
                if node_referrer.pending_referee_count > 0 {
                    node_referrer.pending_referee_count =
                        node_referrer.pending_referee_count - 1;
                }
            }
            self.arena[me].referrers = referrers;
        }

        me
    }

    pub fn new_to_be_header_parental_tree_ready(&self, index: usize) -> bool {
        let ref node_me = self.arena[index];
        if node_me.graph_status >= BLOCK_HEADER_PARENTAL_TREE_READY {
            return false;
        }

        let parent = node_me.parent;
        parent != NULL
            && self.arena[parent].graph_status
                >= BLOCK_HEADER_PARENTAL_TREE_READY
    }

    pub fn new_to_be_header_graph_ready(&self, index: usize) -> bool {
        let ref node_me = self.arena[index];
        if node_me.graph_status >= BLOCK_HEADER_GRAPH_READY {
            return false;
        }

        if node_me.pending_referee_count > 0 {
            return false;
        }

        let parent = node_me.parent;
        parent != NULL
            && self.arena[parent].graph_status >= BLOCK_HEADER_GRAPH_READY
            && !node_me.referees.iter().any(|&referee| {
                self.arena[referee].graph_status < BLOCK_HEADER_GRAPH_READY
            })
    }

    pub fn new_to_be_block_graph_ready(&self, index: usize) -> bool {
        let ref node_me = self.arena[index];
        if !node_me.block_ready {
            return false;
        }

        if node_me.graph_status >= BLOCK_GRAPH_READY {
            return false;
        }

        let parent = node_me.parent;
        node_me.graph_status >= BLOCK_HEADER_GRAPH_READY
            && parent != NULL
            && self.arena[parent].graph_status >= BLOCK_GRAPH_READY
            && !node_me.referees.iter().any(|&referee| {
                self.arena[referee].graph_status < BLOCK_GRAPH_READY
            })
    }

    fn collect_blockset_in_own_view_of_epoch(&mut self, pivot: usize) {
        let mut queue = VecDeque::new();
        for referee in &self.arena[pivot].referees {
            queue.push_back(*referee);
        }

        let mut visited = HashSet::new();
        while let Some(index) = queue.pop_front() {
            visited.insert(index);
            let mut in_old_epoch = false;
            let mut cur_pivot = pivot;
            loop {
                let parent = self.arena[cur_pivot].parent;
                debug_assert!(parent != NULL);
                if self.arena[parent].block_header.height()
                    < self.arena[index].min_epoch_in_other_views
                {
                    break;
                }
                if parent == index
                    || self.arena[parent]
                        .blockset_in_own_view_of_epoch
                        .contains(&index)
                {
                    in_old_epoch = true;
                    break;
                }
                cur_pivot = parent;
            }

            if !in_old_epoch {
                let parent = self.arena[index].parent;
                if !visited.contains(&parent) {
                    queue.push_back(parent);
                }
                for referee in &self.arena[index].referees {
                    if !visited.contains(referee) {
                        queue.push_back(*referee);
                    }
                }
                self.arena[index].min_epoch_in_other_views = min(
                    self.arena[index].min_epoch_in_other_views,
                    self.arena[pivot].block_header.height(),
                );
                self.arena[pivot]
                    .blockset_in_own_view_of_epoch
                    .insert(index);
            }
        }
    }

    fn target_difficulty(&self, cur_index: usize) -> U256 {
        let epoch = self.arena[cur_index].block_header.height();
        assert_ne!(epoch, 0);
        debug_assert!(
            epoch
                == (epoch / self.pow_config.difficulty_adjustment_epoch_period)
                    * self.pow_config.difficulty_adjustment_epoch_period
        );

        let mut cur = cur_index;
        let cur_difficulty = *self.arena[cur].block_header.difficulty();
        let mut block_count = 0 as u64;
        let mut max_time = u64::min_value();
        let mut min_time = u64::max_value();
        for _ in 0..self.pow_config.difficulty_adjustment_epoch_period {
            block_count = block_count
                + self.arena[cur].blockset_in_own_view_of_epoch.len() as u64
                + 1;
            max_time = max(max_time, self.arena[cur].block_header.timestamp());
            min_time = min(min_time, self.arena[cur].block_header.timestamp());
            cur = self.arena[cur].parent;
        }
        self.pow_config.target_difficulty(
            block_count,
            max_time - min_time,
            &cur_difficulty,
        )
    }

    fn verify_header_graph_ready_block(
        &self, index: usize,
    ) -> Result<(), Error> {
        let epoch = self.arena[index].block_header.height();
        let parent = self.arena[index].parent;
        if self.arena[parent].block_header.height() + 1 != epoch {
            warn!(
                "Invalid height. mine {}, parent {}",
                epoch,
                self.arena[parent].block_header.height()
            );
            return Err(From::from(BlockError::InvalidHeight(Mismatch {
                expected: self.arena[parent].block_header.height() + 1,
                found: epoch,
            })));
        }

        let machine = new_machine();
        let gas_limit_divisor = machine.params().gas_limit_bound_divisor;
        let min_gas_limit = machine.params().min_gas_limit;
        let parent_gas_limit = *self.arena[parent].block_header.gas_limit();
        let gas_lower = max(
            parent_gas_limit - parent_gas_limit / gas_limit_divisor,
            min_gas_limit,
        );
        let gas_upper = parent_gas_limit + parent_gas_limit / gas_limit_divisor;
        let self_gas_limit = *self.arena[index].block_header.gas_limit();
        if self_gas_limit <= gas_lower || self_gas_limit >= gas_upper {
            return Err(From::from(BlockError::InvalidGasLimit(OutOfBounds {
                min: Some(gas_lower),
                max: Some(gas_upper),
                found: self_gas_limit,
            })));
        }

        let expected_difficulty: U256 = if epoch
            <= self.pow_config.difficulty_adjustment_epoch_period
        {
            self.pow_config.initial_difficulty.into()
        } else {
            let mut last_period_upper = (epoch
                / self.pow_config.difficulty_adjustment_epoch_period)
                * self.pow_config.difficulty_adjustment_epoch_period;
            if last_period_upper == epoch {
                last_period_upper = last_period_upper
                    - self.pow_config.difficulty_adjustment_epoch_period;
            }
            let mut cur = index;
            while self.arena[cur].block_header.height() > last_period_upper {
                cur = self.arena[cur].parent;
            }
            self.target_difficulty(cur)
        };

        if expected_difficulty != *self.arena[index].block_header.difficulty() {
            warn!(
                "expected_difficulty {}; difficulty {}",
                expected_difficulty,
                *self.arena[index].block_header.difficulty()
            );
            return Err(From::from(BlockError::InvalidDifficulty(Mismatch {
                expected: expected_difficulty,
                found: self.arena[index].block_header.difficulty().clone(),
            })));
        }

        Ok(())
    }

    pub fn adjust_difficulty(&mut self, new_best_hash: &H256) {
        let old_best_index = *self.indices.get(&self.best_block_hash).unwrap();
        let new_best_index = *self.indices.get(new_best_hash).unwrap();
        if self.arena[old_best_index].block_header.hash()
            == *self.arena[new_best_index].block_header.parent_hash()
        {
            // Pivot chain prolonged
            assert!(
                self.current_difficulty
                    == *self.arena[new_best_index].block_header.difficulty()
            );
        }

        let epoch = self.arena[new_best_index].block_header.height();
        if epoch == 0 {
            // This may happen since the block at height 1 may have wrong
            // state root and do not update the pivot chain.
            self.current_difficulty = self.pow_config.initial_difficulty.into();
        } else if epoch
            == (epoch / self.pow_config.difficulty_adjustment_epoch_period)
                * self.pow_config.difficulty_adjustment_epoch_period
        {
            self.current_difficulty = self.target_difficulty(new_best_index);
        } else {
            self.current_difficulty =
                *self.arena[new_best_index].block_header.difficulty();
        }
    }

    pub fn is_in_past(&self, index: usize, pivot: usize) -> bool {
        let mut cur_pivot = pivot;
        loop {
            debug_assert!(cur_pivot != NULL);
            if self.arena[cur_pivot].block_header.height()
                < self.arena[index].min_epoch_in_other_views
            {
                break;
            }
            if cur_pivot == index
                || self.arena[cur_pivot]
                    .blockset_in_own_view_of_epoch
                    .contains(&index)
            {
                return true;
            }
            cur_pivot = self.arena[cur_pivot].parent;
        }
        false
    }
}

pub struct SynchronizationGraph {
    pub inner: RwLock<SynchronizationGraphInner>,
    pub block_headers: Arc<RwLock<HashMap<H256, Arc<BlockHeader>>>>,
    pub compact_blocks: RwLock<HashMap<H256, CompactBlock>>,
    pub blocks: Arc<RwLock<HashMap<H256, Arc<Block>>>>,
    genesis_block_hash: H256,
    pub initial_missed_block_hashes: Mutex<HashSet<H256>>,
    pub consensus: SharedConsensusGraph,
    pub verification_config: VerificationConfig,
    pub cache_man: Arc<Mutex<CacheManager<CacheId>>>,
}

pub type SharedSynchronizationGraph = Arc<SynchronizationGraph>;

impl SynchronizationGraph {
    pub fn new(
        consensus: SharedConsensusGraph, pow_config: ProofOfWorkConfig,
        verification_config: VerificationConfig,
    ) -> Self
    {
        let genesis_block_hash = consensus.genesis_block().hash();
        let genesis_block_header = consensus
            .block_headers
            .read()
            .get(&genesis_block_hash)
            .expect("genesis exists")
            .clone();
        let mut sync_graph = SynchronizationGraph {
            inner: RwLock::new(SynchronizationGraphInner::with_genesis_block(
                genesis_block_header,
                pow_config,
            )),
            compact_blocks: RwLock::new(HashMap::new()),
            blocks: consensus.blocks.clone(),
            block_headers: consensus.block_headers.clone(),
            genesis_block_hash,
            initial_missed_block_hashes: Mutex::new(HashSet::new()),
            verification_config,
            cache_man: consensus.cache_man.clone(),
            consensus,
        };

        sync_graph.recover_graph_from_db();
        sync_graph
    }

    fn recover_graph_from_db(&mut self) {
        let terminals = match self.consensus.db.key_value().get(COL_MISC, b"terminals")
            .expect("Low-level database error when fetching 'terminals' block. Some issue with disk?")
            {
                Some(terminals) => {
                    let rlp = Rlp::new(&terminals);
                    rlp.as_list::<H256>().expect("Failed to decode terminals!")
                }
                None => {
                    return;
                }
            };

        let mut queue = VecDeque::new();
        for terminal in terminals {
            queue.push_back(terminal);
        }

        let mut missed_hashes = self.initial_missed_block_hashes.lock();
        while let Some(hash) = queue.pop_front() {
            if hash == self.genesis_block_hash {
                continue;
            }

            if let Some(block) = self.block_by_hash_from_db(&hash) {
                // This is for constructing synchronization graph.
                let res =
                    self.insert_block_header(block.block_header.clone(), true);
                assert!(res.0);

                let parent = block.block_header.parent_hash().clone();
                let referees = block.block_header.referee_hashes().clone();

                // This is necessary to construct consensus graph.
                self.insert_block(block, true, false);

                if !self.contains_block(&parent) {
                    queue.push_back(parent);
                }

                for referee in referees {
                    if !self.contains_block(&referee) {
                        queue.push_back(referee);
                    }
                }
            } else {
                missed_hashes.insert(hash);
            }
        }
    }

    pub fn block_header_by_hash(&self, hash: &H256) -> Option<BlockHeader> {
        self.consensus
            .block_header_by_hash(hash)
            .map(|header| (*header).clone())
    }

    pub fn block_height_by_hash(&self, hash: &H256) -> Option<u64> {
        self.consensus.block_height_by_hash(hash)
    }

    pub fn block_by_hash(&self, hash: &H256) -> Option<Arc<Block>> {
        self.consensus.block_by_hash(hash)
    }

    pub fn block_by_hash_from_db(&self, hash: &H256) -> Option<Block> {
        self.consensus.block_by_hash_from_db(hash)
    }

    pub fn compact_block_by_hash(&self, hash: &H256) -> Option<CompactBlock> {
        self.compact_blocks.read().get(hash).map(|b| {
            self.cache_man
                .lock()
                .note_used(CacheId::CompactBlock(b.hash()));
            b.clone()
        })
    }

    pub fn genesis_hash(&self) -> &H256 { &self.genesis_block_hash }

    pub fn contains_block_header(&self, hash: &H256) -> bool {
        self.inner.read().indices.contains_key(hash)
    }

    pub fn contains_compact_block(&self, hash: &H256) -> bool {
        self.compact_blocks.read().contains_key(hash)
    }

    pub fn insert_compact_block(&self, cb: CompactBlock) {
        self.cache_man
            .lock()
            .note_used(CacheId::CompactBlock(cb.hash()));
        self.compact_blocks.write().insert(cb.hash(), cb);
    }

    fn parent_or_referees_invalid(&self, header: &BlockHeader) -> bool {
        self.consensus.verified_invalid(header.parent_hash())
            || header
                .referee_hashes()
                .iter()
                .any(|referee| self.consensus.verified_invalid(referee))
    }

    fn set_and_propagate_invalid(
        inner: &mut SynchronizationGraphInner, queue: &mut VecDeque<usize>,
        invalid_set: &mut HashSet<usize>, index: usize,
    )
    {
        if !invalid_set.contains(&index) {
            invalid_set.insert(index);
            let children: Vec<usize> =
                inner.arena[index].children.iter().map(|x| *x).collect();
            for child in children {
                inner.arena[child].graph_status = BLOCK_INVALID;
                queue.push_back(child);
            }
            let referrers: Vec<usize> =
                inner.arena[index].referrers.iter().map(|x| *x).collect();
            for referrer in referrers {
                inner.arena[referrer].graph_status = BLOCK_INVALID;
                queue.push_back(referrer);
            }
        }
    }

    fn process_invalid_blocks(
        &self, inner: &mut SynchronizationGraphInner,
        invalid_set: &HashSet<usize>,
    )
    {
        for index in invalid_set {
            let hash = inner.arena[*index].block_header.hash();
            self.consensus.invalidate_block(&hash);

            let parent = inner.arena[*index].parent;
            if parent != NULL {
                inner.arena[parent].children.retain(|&x| x != *index);
            }
            let parent_hash = *inner.arena[*index].block_header.parent_hash();
            if let Some(children) = inner.children_by_hash.get_mut(&parent_hash)
            {
                children.retain(|&x| x != *index);
            }

            let referees: Vec<usize> =
                inner.arena[*index].referees.iter().map(|x| *x).collect();
            for referee in referees {
                inner.arena[referee].referrers.retain(|&x| x != *index);
            }
            let referee_hashes: Vec<H256> = inner.arena[*index]
                .block_header
                .referee_hashes()
                .iter()
                .map(|x| *x)
                .collect();
            for referee_hash in referee_hashes {
                if let Some(referrers) =
                    inner.referrers_by_hash.get_mut(&referee_hash)
                {
                    referrers.retain(|&x| x != *index);
                }
            }

            let children: Vec<usize> =
                inner.arena[*index].children.iter().map(|x| *x).collect();
            for child in children {
                debug_assert!(invalid_set.contains(&child));
                debug_assert!(inner.arena[child].graph_status == BLOCK_INVALID);
                inner.arena[child].parent = NULL;
            }

            let referrers: Vec<usize> =
                inner.arena[*index].referrers.iter().map(|x| *x).collect();
            for referrer in referrers {
                debug_assert!(invalid_set.contains(&referrer));
                debug_assert!(
                    inner.arena[referrer].graph_status == BLOCK_INVALID
                );
                inner.arena[referrer].referees.retain(|&x| x != *index);
            }

            inner.arena.remove(*index);
            inner.indices.remove(&hash);
            self.block_headers.write().remove(&hash);
            self.remove_block_from_kv(&hash);
        }
    }

    pub fn insert_block_header(
        &self, header: BlockHeader, need_to_verify: bool,
    ) -> (bool, Vec<H256>) {
        let mut inner = self.inner.write();
        let hash = header.hash();

        if self.verified_invalid(&hash) {
            return (false, Vec::new());
        }

        if inner.indices.contains_key(&hash) {
            return (true, Vec::new());
        }

        let header_arc = Arc::new(header);
        let me = if need_to_verify {
            if self.parent_or_referees_invalid(&*header_arc)
                || self
                    .verification_config
                    .verify_header_params(&*header_arc)
                    .is_err()
            {
                inner.insert_invalid(header_arc.clone())
            } else {
                inner.insert(header_arc.clone())
            }
        } else {
            inner.insert(header_arc.clone())
        };

        // Start to pass influence to descendants
        let mut need_to_relay: Vec<H256> = Vec::new();
        let mut me_invalid = false;
        let mut invalid_set: HashSet<usize> = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(me);
        while let Some(index) = queue.pop_front() {
            if inner.arena[index].graph_status == BLOCK_INVALID {
                if me == index {
                    me_invalid = true;
                }
                Self::set_and_propagate_invalid(
                    inner.deref_mut(),
                    &mut queue,
                    &mut invalid_set,
                    index,
                );
            } else if inner.new_to_be_header_graph_ready(index) {
                inner.arena[index].graph_status = BLOCK_HEADER_GRAPH_READY;
                debug_assert!(inner.arena[index].parent != NULL);

                let r = inner.verify_header_graph_ready_block(index);
                if need_to_verify && r.is_err() {
                    warn!(
                        "Invalid header_arc! inserted_header={:?} err={:?}",
                        header_arc, r
                    );
                    if me == index {
                        me_invalid = true;
                    }
                    inner.arena[index].graph_status = BLOCK_INVALID;
                    Self::set_and_propagate_invalid(
                        inner.deref_mut(),
                        &mut queue,
                        &mut invalid_set,
                        index,
                    );
                    continue;
                }

                // Passed verification on header_arc.
                if inner.arena[index].block_ready {
                    need_to_relay.push(inner.arena[index].block_header.hash());
                }

                inner.collect_blockset_in_own_view_of_epoch(index);

                for child in &inner.arena[index].children {
                    debug_assert!(
                        inner.arena[*child].graph_status
                            < BLOCK_HEADER_GRAPH_READY
                    );
                    queue.push_back(*child);
                }
                for referrer in &inner.arena[index].referrers {
                    debug_assert!(
                        inner.arena[*referrer].graph_status
                            < BLOCK_HEADER_GRAPH_READY
                    );
                    queue.push_back(*referrer);
                }
            } else if inner.new_to_be_header_parental_tree_ready(index) {
                inner.arena[index].graph_status =
                    BLOCK_HEADER_PARENTAL_TREE_READY;
                for child in &inner.arena[index].children {
                    debug_assert!(
                        inner.arena[*child].graph_status
                            < BLOCK_HEADER_PARENTAL_TREE_READY
                    );
                    queue.push_back(*child);
                }
            }
        }

        // Post-processing invalid blocks.
        self.process_invalid_blocks(inner.deref_mut(), &invalid_set);

        if me_invalid {
            return (false, need_to_relay);
        }

        self.block_headers
            .write()
            .insert(header_arc.hash(), header_arc);
        (true, need_to_relay)
    }

    pub fn contains_block(&self, hash: &H256) -> bool {
        let inner = self.inner.read();
        if let Some(index) = inner.indices.get(hash) {
            inner.arena[*index].block_ready
        } else {
            false
        }
    }

    pub fn insert_block_to_kv(&self, block: Arc<Block>, persistent: bool) {
        self.consensus.insert_block_to_kv(block, persistent)
    }

    fn remove_block_from_kv(&self, hash: &H256) {
        self.consensus.remove_block_from_kv(hash)
    }

    pub fn insert_block(
        &self, block: Block, need_to_verify: bool, persistent: bool,
    ) -> (bool, bool) {
        let mut insert_success = true;
        let mut need_to_relay = false;

        let block = Arc::new(block);

        let hash = block.hash();

        let mut inner = self.inner.write();

        if self.verified_invalid(&hash) {
            insert_success = false;
            // (false, false)
            return (insert_success, need_to_relay);
        }

        let contains_block = if let Some(index) = inner.indices.get(&hash) {
            inner.arena[*index].block_ready
        } else {
            false
        };

        if contains_block {
            // (true, false)
            return (insert_success, need_to_relay);
        }

        let me = *inner.indices.get(&hash).unwrap();
        debug_assert!(hash == inner.arena[me].block_header.hash());
        debug_assert!(!inner.arena[me].block_ready);
        inner.arena[me].block_ready = true;

        if need_to_verify {
            let r = self.verification_config.verify_block_basic(&block);
            match r {
                Err(Error(
                    ErrorKind::Block(BlockError::InvalidTransactionsRoot(e)),
                    _,
                )) => {
                    warn ! ("BlockTransactionRoot not match! inserted_block={:?} err={:?}", block, e);
                    insert_success = false;
                    return (insert_success, need_to_relay);
                }
                Err(e) => {
                    warn!(
                        "Invalid block! inserted_block={:?} err={:?}",
                        block, e
                    );
                    inner.arena[me].graph_status = BLOCK_INVALID;
                }
                _ => {}
            };
        }

        let mut invalid_set: HashSet<usize> = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(me);

        if inner.arena[me].graph_status != BLOCK_INVALID {
            // Here we always build a new compact block because we should not
            // reuse the nonce
            self.insert_compact_block(block.to_compact());
            self.insert_block_to_kv(block, persistent);
        } else {
            insert_success = false;
        }

        while let Some(index) = queue.pop_front() {
            if inner.arena[index].graph_status == BLOCK_INVALID {
                Self::set_and_propagate_invalid(
                    inner.deref_mut(),
                    &mut queue,
                    &mut invalid_set,
                    index,
                );
            } else if inner.new_to_be_block_graph_ready(index) {
                inner.arena[index].graph_status = BLOCK_GRAPH_READY;
                let h = inner.arena[index].block_header.hash();
                let (
                    new_best_hash,
                    deferred_state_root,
                    deferred_receipts_root,
                ) = self.consensus.on_new_block(&h, inner.deref_mut());
                inner.adjust_difficulty(&new_best_hash);
                inner.best_block_hash = new_best_hash;
                inner.deferred_state_root = deferred_state_root;
                inner.deferred_receipts_root = deferred_receipts_root;

                for child in &inner.arena[index].children {
                    debug_assert!(
                        inner.arena[*child].graph_status < BLOCK_GRAPH_READY
                    );
                    queue.push_back(*child);
                }
                for referrer in &inner.arena[index].referrers {
                    debug_assert!(
                        inner.arena[*referrer].graph_status < BLOCK_GRAPH_READY
                    );
                    queue.push_back(*referrer);
                }
            }
        }
        if inner.arena[me].graph_status >= BLOCK_HEADER_GRAPH_READY {
            need_to_relay = true;
        }

        // Post-processing invalid blocks.
        self.process_invalid_blocks(inner.deref_mut(), &invalid_set);
        if self.consensus.db.key_value().flush().is_err() {
            warn!("db error when flushing block data");
            insert_success = false;
        }

        (insert_success, need_to_relay)
    }

    pub fn get_best_info(&self) -> BestInformation {
        let inner = self.inner.read();
        BestInformation {
            best_block_hash: inner.best_block_hash.clone(),
            current_difficulty: inner.current_difficulty,
            terminal_block_hashes: inner.terminal_block_hashes(),
            deferred_state_root: inner.deferred_state_root.clone(),
            deferred_receipts_root: inner.deferred_receipts_root.clone(),
        }
    }

    pub fn verified_invalid(&self, hash: &H256) -> bool {
        self.consensus.verified_invalid(hash)
    }

    /// Get current cache size.
    pub fn cache_size(&self) -> CacheSize {
        CacheSize {
            blocks: self.blocks.read().heap_size_of_children(),
            block_receipts: self
                .consensus
                .block_receipts
                .read()
                .heap_size_of_children(),
            transaction_addresses: self
                .consensus
                .transaction_addresses
                .read()
                .heap_size_of_children()
                + self
                    .consensus
                    .txpool
                    .unexecuted_transaction_addresses
                    .lock()
                    .heap_size_of_children(),
            compact_blocks: self.compact_blocks.read().heap_size_of_children(),
        }
    }

    pub fn block_cache_gc(&self) {
        let current_size = self.cache_size().total();
        debug!("Before gc cache_size={}", current_size);
        let mut blocks = self.blocks.write();
        let mut block_receipts = self.consensus.block_receipts.write();
        let mut transaction_addresses =
            self.consensus.transaction_addresses.write();
        let mut unexecuted_transaction_addresses = self
            .consensus
            .txpool
            .unexecuted_transaction_addresses
            .lock();
        let mut compact_blocks = self.compact_blocks.write();

        let mut cache_man = self.cache_man.lock();
        cache_man.collect_garbage(current_size, |ids| {
            for id in &ids {
                match *id {
                    CacheId::Block(ref h) => {
                        blocks.remove(h);
                    }
                    CacheId::BlockReceipts(ref h) => {
                        block_receipts.remove(h);
                    }
                    CacheId::TransactionAddress(ref h) => {
                        transaction_addresses.remove(h);
                    }
                    CacheId::UnexecutedTransactionAddress(ref h) => {
                        unexecuted_transaction_addresses.remove(h);
                    }
                    CacheId::CompactBlock(ref h) => {
                        compact_blocks.remove(h);
                    }
                }
            }

            blocks.shrink_to_fit();
            block_receipts.shrink_to_fit();
            transaction_addresses.shrink_to_fit();
            unexecuted_transaction_addresses.shrink_to_fit();
            compact_blocks.shrink_to_fit();

            blocks.heap_size_of_children()
                + block_receipts.heap_size_of_children()
                + transaction_addresses.heap_size_of_children()
                + unexecuted_transaction_addresses.heap_size_of_children()
                + compact_blocks.heap_size_of_children()
        });
    }

    pub fn persist_terminals(&self) { self.consensus.persist_terminals(); }
}

#[derive(Debug, Hash, Eq, PartialEq, Clone)]
pub enum CacheId {
    Block(H256),
    BlockReceipts(H256),
    TransactionAddress(H256),
    UnexecutedTransactionAddress(H256),
    CompactBlock(H256),
}

#[derive(Debug)]
pub struct CacheSize {
    /// Blocks cache size.
    pub blocks: usize,
    /// Block Receipts cache size.
    pub block_receipts: usize,
    /// Transaction Addresses cache size.
    pub transaction_addresses: usize,
    /// Compact blocks cache size.
    pub compact_blocks: usize,
}

impl CacheSize {
    /// Total amount used by the cache.
    pub fn total(&self) -> usize {
        self.blocks
            + self.block_receipts
            + self.transaction_addresses
            + self.compact_blocks
    }
}

use consensus::SharedConsensusGraph;
use error::{BlockError, Error};
use ethereum_types::{H256, U256};
use parking_lot::RwLock;
use pow::{
    target_difficulty, DIFFICULTY_ADJUSTMENT_EPOCH_PERIOD, INITIAL_DIFFICULTY,
};
use primitives::{Block, BlockHeader};
use slab::Slab;
use std::{
    cmp::{max, min},
    collections::{HashMap, HashSet, VecDeque},
    time::SystemTime,
};
use unexpected::Mismatch;

const NULL: usize = !0;

const BLOCK_HEADER_ONLY: u8 = 0;
const BLOCK_HEADER_PARENTAL_TREE_READY: u8 = 1;
#[allow(dead_code)]
const BLOCK_HEADER_GRAPH_READY: u8 = 2;
const BLOCK_GRAPH_READY: u8 = 3;

pub struct SynchronizationGraphNode {
    /// The hash of the block header.
    pub hash: H256,
    /// The status of graph connectivity in the current block view.
    pub graph_status: u8,
    /// Whether the block body is ready.
    pub block_ready: bool,
    /// The height of the block.
    pub block_height: u64,
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
    /// The difficulty of the block.
    /// Used for verification.
    pub difficulty: U256,
    /// The timestamp of the block generation.
    /// Used for verification.
    pub timestamp: u64,
}

pub struct SynchronizationGraphInner {
    pub arena: Slab<SynchronizationGraphNode>,
    pub indices: HashMap<H256, usize>,
    genesis_block_index: usize,
    children_by_hash: HashMap<H256, Vec<usize>>,
    referrers_by_hash: HashMap<H256, Vec<usize>>,
}

impl SynchronizationGraphInner {
    pub fn with_genesis_block(genesis_block: &Block) -> Self {
        let mut inner = SynchronizationGraphInner {
            arena: Slab::new(),
            indices: HashMap::new(),
            genesis_block_index: NULL,
            children_by_hash: HashMap::new(),
            referrers_by_hash: HashMap::new(),
        };
        inner.genesis_block_index = inner.insert(&genesis_block.block_header);

        inner
    }

    /// Return the index of the inserted block.
    pub fn insert(&mut self, header: &BlockHeader) -> usize {
        let hash = header.hash();
        let me = self.arena.insert(SynchronizationGraphNode {
            hash,
            graph_status: if *header.parent_hash() == H256::default() {
                BLOCK_GRAPH_READY
            } else {
                BLOCK_HEADER_ONLY
            },
            block_ready: *header.parent_hash() == H256::default(),
            block_height: header.height(),
            parent: NULL,
            children: Vec::new(),
            referees: Vec::new(),
            pending_referee_count: 0,
            referrers: Vec::new(),
            blockset_in_own_view_of_epoch: HashSet::new(),
            min_epoch_in_other_views: header.height(),
            difficulty: *header.difficulty(),
            timestamp: header.timestamp(),
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

        // Start to pass influence to descendants
        let mut queue = VecDeque::new();
        queue.push_back(me);
        while let Some(index) = queue.pop_front() {
            if self.new_to_be_header_graph_ready(index) {
                self.arena[index].graph_status = BLOCK_HEADER_GRAPH_READY;
                debug_assert!(self.arena[index].parent != NULL);
                self.collect_blockset_in_own_view_of_epoch(index);
                self.verify_header_graph_ready_block(index);

                for child in &self.arena[index].children {
                    debug_assert!(
                        self.arena[*child].graph_status
                            < BLOCK_HEADER_GRAPH_READY
                    );
                    queue.push_back(*child);
                }
                for referrer in &self.arena[index].referrers {
                    debug_assert!(
                        self.arena[*referrer].graph_status
                            < BLOCK_HEADER_GRAPH_READY
                    );
                    queue.push_back(*referrer);
                }
            } else if self.new_to_be_header_parental_tree_ready(index) {
                self.arena[index].graph_status =
                    BLOCK_HEADER_PARENTAL_TREE_READY;
                for child in &self.arena[index].children {
                    debug_assert!(
                        self.arena[*child].graph_status
                            < BLOCK_HEADER_PARENTAL_TREE_READY
                    );
                    queue.push_back(*child);
                }
            }
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
                if self.arena[parent].block_height
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
                    self.arena[pivot].block_height,
                );
                self.arena[pivot]
                    .blockset_in_own_view_of_epoch
                    .insert(index);
            }
        }
    }

    fn verify_header_graph_ready_block(
        &self, index: usize,
    ) -> Result<(), Error> {
        let epoch = self.arena[index].block_height;
        let expected_difficulty: U256 = if epoch
            <= DIFFICULTY_ADJUSTMENT_EPOCH_PERIOD
        {
            INITIAL_DIFFICULTY.into()
        } else {
            let mut last_period_upper = (epoch
                / DIFFICULTY_ADJUSTMENT_EPOCH_PERIOD)
                * DIFFICULTY_ADJUSTMENT_EPOCH_PERIOD;
            if last_period_upper == epoch {
                last_period_upper =
                    last_period_upper - DIFFICULTY_ADJUSTMENT_EPOCH_PERIOD;
            }
            let mut cur = index;
            while self.arena[cur].block_height > last_period_upper {
                cur = self.arena[cur].parent;
            }
            let cur_difficulty = self.arena[cur].difficulty;
            let mut block_count = 0 as u64;
            let mut max_time = u64::min_value();
            let mut min_time = u64::max_value();
            for i in 0..DIFFICULTY_ADJUSTMENT_EPOCH_PERIOD {
                block_count = block_count
                    + self.arena[cur].blockset_in_own_view_of_epoch.len()
                        as u64
                    + 1;
                max_time = max(max_time, self.arena[cur].timestamp);
                min_time = min(min_time, self.arena[cur].timestamp);
                cur = self.arena[cur].parent;
            }
            target_difficulty(block_count, max_time - min_time, &cur_difficulty)
        };

        if expected_difficulty != self.arena[index].difficulty {
            return Err(From::from(BlockError::InvalidDifficulty(Mismatch {
                expected: expected_difficulty,
                found: self.arena[index].difficulty.clone(),
            })));
        }

        Ok(())
    }
}

pub struct SynchronizationGraph {
    pub inner: RwLock<SynchronizationGraphInner>,
    pub block_headers: RwLock<HashMap<H256, BlockHeader>>,
    pub blocks: RwLock<HashMap<H256, Block>>,
    genesis_block_hash: H256,
    pub consensus: SharedConsensusGraph,
}

impl SynchronizationGraph {
    pub fn new(consensus: SharedConsensusGraph) -> Self {
        let genesis_block = consensus.genesis_block();
        let genesis_block_hash = genesis_block.hash();

        let mut block_headers = HashMap::new();
        block_headers
            .insert(genesis_block_hash, genesis_block.block_header.clone());

        let mut blocks = HashMap::new();
        blocks.insert(genesis_block_hash, genesis_block.clone());

        SynchronizationGraph {
            inner: RwLock::new(SynchronizationGraphInner::with_genesis_block(
                &genesis_block,
            )),
            block_headers: RwLock::new(block_headers),
            blocks: RwLock::new(blocks),
            genesis_block_hash,
            consensus,
        }
    }

    pub fn block_header_by_hash(&self, hash: &H256) -> Option<BlockHeader> {
        self.blocks
            .read()
            .get(hash)
            .map(|block| &block.block_header)
            .cloned()
    }

    pub fn block_by_hash(&self, hash: &H256) -> Option<Block> {
        self.blocks.read().get(hash).cloned()
    }

    pub fn genesis_hash(&self) -> &H256 { &self.genesis_block_hash }

    pub fn contains_block_header(&self, hash: &H256) -> bool {
        self.block_headers.read().contains_key(hash)
    }

    pub fn insert_block_header(&self, header: BlockHeader) {
        self.inner.write().insert(&header);
        self.block_headers.write().insert(header.hash(), header);
    }

    pub fn contains_block(&self, hash: &H256) -> bool {
        self.blocks.read().contains_key(hash)
    }

    pub fn insert_block(&self, block: Block) {
        let hash = block.hash();
        {
            self.blocks.write().insert(hash, block);
        }

        let mut inner = self.inner.write();
        let me = *inner.indices.get(&hash).unwrap();
        let mut queue = VecDeque::new();

        debug_assert!(!inner.arena[me].block_ready);
        inner.arena[me].block_ready = true;
        queue.push_back(me);

        while let Some(index) = queue.pop_front() {
            if inner.new_to_be_block_graph_ready(index) {
                inner.arena[index].graph_status = BLOCK_GRAPH_READY;
                self.consensus.on_new_block(
                    self.block_by_hash(&inner.arena[index].hash).unwrap(),
                );
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
    }
}

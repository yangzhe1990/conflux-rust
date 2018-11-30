use consensus::SharedConsensusGraph;
use ethereum_types::H256;
use parking_lot::RwLock;
use primitives::{Block, BlockHeader};
use slab::Slab;
use std::collections::{HashMap, HashSet, VecDeque};

const NULL: usize = !0;

const BLOCK_HEADER_ONLY: u8 = 0;
const BLOCK_READY: u8 = 1;
#[allow(dead_code)]
const BLOCK_PARENTAL_TREE_READY: u8 = 2;
const BLOCK_GRAPH_READY: u8 = 3;

pub struct SynchronizationGraphNode {
    pub hash: H256,
    pub status: u8,
    pub parent: usize,
    pub children: Vec<usize>,
    pub referees: Vec<usize>,
    pub pending_referees: HashSet<H256>,
    pub referrers: Vec<usize>,
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

    pub fn insert(&mut self, header: &BlockHeader) -> usize {
        let hash = header.hash();
        let me = self.arena.insert(SynchronizationGraphNode {
            hash,
            status: if *header.parent_hash() == H256::default() {
                BLOCK_GRAPH_READY
            } else {
                BLOCK_HEADER_ONLY
            },
            parent: NULL,
            children: Vec::new(),
            referees: Vec::new(),
            pending_referees: HashSet::new(),
            referrers: Vec::new(),
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
                self.arena[me].pending_referees.insert(*referee_hash);
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
                self.arena[*referrer].referees.push(me);
                self.arena[*referrer].pending_referees.remove(&hash);
            }
            self.arena[me].referrers = referrers;
        }

        me
    }

    pub fn is_graph_ready(&self, index: usize) -> bool {
        if self.arena[index].status < BLOCK_READY
            || !self.arena[index].pending_referees.is_empty()
        {
            false
        } else {
            let parent = self.arena[index].parent;

            parent != NULL
                && self.arena[parent].status >= BLOCK_GRAPH_READY
                && !self.arena[index].referees.iter().any(|&referee| {
                    self.arena[referee].status < BLOCK_GRAPH_READY
                })
        }
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

        inner.arena[me].status = BLOCK_READY;
        queue.push_back(me);

        while let Some(index) = queue.pop_front() {
            if inner.is_graph_ready(index)
                && inner.arena[index].status != BLOCK_GRAPH_READY
            {
                inner.arena[index].status = BLOCK_GRAPH_READY;
                self.consensus.on_new_block(
                    self.block_by_hash(&inner.arena[index].hash).unwrap(),
                );
                for child in &inner.arena[index].children {
                    queue.push_back(*child);
                }
                for referrer in &inner.arena[index].referrers {
                    queue.push_back(*referrer);
                }
            }
        }
    }
}

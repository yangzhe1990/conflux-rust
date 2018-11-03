use ethereum_types::{H256, U256};
use parking_lot::RwLock;
use primitives::Block;
use slab::Slab;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

const NULL: usize = !0;

pub struct ConsensusGraphNode {
    pub hash: H256,
    pub total_difficulty: U256,

    pub parent: usize,
    pub children: Vec<usize>,
    pub referees: Vec<usize>,
}

pub struct ConsensusGraphInner {
    pub arena: Slab<ConsensusGraphNode>,
    pub indices: HashMap<H256, usize>,
    pub pivot_chain: Vec<H256>,
    pub terminal_block_hashes: HashSet<H256>,

    genesis_block_index: usize,
}

impl ConsensusGraphInner {
    pub fn with_genesis_block(genesis_block: &Block) -> Self {
        let mut inner = ConsensusGraphInner {
            arena: Slab::new(),
            indices: HashMap::new(),
            pivot_chain: Vec::new(),
            terminal_block_hashes: HashSet::new(),
            genesis_block_index: NULL,
        };
        inner.genesis_block_index = inner.insert(genesis_block);
        inner.pivot_chain.push(genesis_block.hash());

        inner
    }

    pub fn insert(&mut self, block: &Block) -> usize {
        let hash = block.hash();

        let parent = if *block.block_header.parent_hash() != H256::default() {
            self.indices
                .get(block.block_header.parent_hash())
                .cloned()
                .unwrap()
        } else {
            NULL
        };
        let referees = block
            .block_header
            .referee_hashes()
            .iter()
            .map(|hash| self.indices.get(hash).cloned().unwrap())
            .collect();

        let index = self.arena.insert(ConsensusGraphNode {
            hash,
            total_difficulty: block.block_header.difficulty().clone(),
            parent,
            children: Vec::new(),
            referees,
        });
        self.indices.insert(hash, index);

        if parent != NULL {
            self.terminal_block_hashes.remove(&self.arena[parent].hash);
            self.terminal_block_hashes.insert(hash);
            self.arena[parent].children.push(index);
        }

        index
    }

    pub fn on_new_block(&mut self, block: &Block) {
        let mut me = self.insert(block);
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
            new_pivot_chain.push(self.arena[me].hash);
            let heaviest = self.arena[me]
                .children
                .iter()
                .max_by_key(|index| self.arena[**index].total_difficulty)
                .cloned()
                .unwrap();
            me = heaviest;
            if self.arena[me].children.len() == 0 {
                break;
            }
        }
        self.pivot_chain = new_pivot_chain;
    }
}

pub struct ConsensusGraph {
    pub inner: RwLock<ConsensusGraphInner>,
    pub blocks: RwLock<HashMap<H256, Block>>,
    genesis_block_hash: H256,
}

pub type SharedConsensusGraph = Arc<ConsensusGraph>;

impl ConsensusGraph {
    pub fn with_genesis_block(genesis_block: Block) -> Self {
        let genesis_block_hash = genesis_block.hash();

        let mut blocks = HashMap::new();
        blocks.insert(genesis_block_hash, genesis_block.clone());

        ConsensusGraph {
            inner: RwLock::new(ConsensusGraphInner::with_genesis_block(
                &genesis_block,
            )),
            blocks: RwLock::new(blocks),
            genesis_block_hash,
        }
    }

    pub fn genesis_block(&self) -> Block {
        let blocks = self.blocks.read();
        blocks.get(&self.genesis_block_hash).unwrap().clone()
    }

    pub fn contains_block(&self, hash: &H256) -> bool {
        self.blocks.read().contains_key(hash)
    }

    pub fn on_new_block(&self, block: Block) {
        self.inner.write().on_new_block(&block);
        self.blocks.write().insert(block.hash(), block);
    }

    pub fn best_block_hash(&self) -> H256 {
        let pivot_chain = &self.inner.read().pivot_chain;
        pivot_chain[pivot_chain.len() - 1]
    }

    pub fn terminal_block_hashes(&self) -> Vec<H256> {
        self.inner
            .read()
            .terminal_block_hashes
            .iter()
            .map(|hash| hash.clone())
            .collect()
    }
}

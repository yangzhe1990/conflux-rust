use block::Block;
use ethereum_types::{Address, H256, U256};
use hash::KECCAK_NULL_RLP;
use block_header::{BlockHeader, BlockHeaderBuilder};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;
pub use primitives::*;

/// Contains information on a best block that is specific to the consensus engine.
///
/// For GHOST fork-choice rule it would typically describe the block with highest
/// combined difficulty (usually the block with the highest block number).
///
/// Sometimes refered as 'latest block'.
pub struct BestBlock {
    /// Best block decoded header.
    pub header: BlockHeader,
    /// Best block uncompressed bytes.
    pub block: Block,
    /// Best block total difficulty.
    pub total_difficulty: U256,
}

pub struct BlockDetail {
    to_genesis: bool,
    total_difficulty: U256,
    children: Vec<H256>,
}

pub struct Ledger {
    // All locks must be captured in the order declared here.
    best_block: RwLock<BestBlock>,
    pub block_headers: RwLock<HashMap<H256, BlockHeader>>,
    block_bodies: RwLock<HashMap<H256, Block>>,
    child_blocks: RwLock<HashMap<H256, BlockDetail>>,
    /// maintain the main chain blocks
    block_hashes: RwLock<HashMap<BlockNumber, H256>>,
}

/// Information about the ledger gathered together.
#[derive(Clone, Debug)]
pub struct LedgerInfo {
    /// Blockchain difficulty.
    pub total_difficulty: U256,
    /// Genesis block hash.
    pub genesis_hash: H256,
    /// Best blockchain block hash.
    pub best_block_hash: H256,
    /// Best ledger block number.
    pub best_block_number: BlockNumber,
}

pub type LedgerRef = Arc<Ledger>;

impl Ledger {
    pub fn new() -> Self {
        Ledger {
            block_headers: RwLock::new(HashMap::new()),
            block_bodies: RwLock::new(HashMap::new()),
            child_blocks: RwLock::new(HashMap::new()),
            block_hashes: RwLock::new(HashMap::new()),

            best_block: RwLock::new(BestBlock {
                header: BlockHeader::new(),
                block: Block::default(),
                total_difficulty: 0.into(),
            }),
        }
    }

    pub fn new_ref() -> LedgerRef {
        Arc::new(Self::new())
    }

    pub fn initialize_with_genesis(&self) {
        let mut genesis_header = BlockHeaderBuilder::new()
            .with_parent_hash(0.into())
            .with_timestamp(0)
            .with_number(0)
            .with_author(Address::default())
            .with_transactions_root(KECCAK_NULL_RLP)
            .with_state_root(KECCAK_NULL_RLP)
            .with_difficulty(0.into())
            .build();

        genesis_header.compute_hash();
        let hash = genesis_header.hash();

        let txs: Vec<SignedTransaction> = Vec::new();
        let genesis_body = Block {
            hash: hash,
            transactions: txs,
        };

        let mut write = self.best_block.write();
        write.header = genesis_header.clone();
        write.block = genesis_body.clone();
        write.total_difficulty = 0.into();

        self.block_headers
            .write()
            .insert(hash, genesis_header.clone());
        self.block_bodies.write().insert(hash, genesis_body.clone());

        let genesis_block_detail = BlockDetail {
            to_genesis: true,
            total_difficulty: 0.into(),
            children: Vec::new(),
        };
        self.child_blocks.write().insert(hash, genesis_block_detail);
        self.block_hashes.write().insert(0, hash);
    }

    /// Get the hash of given block's number.
    fn block_hash_by_number(&self, index: BlockNumber) -> Option<H256> {
        if let Some(v) = self.block_hashes.read().get(&index) {
            Some(v.clone())
        } else {
            None
        }
    }

    /// Get the block number by its hash
    pub fn block_number_by_hash(&self, hash: &H256) -> Option<BlockNumber> {
        let read = self.block_headers.read();
        if let Some(v) = read.get(hash) {
            return Some(v.number());
        } else {
            None
        }
    }

    pub fn block_hash(&self, id: BlockId) -> Option<H256> {
        match id {
            BlockId::Hash(hash) => Some(hash),
            BlockId::Number(number) => self.block_hash_by_number(number),
            BlockId::Earliest => self.block_hash_by_number(0),
            BlockId::Latest => Some(self.best_block_hash()),
        }
    }

    /// Get best block hash.
    pub fn best_block_hash(&self) -> H256 {
        self.best_block.read().header.hash()
    }

    /// Get best block number.
    pub fn best_block_number(&self) -> BlockNumber {
        self.best_block.read().header.number()
    }

    pub fn best_block(&self) -> BestBlock {
        let read = self.best_block.read();
        BestBlock {
            header: read.header.clone(),
            block: read.block.clone(),
            total_difficulty: read.total_difficulty,
        }
    }

    /// Get block header data
    pub fn block_header_by_hash(&self, hash: &H256) -> Option<BlockHeader> {
        let read = self.block_headers.read();
        if let Some(v) = read.get(hash) {
            return Some(v.clone());
        } else {
            None
        }
    }

    /// Get block body data
    pub fn block_body_by_hash(&self, hash: &H256) -> Option<Block> {
        let read = self.block_bodies.read();
        if let Some(v) = read.get(hash) {
            return Some(v.clone());
        } else {
            None
        }
    }

    /// Returns reference to genesis hash.
    fn genesis_hash(&self) -> H256 {
        self.block_hash_by_number(0)
            .expect("Genesis hash should always exist")
    }

    /// Returns general ledger information
    pub fn ledger_info(&self) -> LedgerInfo {
        let best_block = self.best_block.read();
        let genesis_hash = self.genesis_hash();
        LedgerInfo {
            genesis_hash,
            total_difficulty: best_block.total_difficulty,
            best_block_hash: best_block.header.hash(),
            best_block_number: best_block.header.number(),
        }
    }

    pub fn block_header_exists(&self, hash: &H256) -> bool {
        self.block_headers.read().contains_key(hash)
    }

    pub fn block_body_exists(&self, hash: &H256) -> bool {
        self.block_bodies.read().contains_key(hash)
    }

    pub fn block_header(&self, id: BlockId) -> Option<BlockHeader> {
        self.block_hash(id)
            .and_then(|hash| self.block_header_by_hash(&hash))
    }

    pub fn block_body(&self, id: BlockId) -> Option<Block> {
        self.block_hash(id)
            .and_then(|hash| self.block_body_by_hash(&hash))
    }

    pub fn add_block_header_by_hash(&self, hash: &H256, header: BlockHeader) {
        self.block_headers.write().insert(hash.clone(), header);
    }

    pub fn add_block_body_by_hash(&self, hash: &H256, body: Block) {
        self.block_bodies.write().insert(hash.clone(), body);
    }

    pub fn add_child(&self, parent: &H256, child: &H256) {
        let mut write = self.child_blocks.write();
        let block_detail = write.entry(parent.clone()).or_insert(BlockDetail {
            to_genesis: false,
            total_difficulty: 0.into(),
            children: Vec::new(),
        });

        let mut exist = false;
        for item in block_detail.children.iter() {
            if *item == *child {
                exist = true;
            }
        }

        if !exist {
            block_detail.children.push(child.clone());
        }
    }

    pub fn adjust_main_chain(
        &self,
        mut blocks_to_adjust: VecDeque<H256>,
    ) -> bool {
        let mut best_block = self.best_block.write();
        let headers = self.block_headers.read();
        let bodies = self.block_bodies.read();
        let mut ledger_structure = self.child_blocks.write();

        let mut max_total_difficulty = 0.into();
        let mut hash_in_longest_chain = H256::default();
        //let mut number_in_longest_chain = 0;

        while !blocks_to_adjust.is_empty() {
            let hash = blocks_to_adjust.pop_front().unwrap();
            let header = headers.get(&hash).unwrap().clone();
            let parent_hash = header.parent_hash();

            if ledger_structure.contains_key(parent_hash) {
                let parent_total_difficulty;
                let to_genesis;
                {
                    let parent = ledger_structure.get(parent_hash).unwrap();
                    to_genesis = parent.to_genesis;
                    parent_total_difficulty = parent.total_difficulty;
                }

                if to_genesis {
                    let me =
                        ledger_structure.entry(hash).or_insert(BlockDetail {
                            to_genesis: false,
                            total_difficulty: 0.into(),
                            children: Vec::new(),
                        });

                    me.to_genesis = true;
                    me.total_difficulty =
                        parent_total_difficulty + *header.difficulty();

                    if me.total_difficulty > max_total_difficulty {
                        max_total_difficulty = me.total_difficulty;
                        hash_in_longest_chain = hash;
                        //number_in_longest_chain = header.number();
                    }

                    for child in me.children.iter() {
                        blocks_to_adjust.push_back(*child);
                    }
                }
            }
        }

        let mut adjusted = false;

        if max_total_difficulty > best_block.total_difficulty {
            best_block.header =
                headers.get(&hash_in_longest_chain).unwrap().clone();
            best_block.block =
                bodies.get(&hash_in_longest_chain).unwrap().clone();
            best_block.total_difficulty = max_total_difficulty;

            let mut main_chain = self.block_hashes.write();
            let mut cur_hash = hash_in_longest_chain;
            loop {
                let header = headers.get(&cur_hash).unwrap().clone();
                let number = header.number();
                let parent_hash = header.parent_hash();
                if !main_chain.contains_key(&number)
                    || *(main_chain.get(&number).unwrap()) != cur_hash
                {
                    main_chain.insert(number, cur_hash);
                    cur_hash = *parent_hash;
                    adjusted = true;
                } else {
                    break;
                }
            }
        }

        adjusted
    }
}

impl Default for Ledger {
    // FIXME: Fix this default trait as the initial state of the ledger
    fn default() -> Self {
        Self::new()
    }
}

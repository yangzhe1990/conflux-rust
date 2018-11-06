use cache_manager::WriteBackCacheManager;
use ethereum_types::{H256, U256};
use parking_lot::{Mutex, RwLock};
pub use primitives::*;
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

pub struct BestEpoch {
    pub best_epoch_hash: H256,
    pub best_epoch_number: EpochNumber,
}

#[allow(dead_code)]
pub struct BlockIndex {
    /// The hash of the parent of this block
    parent: H256,
    /// The hash of this block
    hash: H256,
    /// The hashes of the children of this block
    children: Vec<H256>,
    number: EpochNumber,
    epoch_block_hashes: Vec<H256>,
    /// Total amount of work in the subtree rooted at the entry
    total_difficulty: U256,
}

pub const MIN_LEDGER_CACHE_MB: usize = 4;
pub const DEFAULT_LEDGER_CACHE_SIZE: usize = 8;

#[allow(dead_code)]
#[derive(Debug, Hash, Eq, PartialEq, Clone)]
enum CacheId {
    Block(H256),
    BlockHeader(H256),
    BlockIndex(H256),
    EpochIndex0(EpochNumber),
    EpochIndex1(H256),
}

pub struct LedgerCacheConfig {
    pref_cache_size: usize,
    max_cache_size: usize,
}

pub fn to_ledger_cache_config(cache_size: usize) -> LedgerCacheConfig {
    let mb = 1024 * 1024;
    LedgerCacheConfig {
        pref_cache_size: cache_size * 3 / 4 * mb,
        max_cache_size: cache_size * mb,
    }
}

#[allow(dead_code)]
pub struct Ledger {
    best_epoch: RwLock<BestEpoch>,
    epoch_indices:
        RwLock<(HashMap<EpochNumber, H256>, HashMap<H256, EpochNumber>)>,
    block_indices: RwLock<HashMap<H256, BlockIndex>>,
    block_headers: RwLock<HashMap<H256, BlockHeader>>,
    blocks: RwLock<HashMap<H256, Block>>,

    cache_man: Mutex<WriteBackCacheManager<CacheId>>,
}

/// Information about the ledger gathered together.
#[derive(Clone, Debug)]
pub struct LedgerInfo {
    /// Genesis block hash.
    pub genesis_hash: H256,
    /// Best epoch hash.
    pub best_epoch_hash: H256,
    /// Best epoch number.
    pub best_epoch_number: EpochNumber,
}

pub type LedgerRef = Arc<Ledger>;

impl Ledger {
    pub fn new(config: LedgerCacheConfig) -> Self {
        // FIXME: set bytes_per_cache_entry correctly
        let cache_man = WriteBackCacheManager::new(
            config.pref_cache_size,
            config.max_cache_size,
            400,
        );
        Ledger {
            best_epoch: RwLock::new(BestEpoch {
                best_epoch_hash: H256::default(),
                best_epoch_number: 0,
            }),
            epoch_indices: RwLock::new((HashMap::new(), HashMap::new())),
            block_indices: RwLock::new(HashMap::new()),
            block_headers: RwLock::new(HashMap::new()),
            blocks: RwLock::new(HashMap::new()),
            cache_man: Mutex::new(cache_man),
        }
    }

    pub fn initialize_with_genesis(&self) {
        let mut genesis_block_header = BlockHeader::default();
        let genesis_hash = genesis_block_header.compute_hash();

        let genesis_block = Block {
            block_header: genesis_block_header.clone(),
            transactions: Vec::new(),
        };

        let mut best_epoch = self.best_epoch.write();
        best_epoch.best_epoch_hash = genesis_hash;
        best_epoch.best_epoch_number = 0;

        self.block_headers
            .write()
            .insert(genesis_hash, genesis_block_header.clone());
        self.blocks
            .write()
            .insert(genesis_hash, genesis_block.clone());

        let genesis_block_index = BlockIndex {
            parent: H256::default(),
            hash: genesis_hash,
            children: Vec::new(),
            number: 0,
            epoch_block_hashes: Vec::new(),
            total_difficulty: 0.into(),
        };
        self.block_indices
            .write()
            .insert(genesis_hash, genesis_block_index);

        let mut epoch_indices = self.epoch_indices.write();
        epoch_indices.0.insert(0, genesis_hash);
        epoch_indices.1.insert(genesis_hash, 0);
    }

    pub fn epoch_hash(&self, number: EpochNumber) -> Option<H256> {
        let epoch_hashes = &self.epoch_indices.write().0;
        epoch_hashes.get(&number).cloned()
    }

    /// Get best epoch hash.
    pub fn best_epoch_hash(&self) -> H256 {
        self.best_epoch.read().best_epoch_hash
    }

    /// Get best epoch number.
    pub fn best_epoch_number(&self) -> EpochNumber {
        self.best_epoch.read().best_epoch_number
    }

    /// Get block header
    pub fn block_header_by_hash(&self, hash: &H256) -> Option<BlockHeader> {
        let block_headers = self.block_headers.read();
        if let Some(header) = block_headers.get(hash) {
            Some(header.clone())
        } else {
            None
        }
    }

    /// Get block
    pub fn block_by_hash(&self, hash: &H256) -> Option<Block> {
        let blocks = self.blocks.read();
        if let Some(block) = blocks.get(hash) {
            Some(block.clone())
        } else {
            None
        }
    }

    /// Get transactions within an epoch
    pub fn epoch_transactions_by_hash(
        &self, hash: &H256,
    ) -> Option<Vec<SignedTransaction>> {
        let mut transactions = Vec::new();
        if let Some(block_index) = self.block_indices.read().get(hash) {
            for epoch_block_hash in &block_index.epoch_block_hashes {
                if let Some(block) = self.blocks.read().get(&epoch_block_hash) {
                    transactions.extend(block.transactions.clone());
                }
            }
            Some(transactions)
        } else {
            None
        }
    }

    /// Returns reference to genesis hash.
    fn genesis_hash(&self) -> H256 {
        self.epoch_hash(0)
            .expect("Genesis hash should always exist")
    }

    /// Returns general ledger information
    pub fn ledger_info(&self) -> LedgerInfo {
        LedgerInfo {
            genesis_hash: self.genesis_hash(),
            best_epoch_hash: self.best_epoch_hash(),
            best_epoch_number: self.best_epoch_number(),
        }
    }

    pub fn block_header_exists(&self, hash: &H256) -> bool {
        self.block_headers.read().contains_key(hash)
    }

    pub fn block_exists(&self, hash: &H256) -> bool {
        self.blocks.read().contains_key(hash)
    }

    //
    //    pub fn block_header(&self, id: BlockId) -> Option<BlockHeader> {
    //        self.block_hash(id)
    //            .and_then(|hash| self.block_header_by_hash(&hash))
    //    }
    //
    //    pub fn block_by_hash(&self, hash: &H256) -> Option<Block> {
    //        self.block_hash(id)
    //            .and_then(|hash| self.block_body_by_hash(&hash))
    //    }
    //
    pub fn add_block_header_by_hash(&self, hash: &H256, header: BlockHeader) {
        self.block_headers.write().insert(hash.clone(), header);
    }

    pub fn add_block_by_hash(&self, hash: &H256, block: Block) {
        self.blocks.write().insert(hash.clone(), block);
    }

    pub fn add_child(&self, _parent: &H256, _child: &H256) {
        //        let mut write = self.block_indices.write();
        //        let block_index =
        // write.entry(parent.clone()).or_insert(BlockIndex {
        //            to_genesis: false,
        //            total_difficulty: 0.into(),
        //            children: Vec::new(),
        //        });
        //
        //        let mut exist = false;
        //        for item in block_detail.children.iter() {
        //            if *item == *child {
        //                exist = true;
        //            }
        //        }
        //
        //        if !exist {
        //            block_detail.children.push(child.clone());
        //        }
    }

    pub fn adjust_main_chain(&self, _blocks_to_adjust: VecDeque<H256>) -> bool {
        false
        //
        //        let mut best_block = self.best_block.write();
        //        let headers = self.block_headers.read();
        //        let bodies = self.block_bodies.read();
        //        let mut ledger_structure = self.child_blocks.write();
        //
        //        let mut max_total_difficulty = 0.into();
        //        let mut hash_in_longest_chain = H256::default();
        //        //let mut number_in_longest_chain = 0;
        //
        //        while !blocks_to_adjust.is_empty() {
        //            let hash = blocks_to_adjust.pop_front().unwrap();
        //            let header = headers.get(&hash).unwrap().clone();
        //            let parent_hash = header.parent_hash();
        //
        //            if ledger_structure.contains_key(parent_hash) {
        //                let parent_total_difficulty;
        //                let to_genesis;
        //                {
        //                    let parent =
        // ledger_structure.get(parent_hash).unwrap();
        // to_genesis =
        // parent.to_genesis;
        //                     parent_total_difficulty =
        // parent.total_difficulty;                }
        //
        //                if to_genesis {
        //                    let me =
        //
        // ledger_structure.entry(hash).or_insert(BlockDetail {
        //                            to_genesis: false,
        //                            total_difficulty: 0.into(),
        //                            children: Vec::new(),
        //                        });
        //
        //                    me.to_genesis = true;
        //                    me.total_difficulty =
        //                        parent_total_difficulty +
        // *header.difficulty();
        //
        //                    if me.total_difficulty > max_total_difficulty {
        //                        max_total_difficulty = me.total_difficulty;
        //                        hash_in_longest_chain = hash;
        //                        //number_in_longest_chain = header.number();
        //                    }
        //
        //                    for child in me.children.iter() {
        //                        blocks_to_adjust.push_back(*child);
        //                    }
        //                }
        //            }
        //        }
        //
        //        let mut adjusted = false;
        //
        //        if max_total_difficulty > best_block.total_difficulty {
        //            best_block.header =
        //                headers.get(&hash_in_longest_chain).unwrap().clone();
        //            best_block.block =
        //                bodies.get(&hash_in_longest_chain).unwrap().clone();
        //            best_block.total_difficulty = max_total_difficulty;
        //
        //            let mut main_chain = self.block_hashes.write();
        //            let mut cur_hash = hash_in_longest_chain;
        //            loop {
        //                let header = headers.get(&cur_hash).unwrap().clone();
        //                let number = header.number();
        //                let parent_hash = header.parent_hash();
        //                if !main_chain.contains_key(&number)
        //                    || *(main_chain.get(&number).unwrap()) != cur_hash
        //                {
        //                    main_chain.insert(number, cur_hash);
        //                    cur_hash = *parent_hash;
        //                    adjusted = true;
        //                } else {
        //                    break;
        //                }
        //            }
        //        }
        //
        //        adjusted
    }
}

impl Default for Ledger {
    // FIXME: Fix this default trait as the initial state of the ledger
    fn default() -> Self {
        let ledger_cache_config =
            to_ledger_cache_config(DEFAULT_LEDGER_CACHE_SIZE);
        Self::new(ledger_cache_config)
    }
}

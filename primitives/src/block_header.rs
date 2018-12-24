use crate::bytes::Bytes;
use ethereum_types::{Address, H256, U256};
use crate::hash::{keccak, KECCAK_NULL_RLP};
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};

/// A block header.
#[derive(Clone, Debug, Eq)]
pub struct BlockHeader {
    /// Parent hash.
    parent_hash: H256,
    /// Block height
    height: u64,
    /// Block timestamp.
    timestamp: u64,
    /// Block author.
    author: Address,
    /// Transactions root.
    transactions_root: H256,
    /// Deferred state root.
    deferred_state_root: H256,
    /// Block difficulty.
    difficulty: U256,
    /// Gas limit.
    gas_limit: U256,
    /// Gas used.
    gas_used: U256,
    /// Referee hashes
    referee_hashes: Vec<H256>,
    /// Nonce of the block
    nonce: u64,
    /// Hash of the block
    hash: Option<H256>,
}

impl PartialEq for BlockHeader {
    fn eq(&self, o: &BlockHeader) -> bool {
        self.parent_hash == o.parent_hash
            && self.height == o.height
            && self.timestamp == o.timestamp
            && self.author == o.author
            && self.transactions_root == o.transactions_root
            && self.deferred_state_root == o.deferred_state_root
            && self.difficulty == o.difficulty
            && self.gas_limit == o.gas_limit
            && self.gas_used == o.gas_used
            && self.referee_hashes == o.referee_hashes
    }
}

impl Default for BlockHeader {
    fn default() -> Self {
        BlockHeader {
            parent_hash: H256::default(),
            height: 0,
            timestamp: 0,
            author: Address::default(),
            transactions_root: KECCAK_NULL_RLP,
            deferred_state_root: KECCAK_NULL_RLP,
            difficulty: U256::zero(),
            gas_limit: U256::zero(),
            gas_used: U256::zero(),
            referee_hashes: Vec::new(),
            nonce: 0,
            hash: None,
        }
    }
}

impl BlockHeader {
    /// Create a new, default-valued, header.
    pub fn new() -> Self { Self::default() }

    /// Get the parent_hash field of the header.
    pub fn parent_hash(&self) -> &H256 { &self.parent_hash }

    /// Get the block height
    pub fn height(&self) -> u64 { self.height }

    /// Get the timestamp field of the header.
    pub fn timestamp(&self) -> u64 { self.timestamp }

    /// Get the author field of the header.
    pub fn author(&self) -> &Address { &self.author }

    /// Get the transactions root field of the header.
    pub fn transactions_root(&self) -> &H256 { &self.transactions_root }

    /// Get the deferred state root field of the header.
    pub fn deferred_state_root(&self) -> &H256 { &self.deferred_state_root }

    /// Get the difficulty field of the header.
    pub fn difficulty(&self) -> &U256 { &self.difficulty }

    /// Get the gas limit field of the header.
    pub fn gas_limit(&self) -> &U256 { &self.gas_limit }

    /// Get the referee hashes field of the header.
    pub fn referee_hashes(&self) -> &Vec<H256> { &self.referee_hashes }

    /// Get the nonce field of the header.
    pub fn nonce(&self) -> u64 { self.nonce }

    /// Set the nonce field of the header.
    pub fn set_nonce(&mut self, nonce: u64) { self.nonce = nonce; }

    /// Compute the hash of the block.
    pub fn compute_hash(&mut self) -> H256 {
        let hash = self.hash();
        self.hash = Some(hash);
        hash
    }

    /// Get the hash of the block.
    pub fn hash(&self) -> H256 {
        self.hash.unwrap_or_else(|| keccak(self.rlp()))
    }

    /// Get the hash of PoW problem.
    pub fn problem_hash(&self) -> H256 { keccak(self.rlp_without_nonce()) }

    /// Get the hash of the block.
    pub fn bare_hash(&self) -> H256 { keccak(self.rlp_without_nonce()) }

    /// Get the RLP representation of this header(except nonce).
    pub fn rlp_without_nonce(&self) -> Bytes {
        let mut stream = RlpStream::new();
        self.stream_rlp_without_nonce(&mut stream);
        stream.out()
    }

    /// Get the RLP representation of this header.
    pub fn rlp(&self) -> Bytes {
        let mut stream = RlpStream::new();
        self.stream_rlp(&mut stream);
        stream.out()
    }

    /// Place this header(except nonce) into an RLP stream `stream`.
    fn stream_rlp_without_nonce(&self, stream: &mut RlpStream) {
        stream
            .begin_list(10)
            .append(&self.parent_hash)
            .append(&self.height)
            .append(&self.timestamp)
            .append(&self.author)
            .append(&self.transactions_root)
            .append(&self.deferred_state_root)
            .append(&self.difficulty)
            .append(&self.gas_limit)
            .append(&self.gas_used)
            .append_list(&self.referee_hashes);
    }

    /// Place this header into an RLP stream `stream`.
    fn stream_rlp(&self, stream: &mut RlpStream) {
        stream
            .begin_list(11)
            .append(&self.parent_hash)
            .append(&self.height)
            .append(&self.timestamp)
            .append(&self.author)
            .append(&self.transactions_root)
            .append(&self.deferred_state_root)
            .append(&self.difficulty)
            .append(&self.gas_limit)
            .append(&self.gas_used)
            .append_list(&self.referee_hashes)
            .append(&self.nonce);
    }

    pub fn size(&self) -> usize {
        // FIXME: We need to revisit the size of block header once we finished
        // the persistent storage part
        0
    }
}

pub struct BlockHeaderBuilder {
    parent_hash: H256,
    height: u64,
    timestamp: u64,
    author: Address,
    transactions_root: H256,
    deferred_state_root: H256,
    difficulty: U256,
    gas_limit: U256,
    gas_used: U256,
    referee_hashes: Vec<H256>,
    nonce: u64,
}

impl BlockHeaderBuilder {
    pub fn new() -> Self {
        Self {
            parent_hash: H256::default(),
            height: 0,
            timestamp: 0,
            author: Address::default(),
            transactions_root: KECCAK_NULL_RLP,
            deferred_state_root: KECCAK_NULL_RLP,
            difficulty: U256::default(),
            gas_limit: U256::zero(),
            gas_used: U256::zero(),
            referee_hashes: Vec::new(),
            nonce: 0,
        }
    }

    pub fn with_parent_hash(&mut self, parent_hash: H256) -> &mut Self {
        self.parent_hash = parent_hash;
        self
    }

    pub fn with_height(&mut self, height: u64) -> &mut Self {
        self.height = height;
        self
    }

    pub fn with_timestamp(&mut self, timestamp: u64) -> &mut Self {
        self.timestamp = timestamp;
        self
    }

    pub fn with_author(&mut self, author: Address) -> &mut Self {
        self.author = author;
        self
    }

    pub fn with_transactions_root(
        &mut self, transactions_root: H256,
    ) -> &mut Self {
        self.transactions_root = transactions_root;
        self
    }

    pub fn with_deferred_state_root(
        &mut self, deferred_state_root: H256,
    ) -> &mut Self {
        self.deferred_state_root = deferred_state_root;
        self
    }

    pub fn with_difficulty(&mut self, difficulty: U256) -> &mut Self {
        self.difficulty = difficulty;
        self
    }

    pub fn with_gas_limit(&mut self, gas_limit: U256) -> &mut Self {
        self.gas_limit = gas_limit;
        self
    }

    pub fn with_gas_used(&mut self, gas_used: U256) -> &mut Self {
        self.gas_used = gas_used;
        self
    }

    pub fn with_referee_hashes(
        &mut self, referee_hashes: Vec<H256>,
    ) -> &mut Self {
        self.referee_hashes = referee_hashes;
        self
    }

    pub fn with_nonce(&mut self, nonce: u64) -> &mut Self {
        self.nonce = nonce;
        self
    }

    pub fn build(&self) -> BlockHeader {
        BlockHeader {
            parent_hash: self.parent_hash,
            height: self.height,
            timestamp: self.timestamp,
            author: self.author,
            transactions_root: self.transactions_root,
            deferred_state_root: self.deferred_state_root,
            difficulty: self.difficulty,
            gas_limit: self.gas_limit,
            gas_used: self.gas_used,
            referee_hashes: self.referee_hashes.clone(),
            nonce: self.nonce,
            hash: None,
        }
    }
}

impl Encodable for BlockHeader {
    fn rlp_append(&self, stream: &mut RlpStream) { self.stream_rlp(stream); }
}

impl Decodable for BlockHeader {
    fn decode(r: &Rlp) -> Result<Self, DecoderError> {
        Ok(BlockHeader {
            parent_hash: r.val_at(0)?,
            height: r.val_at(1)?,
            timestamp: r.val_at(2)?,
            author: r.val_at(3)?,
            transactions_root: r.val_at(4)?,
            deferred_state_root: r.val_at(5)?,
            difficulty: r.val_at(6)?,
            gas_limit: r.val_at(7)?,
            gas_used: r.val_at(8)?,
            referee_hashes: r.list_at(9)?,
            nonce: r.val_at(10)?,
            hash: keccak(r.as_raw()).into(),
        })
    }
}

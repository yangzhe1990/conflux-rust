use bytes::Bytes;
use ethereum_types::{Address, H256, U256};
use hash::{keccak, KECCAK_NULL_RLP};
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use std::cmp;

pub use types::BlockNumber;

/// A block header.
#[derive(Debug, Clone, Eq)]
pub struct BlockHeader {
    /// Parent hash.
    parent_hash: H256,
    /// Block timestamp.
    timestamp: u64,
    /// Block number.
    number: BlockNumber,
    /// Block author.
    author: Address,
    /// Transactions root.
    transactions_root: H256,
    /// State root.
    state_root: H256,
    /// Block difficulty.
    difficulty: U256,
    /// Memorized hash of that header and the seal.
    hash: Option<H256>,
}

impl PartialEq for BlockHeader {
    fn eq(&self, c: &BlockHeader) -> bool {
        if let (&Some(ref h1), &Some(ref h2)) = (&self.hash, &c.hash) {
            if h1 == h2 {
                return true;
            }
        }

        self.parent_hash == c.parent_hash
            && self.timestamp == c.timestamp
            && self.number == c.number
            && self.author == c.author
            && self.transactions_root == c.transactions_root
            && self.state_root == c.state_root
            && self.difficulty == c.difficulty
    }
}

impl Default for BlockHeader {
    fn default() -> Self {
        BlockHeader {
            parent_hash: H256::default(),
            timestamp: 0,
            number: 0,
            author: Address::default(),
            transactions_root: KECCAK_NULL_RLP,
            state_root: KECCAK_NULL_RLP,
            difficulty: U256::default(),
            hash: None,
        }
    }
}

impl BlockHeader {
    /// Create a new, default-valued, header.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the parent_hash field of the header.
    pub fn parent_hash(&self) -> &H256 {
        &self.parent_hash
    }

    /// Get the timestamp field of the header.
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Get the number field of the header.
    pub fn number(&self) -> BlockNumber {
        self.number
    }

    /// Get the author field of the header.
    pub fn author(&self) -> &Address {
        &self.author
    }

    /// Get the state root field of the header.
    pub fn state_root(&self) -> &H256 {
        &self.state_root
    }

    /// Get the transactions root field of the header.
    pub fn transactions_root(&self) -> &H256 {
        &self.transactions_root
    }

    /// Get the difficulty field of the header.
    pub fn difficulty(&self) -> &U256 {
        &self.difficulty
    }

    /// Get & memoize the hash of this header (keccak of the RLP with seal).
    pub fn compute_hash(&mut self) -> H256 {
        let hash = self.hash();
        self.hash = Some(hash);
        hash
    }

    /// Get the hash of this header (keccak of the RLP with seal).
    pub fn hash(&self) -> H256 {
        self.hash.unwrap_or_else(|| keccak(self.rlp()))
    }

    /// Get the hash of the header excluding the seal
    pub fn bare_hash(&self) -> H256 {
        keccak(self.rlp())
    }

    /// Get the RLP representation of this Header.
    pub fn rlp(&self) -> Bytes {
        let mut s = RlpStream::new();
        self.stream_rlp(&mut s);
        s.out()
    }

    /// Place this header into an RLP stream `s`, optionally `with_seal`.
    fn stream_rlp(&self, s: &mut RlpStream) {
        s.begin_list(7)
            .append(&self.parent_hash)
            .append(&self.author)
            .append(&self.state_root)
            .append(&self.transactions_root)
            .append(&self.difficulty)
            .append(&self.number)
            .append(&self.timestamp);
    }
}

pub struct BlockHeaderBuilder {
    parent_hash: H256,
    state_root: H256,
    transactions_root: H256,
    timestamp: u64,
    number: BlockNumber,
    author: Address,
    difficulty: U256,
}

impl BlockHeaderBuilder {
    pub fn new() -> Self {
        Self {
            parent_hash: H256::default(),
            state_root: H256::default(),
            transactions_root: H256::default(),
            timestamp: 0,
            number: 0,
            author: Address::new(),
            difficulty: U256::default(),
        }
    }

    pub fn with_parent_hash(&mut self, parent_hash: H256) -> &mut Self {
        self.parent_hash = parent_hash;
        self
    }

    pub fn with_state_root(&mut self, state_root: H256) -> &mut Self {
        self.state_root = state_root;
        self
    }

    pub fn with_transactions_root(&mut self, transactions_root: H256) -> &mut Self {
        self.transactions_root = transactions_root;
        self
    }

    pub fn with_timestamp(&mut self, timestamp: u64) -> &mut Self {
        self.timestamp = timestamp;
        self
    }

    pub fn with_number(&mut self, number: u64) -> &mut Self {
        self.number = number;
        self
    }

    pub fn with_author(&mut self, author: Address) -> &mut Self {
        self.author = author;
        self
    }

    pub fn with_difficulty(&mut self, difficulty: U256) -> &mut Self {
        self.difficulty = difficulty;
        self
    }

    pub fn build(&self) -> BlockHeader {
        BlockHeader {
            parent_hash: self.parent_hash,
            timestamp: self.timestamp,
            number: self.number,
            author: self.author,
            transactions_root: self.transactions_root,
            state_root: self.state_root,
            difficulty: self.difficulty,
            hash: None,
        }
    }
}

impl Decodable for BlockHeader {
    fn decode(r: &Rlp) -> Result<Self, DecoderError> {
        Ok(BlockHeader {
            parent_hash: r.val_at(0)?,
            author: r.val_at(1)?,
            state_root: r.val_at(2)?,
            transactions_root: r.val_at(3)?,
            difficulty: r.val_at(4)?,
            number: r.val_at(5)?,
            timestamp: cmp::min(r.val_at::<U256>(6)?, u64::max_value().into())
                .as_u64(),
            hash: keccak(r.as_raw()).into(),
        })
    }
}

impl Encodable for BlockHeader {
    fn rlp_append(&self, s: &mut RlpStream) {
        self.stream_rlp(s);
    }
}

/// A mega block header
#[derive(Debug, Clone, Eq)]
pub struct MegaHeader {
    /// Parent hash
    parent_hash: H256,
    /// Reference hashes
    reference_hashes: Vec<H256>,
    /// Block timestamp
    timestamp: u64,
    /// Block number ???
    number: BlockNumber,
    /// Block author
    author: Address,
    /// Transactions root
    transactions_root: H256,
    /// State root
    state_root: H256,
    /// Block difficulty
    difficulty: U256,
    /// Memorized hash of that header and the seal.
    hash: Option<H256>,
}

impl PartialEq for MegaHeader {
    fn eq(&self, h: &MegaHeader) -> bool {
        if let (&Some(ref h1), &Some(ref h2)) = (&self.hash, &h.hash) {
            if h1 == h2 {
                return true;
            }
        }

        self.parent_hash == h.parent_hash
            && self.reference_hashes == h.reference_hashes
            && self.timestamp == h.timestamp
            && self.number == h.number
            && self.author == h.author
            && self.transactions_root == h.transactions_root
            && self.state_root == h.state_root
            && self.difficulty == h.difficulty
    }
}

impl Default for MegaHeader {
    fn default() -> Self {
        MegaHeader {
            parent_hash: H256::default(),
            reference_hashes: Vec::new(),
            timestamp: 0,
            number: 0,
            author: Address::default(),
            transactions_root: KECCAK_NULL_RLP,
            state_root: KECCAK_NULL_RLP,
            difficulty: U256::default(),
            hash: None,
        }
    }
}

impl MegaHeader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn parent_hash(&self) -> &H256 {
        &self.parent_hash
    }

    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    pub fn number(&self) -> BlockNumber {
        self.number
    }

    pub fn author(&self) -> &Address {
        &self.author
    }

    pub fn state_root(&self) -> &H256 {
        &self.state_root
    }

    pub fn transactions_root(&self) -> &H256 {
        &self.transactions_root
    }

    pub fn difficulty(&self) -> &U256 {
        &self.difficulty
    }

    pub fn rlp(&self) -> Bytes {
        let mut stream = RlpStream::new();
        self.stream_rlp(&mut stream);
        stream.out()
    }

    fn stream_rlp(&self, stream: &mut RlpStream) {
        stream.begin_list(8)
            .append(&self.parent_hash)
            .append_list(&self.reference_hashes)
            .append(&self.author)
            .append(&self.state_root)
            .append(&self.transactions_root)
            .append(&self.difficulty)
            .append(&self.number)
            .append(&self.timestamp);
    }
}

impl Decodable for MegaHeader {
    fn decode(r: &Rlp) -> Result<Self, DecoderError> {
        Ok(MegaHeader {
            parent_hash: r.val_at(0)?,
            reference_hashes: r.list_at(1)?,
            author: r.val_at(2)?,
            state_root: r.val_at(3)?,
            transactions_root: r.val_at(4)?,
            difficulty: r.val_at(5)?,
            number: r.val_at(6)?,
            timestamp: cmp::min(r.val_at::<U256>(6)?, u64::max_value().into()).as_u64(),
            hash: keccak(r.as_raw()).into(),
        })
    }
}

impl Encodable for MegaHeader {
    fn rlp_append(&self, stream: &mut RlpStream) {
        self.stream_rlp(stream);
    }
}

pub struct MegaHeaderBuilder {
    parent_hash: H256,
    reference_hashes: Vec<H256>,
    state_root: H256,
    transactions_root: H256,
    timestamp: u64,
    number: BlockNumber,
    author: Address,
    difficulty: U256,
}

impl MegaHeaderBuilder {
    pub fn new() -> Self {
        Self {
            parent_hash: H256::default(),
            reference_hashes: Vec::new(),
            state_root: H256::default(),
            transactions_root: H256::default(),
            timestamp: 0,
            number: 0,
            author: Address::default(),
            difficulty: U256::default(),
        }
    }

    pub fn with_parent_hash(&mut self, parent_hash: H256) -> &mut Self {
        self.parent_hash = parent_hash;
        self
    }

    pub fn with_reference_hashes(&mut self, reference_hashes: Vec<H256>) -> &mut Self {
        self.reference_hashes = reference_hashes;
        self
    }

    pub fn with_state_root(&mut self, state_root: H256) -> &mut Self {
        self.state_root = state_root;
        self
    }

    pub fn with_transactions_root(&mut self, transactions_root: H256) -> &mut Self {
        self.transactions_root = transactions_root;
        self
    }

    pub fn with_timestamp(&mut self, timestamp: u64) -> &mut Self {
        self.timestamp = timestamp;
        self
    }

    pub fn with_number(&mut self, number: u64) -> &mut Self {
        self.number = number;
        self
    }

    pub fn with_author(&mut self, author: Address) -> &mut Self {
        self.author = author;
        self
    }

    pub fn with_difficulty(&mut self, difficulty: U256) -> &mut Self {
        self.difficulty = difficulty;
        self
    }

    pub fn build(&self) -> MegaHeader {
        MegaHeader {
            parent_hash: self.parent_hash,
            reference_hashes: self.reference_hashes.clone(),
            timestamp: self.timestamp,
            number: self.number,
            author: self.author,
            transactions_root: self.transactions_root,
            state_root: self.state_root,
            difficulty: self.difficulty,
            hash: None,
        }
    }
}
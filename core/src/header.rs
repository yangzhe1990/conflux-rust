use std::cmp;
use hash::{KECCAK_NULL_RLP, KECCAK_EMPTY_LIST_RLP, keccak};
use ethereum_types::{H256, U256, Address, Bloom};
use bytes::Bytes;
use rlp::{Rlp, RlpStream, Encodable, DecoderError, Decodable};

pub use types::BlockNumber;

/// A block header.
#[derive(Debug, Clone, Eq)]
pub struct Header {
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

	/// Memoized hash of that header and the seal.
	hash: Option<H256>,
}

impl PartialEq for Header {
	fn eq(&self, c: &Header) -> bool {
		if let (&Some(ref h1), &Some(ref h2)) = (&self.hash, &c.hash) {
			if h1 == h2 {
				return true
			}
		}

		self.parent_hash == c.parent_hash &&
		self.timestamp == c.timestamp &&
		self.number == c.number &&
		self.author == c.author &&
		self.transactions_root == c.transactions_root &&
		self.state_root == c.state_root &&
		self.difficulty == c.difficulty 
	}
}

impl Default for Header {
	fn default() -> Self {
		Header {
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

impl Header {
	/// Create a new, default-valued, header.
	pub fn new() -> Self { Self::default() }

	/// Get the parent_hash field of the header.
	pub fn parent_hash(&self) -> &H256 { &self.parent_hash }

	/// Get the timestamp field of the header.
	pub fn timestamp(&self) -> u64 { self.timestamp }

	/// Get the number field of the header.
	pub fn number(&self) -> BlockNumber { self.number }

	/// Get the author field of the header.
	pub fn author(&self) -> &Address { &self.author }

	/// Get the state root field of the header.
	pub fn state_root(&self) -> &H256 { &self.state_root }

	/// Get the transactions root field of the header.
	pub fn transactions_root(&self) -> &H256 { &self.transactions_root }

	/// Get the difficulty field of the header.
	pub fn difficulty(&self) -> &U256 { &self.difficulty }

	/// Set the number field of the header.
	pub fn set_parent_hash(&mut self, a: H256) {
		change_field(&mut self.hash, &mut self.parent_hash, a);
	}

	/// Set the state root field of the header.
	pub fn set_state_root(&mut self, a: H256) {
		change_field(&mut self.hash, &mut self.state_root, a);
	}

	/// Set the transactions root field of the header.
	pub fn set_transactions_root(&mut self, a: H256) {
		change_field(&mut self.hash, &mut self.transactions_root, a);
	}

	/// Set the timestamp field of the header.
	pub fn set_timestamp(&mut self, a: u64) {
		change_field(&mut self.hash, &mut self.timestamp, a);
	}

	/// Set the number field of the header.
	pub fn set_number(&mut self, a: BlockNumber) {
		change_field(&mut self.hash, &mut self.number, a);
	}

	/// Set the author field of the header.
	pub fn set_author(&mut self, a: Address) {
		change_field(&mut self.hash, &mut self.author, a);
	}

	/// Set the difficulty field of the header.
	pub fn set_difficulty(&mut self, a: U256) {
		change_field(&mut self.hash, &mut self.difficulty, a);
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
	fn rlp(&self) -> Bytes {
		let mut s = RlpStream::new();
		self.stream_rlp(&mut s);
		s.out()
	}

	/// Place this header into an RLP stream `s`, optionally `with_seal`.
	fn stream_rlp(&self, s: &mut RlpStream) {
		s.begin_list(7);
		s.append(&self.parent_hash);
		s.append(&self.author);
		s.append(&self.state_root);
		s.append(&self.transactions_root);
		s.append(&self.difficulty);
		s.append(&self.number);
		s.append(&self.timestamp);
	}
}

/// Alter value of given field, reset memoised hash if changed.
fn change_field<T>(hash: &mut Option<H256>, field: &mut T, value: T) where T: PartialEq<T> {
	if field != &value {
		*field = value;
		*hash = None;
	}
}

impl Decodable for Header {
	fn decode(r: &Rlp) -> Result<Self, DecoderError> {
		let mut blockheader = Header {
			parent_hash: r.val_at(0)?,
			author: r.val_at(1)?,
			state_root: r.val_at(2)?,
			transactions_root: r.val_at(3)?,
			difficulty: r.val_at(4)?,
			number: r.val_at(5)?,
			timestamp: cmp::min(r.val_at::<U256>(6)?, u64::max_value().into()).as_u64(),
			hash: keccak(r.as_raw()).into(),
		};

		Ok(blockheader)
	}
}

impl Encodable for Header {
	fn rlp_append(&self, s: &mut RlpStream) {
		self.stream_rlp(s);
	}
}

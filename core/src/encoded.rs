extern crate common_types as types;

use ethereum_types::H256;
use hash::keccak;
use types::*;

/// Owning header view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header(Vec<u8>);

impl Header {
    /// Get a borrowed header view onto the data.
    ///#[inline]
    //pub fn view(&self) -> HeaderView { view!(HeaderView, &self.0) }
    /// Consume the view and return the raw bytes.
    pub fn into_inner(self) -> Vec<u8> { self.0 }

    /// Returns the header hash.
    pub fn hash(&self) -> H256 { keccak(&self.0) }
    /// Number of this block.
    pub fn number(&self) -> BlockNumber { 0 }
}

/// Owning block body view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Body(Vec<u8>);

impl Body {}

/// Owning block view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block(Vec<u8>);

impl Block {
	/// Create a new owning block view. The raw bytes passed in must be an rlp-encoded block.
	pub fn new(raw: Vec<u8>) -> Self { Block(raw) }
}

// forwarders to borrowed header view.
impl Block {}

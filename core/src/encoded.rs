extern crate common_types as types;

use types::*;
use hash::keccak;
use ethereum_types::{H256};

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


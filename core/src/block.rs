use std::cmp;
use std::collections::HashSet;
use std::sync::Arc;

use bytes::Bytes;
use ethereum_types::{Address, Bloom, H256, U256};
use rlp::{encode_list, Decodable, DecoderError, Encodable, Rlp, RlpStream};
use transaction::Transaction;

/// A block, encoded as it is on the block chain.
#[derive(Default, Debug, Clone)]
pub struct Block {
    /// The header hash of this block.
    pub hash: H256,
    /// The transactions in this block.
    pub transactions: Vec<Transaction>,
}

impl Block {
    /// Returns true if the given bytes form a valid encoding of a block in RLP.
    pub fn is_good(b: &[u8]) -> bool { Rlp::new(b).as_val::<Block>().is_ok() }

    /// Get the RLP-encoding of the block with the seal.
    pub fn rlp(&self) -> Bytes {
        let mut block_rlp = RlpStream::new_list(2);
        block_rlp.append(&self.hash);
        block_rlp.append_list(&self.transactions);
        block_rlp.out()
    }

    pub fn hash(&self) -> H256 { self.hash }
}

impl Decodable for Block {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        if rlp.as_raw().len() != rlp.payload_info()?.total() {
            return Err(DecoderError::RlpIsTooBig);
        }
        if rlp.item_count()? != 2 {
            return Err(DecoderError::RlpIncorrectListLen);
        }
        Ok(Block {
            hash: rlp.val_at(0)?,
            transactions: rlp.list_at(1)?,
        })
    }
}

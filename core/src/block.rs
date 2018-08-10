use std::cmp;
use std::collections::HashSet;
use std::sync::Arc;

use bytes::Bytes;
use ethereum_types::{H256, U256, Address, Bloom};
use header::{Header};
use rlp::{Rlp, RlpStream, Encodable, Decodable, DecoderError, encode_list};
use transaction::{Transaction};

/// A block, encoded as it is on the block chain.
#[derive(Default, Debug, Clone)]
pub struct Block {
	/// The header of this block.
	pub header: Header,
	/// The transactions in this block.
	pub transactions: Vec<Transaction>,
}

impl Block {
	/// Returns true if the given bytes form a valid encoding of a block in RLP.
	pub fn is_good(b: &[u8]) -> bool {
		Rlp::new(b).as_val::<Block>().is_ok()
	}

	/// Get the RLP-encoding of the block with the seal.
	pub fn rlp_bytes(&self) -> Bytes {
		let mut block_rlp = RlpStream::new_list(2);
		block_rlp.append(&self.header);
		block_rlp.append_list(&self.transactions);
		block_rlp.out()
	}
}

impl Decodable for Block {
	fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
		if rlp.as_raw().len() != rlp.payload_info()?.total() {
			return Err(DecoderError::RlpIsTooBig);
		}
		if rlp.item_count()? != 3 {
			return Err(DecoderError::RlpIncorrectListLen);
		}
		Ok(Block {
			header: rlp.val_at(0)?,
			transactions: rlp.list_at(1)?,
		})
	}
}

use ethereum_types::{H256, U256};
use primitives::BlockNumber;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use std::mem::size_of;
use {Message, MsgId};

#[derive(Debug, PartialEq)]
pub enum BlockId {
    Hash(H256),
    Number(BlockNumber),
}

impl Encodable for BlockId {
    fn rlp_append(&self, stream: &mut RlpStream) {
        match self {
            BlockId::Hash(hash) => hash.rlp_append(stream),
            BlockId::Number(bn) => bn.rlp_append(stream),
        }
    }
}

impl Decodable for BlockId {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        if rlp.size() == size_of::<BlockNumber>() {
            Ok(BlockId::Number(BlockNumber::decode(rlp)?))
        } else if rlp.size() == size_of::<H256>() {
            Ok(BlockId::Hash(H256::decode(rlp)?))
        } else {
            Err(DecoderError::RlpInvalidLength)
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct GetBlockHeaders {
    pub block_id: BlockId,
    pub max_headers: usize,
    pub skip: usize,
    pub reverse: bool,
}

impl Message for GetBlockHeaders {
    fn msg_id(&self) -> MsgId { MsgId::GET_BLOCK_HEADERS }
}

impl Encodable for GetBlockHeaders {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream
            .begin_list(4)
            .append(&self.block_id)
            .append(&self.max_headers)
            .append(&self.skip)
            .append(&self.reverse);
    }
}

impl Decodable for GetBlockHeaders {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(GetBlockHeaders {
            block_id: rlp.val_at(0)?,
            max_headers: rlp.val_at(1)?,
            skip: rlp.val_at(2)?,
            reverse: rlp.val_at(3)?,
        })
    }
}

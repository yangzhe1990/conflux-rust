use ethereum_types::H256;
use primitives::BlockNumber;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use std::mem::size_of;
use Message;
use MsgId;

#[derive(Debug, PartialEq)]
pub struct GetBlockHeaders {
    pub hash: H256,
    pub max_blocks: u64,
}

impl Message for GetBlockHeaders {
    fn msg_id(&self) -> MsgId { MsgId::GET_BLOCK_HEADERS }
}

impl Encodable for GetBlockHeaders {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream
            .begin_list(2)
            .append(&self.hash)
            .append(&self.max_blocks);
    }
}

impl Decodable for GetBlockHeaders {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(GetBlockHeaders {
            hash: rlp.val_at(0)?,
            max_blocks: rlp.val_at(1)?,
        })
    }
}

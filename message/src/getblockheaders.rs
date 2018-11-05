use ethereum_types::H256;
use primitives::BlockNumber;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use std::mem::size_of;
use Message;
use MsgId;

#[derive(Debug, PartialEq)]
pub struct GetBlockHeaders {
    pub reqid: u16,
    pub hash: H256,
    pub max_blocks: u64,
}

impl Message for GetBlockHeaders {
    fn msg_id(&self) -> MsgId {
        MsgId::GET_BLOCK_HEADERS
    }
    fn set_request_id(&mut self, reqid: u16) {
        self.reqid = reqid
    }
}

impl Encodable for GetBlockHeaders {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream
            .begin_list(3)
            .append(&self.reqid)
            .append(&self.hash)
            .append(&self.max_blocks);
    }
}

impl Decodable for GetBlockHeaders {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        if rlp.item_count()? != 3 {
            return Err(DecoderError::RlpIncorrectListLen);
        }

        Ok(GetBlockHeaders {
            reqid: rlp.val_at(0)?,
            hash: rlp.val_at(1)?,
            max_blocks: rlp.val_at(2)?,
        })
    }
}

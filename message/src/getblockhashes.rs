use ethereum_types::H256;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use Message;
use MsgId;

#[derive(Debug, PartialEq)]
pub struct GetBlockHashes {
    reqid: u16,
    hash: H256,
    max_blocks: usize,
}

impl Message for GetBlockHashes {
    fn msg_id(&self) -> MsgId {
        MsgId::GET_BLOCK_HASHES
    }
    fn set_request_id(&mut self, reqid: u16) {
        self.reqid = reqid
    }
}

impl Encodable for GetBlockHashes {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream
            .begin_list(3)
            .append(&self.reqid)
            .append(&self.hash)
            .append(&self.max_blocks);
    }
}

impl Decodable for GetBlockHashes {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        if rlp.item_count()? != 3 {
            return Err(DecoderError::RlpIncorrectListLen);
        }

        Ok(GetBlockHashes {
            reqid: rlp.val_at(0)?,
            hash: rlp.val_at(1)?,
            max_blocks: rlp.val_at(2)?,
        })
    }
}

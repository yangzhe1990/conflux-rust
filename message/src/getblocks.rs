use ethereum_types::H256;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use Message;
use MsgId;

#[derive(Debug, PartialEq, Default)]
pub struct GetBlocks {
    pub reqid: u16,
    pub hashes: Vec<H256>,
}

impl Message for GetBlocks {
    fn msg_id(&self) -> MsgId {
        MsgId::GET_BLOCKS
    }
    fn set_request_id(&mut self, reqid: u16) {
        self.reqid = reqid
    }
}

impl Encodable for GetBlocks {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream
            .begin_list(2)
            .append(&self.reqid)
            .append_list(&self.hashes);
    }
}

impl Decodable for GetBlocks {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        if rlp.item_count()? != 2 {
            return Err(DecoderError::RlpIncorrectListLen);
        }

        Ok(GetBlocks {
            reqid: rlp.val_at(0)?,
            hashes: rlp.list_at(1)?,
        })
    }
}

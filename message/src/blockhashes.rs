use ethereum_types::{H256, U256};
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use {Message, MsgId};

#[derive(Debug, PartialEq)]
pub struct GetBlockHashesResponse {
    reqid: u16,
    hashes: Vec<H256>,
}

impl Message for GetBlockHashesResponse {
    fn msg_id(&self) -> MsgId {
        MsgId::GET_BLOCK_HASHES_RESPONSE
    }
}

impl Encodable for GetBlockHashesResponse {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream
            .begin_list(2)
            .append(&self.reqid)
            .append_list(&self.hashes);
    }
}

impl Decodable for GetBlockHashesResponse {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(GetBlockHashesResponse {
            reqid: rlp.val_at(0)?,
            hashes: rlp.list_at(1)?,
        })
    }
}

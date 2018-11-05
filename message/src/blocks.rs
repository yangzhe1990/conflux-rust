use primitives::Block;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use Message;
use MsgId;

#[derive(Debug, PartialEq, Default)]
pub struct GetBlocksResponse {
    pub reqid: u16,
    pub blocks: Vec<Block>,
}

impl Message for GetBlocksResponse {
    fn msg_id(&self) -> MsgId {
        MsgId::GET_BLOCKS_RESPONSE
    }
}

impl Encodable for GetBlocksResponse {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream
            .begin_list(2)
            .append(&self.reqid)
            .append_list(&self.blocks);
    }
}

impl Decodable for GetBlocksResponse {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(GetBlocksResponse {
            reqid: rlp.val_at(0)?,
            blocks: rlp.list_at(1)?,
        })
    }
}

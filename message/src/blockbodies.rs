use primitives::Block;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use Message;
use MsgId;

#[derive(Debug, PartialEq, Default)]
pub struct GetBlockBodiesResponse {
    pub reqid: u16,
    pub bodies: Vec<Block>,
}

impl Message for GetBlockBodiesResponse {
    fn msg_id(&self) -> MsgId {
        MsgId::GET_BLOCK_BODIES_RESPONSE
    }
}

impl Encodable for GetBlockBodiesResponse {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream
            .begin_list(2)
            .append(&self.reqid)
            .append_list(&self.bodies);
    }
}

impl Decodable for GetBlockBodiesResponse {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(GetBlockBodiesResponse {
            reqid: rlp.val_at(0)?,
            bodies: rlp.list_at(1)?,
        })
    }
}

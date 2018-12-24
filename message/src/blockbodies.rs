use primitives::Block;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use std::ops::{Deref, DerefMut};
use crate::Message;
use crate::MsgId;
use crate::RequestId;

#[derive(Debug, PartialEq, Default)]
pub struct GetBlockBodiesResponse {
    pub request_id: RequestId,
    pub bodies: Vec<Block>,
}

impl Message for GetBlockBodiesResponse {
    fn msg_id(&self) -> MsgId { MsgId::GET_BLOCK_BODIES_RESPONSE }
}

impl Deref for GetBlockBodiesResponse {
    type Target = RequestId;

    fn deref(&self) -> &Self::Target { &self.request_id }
}

impl DerefMut for GetBlockBodiesResponse {
    fn deref_mut(&mut self) -> &mut RequestId { &mut self.request_id }
}

impl Encodable for GetBlockBodiesResponse {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream
            .begin_list(2)
            .append(&self.request_id)
            .append_list(&self.bodies);
    }
}

impl Decodable for GetBlockBodiesResponse {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(GetBlockBodiesResponse {
            request_id: rlp.val_at(0)?,
            bodies: rlp.list_at(1)?,
        })
    }
}

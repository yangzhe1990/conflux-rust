use crate::{Message, MsgId, RequestId};
use primitives::block::RawBlock;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use std::ops::{Deref, DerefMut};

#[derive(Debug, PartialEq, Default)]
pub struct GetBlocksResponse {
    pub request_id: RequestId,
    pub blocks: Vec<RawBlock>,
}

impl Message for GetBlocksResponse {
    fn msg_id(&self) -> MsgId { MsgId::GET_BLOCKS_RESPONSE }
}

impl Deref for GetBlocksResponse {
    type Target = RequestId;

    fn deref(&self) -> &Self::Target { &self.request_id }
}

impl DerefMut for GetBlocksResponse {
    fn deref_mut(&mut self) -> &mut RequestId { &mut self.request_id }
}

impl Encodable for GetBlocksResponse {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream
            .begin_list(2)
            .append(&self.request_id)
            .append_list(&self.blocks);
    }
}

impl Decodable for GetBlocksResponse {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(GetBlocksResponse {
            request_id: rlp.val_at(0)?,
            blocks: rlp.list_at(1)?,
        })
    }
}

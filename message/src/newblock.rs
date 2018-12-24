use primitives::Block;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use crate::Message;
use crate::MsgId;

#[derive(Debug, PartialEq)]
pub struct NewBlock {
    pub block: Block,
}

impl Message for NewBlock {
    fn msg_id(&self) -> MsgId { MsgId::NEW_BLOCK }
}

impl Encodable for NewBlock {
    fn rlp_append(&self, stream: &mut RlpStream) { stream.append(&self.block); }
}

impl Decodable for NewBlock {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(NewBlock {
            block: rlp.as_val::<Block>()?,
        })
    }
}

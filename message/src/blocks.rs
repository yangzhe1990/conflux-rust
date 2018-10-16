use primitives::Block;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use Message;
use MsgId;

#[derive(Debug, PartialEq, Default)]
pub struct Blocks {
    pub blocks: Vec<Block>,
}

impl Message for Blocks {
    fn msg_id(&self) -> MsgId { MsgId::BLOCKS }
}

impl Encodable for Blocks {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream.append_list(&self.blocks);
    }
}

impl Decodable for Blocks {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(Blocks {
            blocks: rlp.as_list()?,
        })
    }
}

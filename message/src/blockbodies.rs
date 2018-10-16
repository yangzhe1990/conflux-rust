use primitives::Block;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use Message;
use MsgId;

#[derive(Debug, PartialEq, Default)]
pub struct BlockBodies {
    pub bodies: Vec<Block>,
}

impl Message for BlockBodies {
    fn msg_id(&self) -> MsgId { MsgId::BLOCK_BODIES }
}

impl Encodable for BlockBodies {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream.append_list(&self.bodies);
    }
}

impl Decodable for BlockBodies {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(BlockBodies {
            bodies: rlp.as_list()?,
        })
    }
}

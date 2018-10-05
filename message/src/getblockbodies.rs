use ethereum_types::H256;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use {Message, MsgId};

#[derive(Debug, PartialEq)]
pub struct GetBlockBodies {
    pub hashes: Vec<H256>,
}

impl Message for GetBlockBodies {
    fn msg_id(&self) -> MsgId { MsgId::GET_BLOCK_BODIES }
}

impl Encodable for GetBlockBodies {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream.append_list(&self.hashes);
    }
}

impl Decodable for GetBlockBodies {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(GetBlockBodies {
            hashes: rlp.as_list()?,
        })
    }
}

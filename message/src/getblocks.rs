use ethereum_types::H256;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use Message;
use MsgId;

#[derive(Debug, PartialEq, Default)]
pub struct GetBlocks {
    pub hashes: Vec<H256>,
}

impl Message for GetBlocks {
    fn msg_id(&self) -> MsgId { MsgId::GET_BLOCKS }
}

impl Encodable for GetBlocks {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream.append_list(&self.hashes);
    }
}

impl Decodable for GetBlocks {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(GetBlocks {
            hashes: rlp.as_list()?,
        })
    }
}

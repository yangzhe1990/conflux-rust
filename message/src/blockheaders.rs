use primitives::BlockHeader;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use Message;
use MsgId;

#[derive(Debug, PartialEq, Default)]
pub struct BlockHeaders {
    pub headers: Vec<BlockHeader>,
}

impl Message for BlockHeaders {
    fn msg_id(&self) -> MsgId { MsgId::BLOCK_HEADERS }
}

impl Encodable for BlockHeaders {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream.append_list(&self.headers);
    }
}

impl Decodable for BlockHeaders {
    fn decode(rlp: &Rlp) -> Result<BlockHeaders, DecoderError> {
        Ok(BlockHeaders {
            headers: rlp.as_list()?,
        })
    }
}

use ethereum_types::{H256, U256};
use primitives::BlockHeader;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use Payload;

#[derive(Debug, PartialEq)]
pub struct BlockHeaders {
    headers: Vec<BlockHeader>,
}

impl Encodable for BlockHeaders {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream.append_list(&self.headers);
    }
}

impl Decodable for BlockHeaders {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(BlockHeaders {
            headers: rlp.as_list()?,
        })
    }
}

use ethereum_types::H256;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use Payload;

#[derive(Debug, PartialEq)]
pub struct GetBlockBodies {
    hashes: Vec<H256>,
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

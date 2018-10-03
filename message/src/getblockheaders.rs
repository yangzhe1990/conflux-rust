use ethereum_types::{H256, U256};
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use Payload;

#[derive(Debug, PartialEq)]
pub struct GetBlockHeaders {
    block: H256,
    max_headers: usize,
    skip: usize,
    reverse: bool,
}

impl Encodable for GetBlockHeaders {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream
            .begin_list(4)
            .append(&self.block)
            .append(&self.max_headers)
            .append(&self.skip)
            .append(&self.reverse);
    }
}

impl Decodable for GetBlockHeaders {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(GetBlockHeaders {
            block: rlp.val_at(0)?,
            max_headers: rlp.val_at(1)?,
            skip: rlp.val_at(2)?,
            reverse: rlp.val_at(3)?,
        })
    }
}

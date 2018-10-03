use ethereum_types::{H256, U256};
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use Payload;

#[derive(Debug, PartialEq)]
pub struct GetBlockHashes {
    hash: H256,
    max_blocks: usize,
}

impl Payload for GetBlockHashes {
    fn command() -> u8 { 0x03 }
}

impl Encodable for GetBlockHashes {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream
            .begin_list(2)
            .append(&self.hash)
            .append(&self.max_blocks);
    }
}

impl Decodable for GetBlockHashes {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(GetBlockHashes {
            hash: rlp.val_at(0)?,
            max_blocks: rlp.val_at(1)?,
        })
    }
}

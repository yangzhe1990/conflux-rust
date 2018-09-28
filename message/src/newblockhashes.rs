use ethereum_types::H256;
use primitives::BlockNumber;
use rlp::{Rlp, RlpStream, Encodable, Decodable, DecoderError};
use Payload;

#[derive(Debug, PartialEq)]
pub struct BlockHash(BlockNumber, H256);

impl Encodable for BlockHash {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream.begin_list(2).append(&self.0).append(&self.1);
    }
}

impl Decodable for BlockHash {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(BlockHash(rlp.val_at::<BlockNumber>(0)?, rlp.val_at::<H256>(1)?))
    }
}

#[derive(Debug, PartialEq)]
pub struct NewBlockHashes {
    pub block_hashes: Vec<BlockHash>,
}

impl Payload for NewBlockHashes {
    fn command() -> u8 { 0x01 }
}

impl Encodable for NewBlockHashes {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream.append_list(&self.block_hashes);
    }
}

impl Decodable for NewBlockHashes {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(NewBlockHashes {
            block_hashes: rlp.as_list::<BlockHash>()?
        })
    }
}

use ethereum_types::{H256, U256};
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use {Message, MsgId};

#[derive(Debug, PartialEq)]
pub struct BlockHashes {
    hashes: Vec<H256>,
}

impl Message for BlockHashes {
    fn msg_id(&self) -> MsgId { MsgId::BLOCK_HASHES }
}

impl Encodable for BlockHashes {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream.append_list(&self.hashes);
    }
}

impl Decodable for BlockHashes {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(BlockHashes {
            hashes: rlp.as_list()?,
        })
    }
}

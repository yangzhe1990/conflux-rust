use ethereum_types::{H256, U256};
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use {Message, MsgId};

#[derive(Debug, PartialEq)]
pub struct GetBlockHashes {
    hash: H256,
    max_blocks: usize,
}

impl Message for GetBlockHashes {
    fn msg_id(&self) -> MsgId { MsgId::GET_BLOCK_HASHES }
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
            hash: rlp.val_at(1)?,
            max_blocks: rlp.val_at(2)?,
        })
    }
}

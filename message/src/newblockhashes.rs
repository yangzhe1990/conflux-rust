use crate::{Message, MsgId};
use ethereum_types::H256;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};

#[derive(Debug, PartialEq)]
pub struct NewBlockHashes {
    pub block_hashes: Vec<H256>,
}

impl Message for NewBlockHashes {
    fn msg_id(&self) -> MsgId { MsgId::NEW_BLOCK_HASHES }
}

impl Encodable for NewBlockHashes {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream.append_list(&self.block_hashes);
    }
}

impl Decodable for NewBlockHashes {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(NewBlockHashes {
            block_hashes: rlp.as_list()?,
        })
    }
}

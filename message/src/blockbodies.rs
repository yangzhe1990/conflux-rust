use ethereum_types::{H256, U256};
use primitives::{
    Block as InternalBlockBody, SignedTransaction, TransactionWithSignature,
};
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use {Message, MsgId};

pub type BlockBody = InternalBlockBody;

#[derive(Debug, PartialEq, Default)]
pub struct BlockBodies {
    pub bodies: Vec<Option<BlockBody>>,
}

impl Message for BlockBodies {
    fn msg_id(&self) -> MsgId { MsgId::BLOCK_BODIES }
}

impl Encodable for BlockBodies {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream.append_list(&self.bodies);
    }
}

impl Decodable for BlockBodies {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(BlockBodies {
            bodies: rlp.as_list()?,
        })
    }
}

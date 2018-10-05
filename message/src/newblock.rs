use ethereum_types::{H256, U256};
use primitives::{BlockHeader, SignedTransaction, TransactionWithSignature};
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use {Message, MsgId, BlockBody};

#[derive(Debug, PartialEq)]
pub struct NewBlock {
    pub total_difficulty: U256,
    pub header: BlockHeader,
    pub body: BlockBody,
}

impl Message for NewBlock {
    fn msg_id(&self) -> MsgId { MsgId::NEW_BLOCK }
}

impl Encodable for NewBlock {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream
            .begin_list(3)
            .append(&self.total_difficulty)
            .append(&self.header)
            .append(&self.body);
    }
}

impl Decodable for NewBlock {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(NewBlock {
            total_difficulty: rlp.val_at(0)?,
            header: rlp.val_at(1)?,
            body: rlp.val_at(2)?,
        })
    }
}

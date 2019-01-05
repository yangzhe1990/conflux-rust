use crate::{Message, MsgId};
use primitives::TransactionWithSignature;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};

#[derive(Debug, PartialEq)]
pub struct Transactions {
    pub transactions: Vec<TransactionWithSignature>,
}

impl Message for Transactions {
    fn msg_id(&self) -> MsgId { MsgId::TRANSACTIONS }
}

impl Encodable for Transactions {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream.append_list(&self.transactions);
    }
}

impl Decodable for Transactions {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(Transactions {
            transactions: rlp.as_list()?,
        })
    }
}

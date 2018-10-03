use primitives::TransactionWithSignature;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use Payload;

#[derive(Debug, PartialEq)]
pub struct Transactions {
    transactions: Vec<TransactionWithSignature>,
}

impl Payload for Transactions {
    fn command() -> u8 { 0x02 }
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

use ethereum_types::Address;
use rlp::{Rlp, RlpStream, Encodable, DecoderError, Decodable};

#[derive(Debug, Clone)]
pub struct Transaction {
    pub nonce: u64,
    pub value: f64,
    pub sender: Address,
    pub receiver: Address,
}

impl Decodable for Transaction {
    fn decode(r: &Rlp) -> Result<Self, DecoderError> {
        let mut tx = Transaction {
            nonce: r.val_at(0)?,
            value: 0.0,
            sender: r.val_at(2)?,
            receiver: r.val_at(3)?,
        };
        Ok(tx)
    }
}

impl Encodable for Transaction {
    fn rlp_append(&self, s: &mut RlpStream) {
    }
}

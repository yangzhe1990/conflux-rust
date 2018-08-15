use ethereum_types::Address;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};

#[derive(Debug, Clone)]
pub struct Transaction {
    pub nonce: u64,
    pub value: f64,
    pub sender: Address,
    pub receiver: Address,
}

impl Decodable for Transaction {
    fn decode(r: &Rlp) -> Result<Self, DecoderError> {
        let txn = Transaction {
            nonce: r.val_at(0)?,
            value: 0.0,
            sender: r.val_at(2)?,
            receiver: r.val_at(3)?,
        };
        Ok(txn)
    }
}

impl Encodable for Transaction {
    fn rlp_append(&self, s: &mut RlpStream) {}
}

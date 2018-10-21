use ethereum_types::U256;
use rlp::*;

pub struct Account {
    balance: U256,
    nonce: U256,
}

impl Decodable for Account {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        let vec= rlp.as_list()?;
        Ok(Account {
            balance: vec[0],
            nonce: vec[1],
        })
    }
}

impl Encodable for Account {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(2).append(&self.balance).append(&self.nonce);
    }
}

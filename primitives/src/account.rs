use ethereum_types::U256;
use rlp::*;

pub struct Account {
    pub balance: U256,
    pub nonce: U256,
}

impl Default for Account {
    fn default() -> Self {
        Account {
            balance: U256::zero(),
            nonce: U256::zero(),
        }
    }
}

impl Decodable for Account {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(Account {
            balance: rlp.val_at(0)?,
            nonce: rlp.val_at(1)?,
        })
    }
}

impl Encodable for Account {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream
            .begin_list(2)
            .append(&self.balance)
            .append(&self.nonce);
    }
}

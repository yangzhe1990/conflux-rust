use ethereum_types::{H256, U256};
use hash::KECCAK_EMPTY;
use rlp::*;

pub struct Account {
    pub balance: U256,
    pub nonce: U256,
    pub storage_root: H256,
    pub code_hash: H256,
}

impl Default for Account {
    fn default() -> Self {
        Account {
            balance: U256::zero(),
            nonce: U256::zero(),
            storage_root: H256::zero(),
            code_hash: KECCAK_EMPTY,
        }
    }
}

impl Decodable for Account {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(Account {
            balance: rlp.val_at(0)?,
            nonce: rlp.val_at(1)?,
            storage_root: rlp.val_at(2)?,
            code_hash: rlp.val_at(3)?,
        })
    }
}

impl Encodable for Account {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream
            .begin_list(4)
            .append(&self.balance)
            .append(&self.nonce)
            .append(&self.storage_root)
            .append(&self.code_hash);
    }
}

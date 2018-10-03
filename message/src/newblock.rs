use ethereum_types::{H256, U256};
use primitives::{BlockHeader, SignedTransaction, TransactionWithSignature};
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use Payload;

#[derive(Debug, PartialEq)]
pub struct NewBlock {
    pub total_difficulty: U256,
    pub header: BlockHeader,
    pub transactions: Vec<TransactionWithSignature>,
}

impl Payload for NewBlock {
    fn command() -> u8 { 0x08 }
}

impl Encodable for NewBlock {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream
            .begin_list(3)
            .append(&self.total_difficulty)
            .append(&self.header)
            .append_list(&self.transactions);
    }
}

impl Decodable for NewBlock {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(NewBlock {
            total_difficulty: rlp.val_at(0)?,
            header: rlp.val_at(1)?,
            transactions: rlp.list_at(2)?,
        })
    }
}

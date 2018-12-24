use ethereum_types::{H256, U256};
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use crate::BlockHeader;
use crate::SignedTransaction;
use crate::TransactionWithSignature;

/// A block, encoded as it is on the block chain.
#[derive(Default, Debug, Clone, PartialEq)]
pub struct Block {
    /// The header hash of this block.
    pub block_header: BlockHeader,
    /// The¡ transactions in this block.
    pub transactions: Vec<SignedTransaction>,
}

impl Block {
    pub fn hash(&self) -> H256 { self.block_header.hash() }

    pub fn total_gas(&self) -> U256 {
        let mut sum = U256::from(0);
        for t in &self.transactions {
            sum += t.gas;
        }
        sum
    }

    pub fn size(&self) -> usize {
        let mut ret = self.block_header.size();
        for t in &self.transactions {
            ret += t.size();
        }
        ret
    }
}

impl Encodable for Block {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream
            .begin_list(2)
            .append(&self.block_header)
            .append_list(&self.transactions);
    }
}

impl Decodable for Block {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        if rlp.as_raw().len() != rlp.payload_info()?.total() {
            return Err(DecoderError::RlpIsTooBig);
        }
        if rlp.item_count()? != 2 {
            return Err(DecoderError::RlpIncorrectListLen);
        }

        let transactions = rlp.list_at::<TransactionWithSignature>(1)?;
        let signed_transactions: Result<
            Vec<SignedTransaction>,
            DecoderError,
        > = transactions
            .into_iter()
            .map(|transaction| match transaction.recover_public() {
                Ok(public) => Ok(SignedTransaction::new(public, transaction)),
                Err(_) => {
                    Err(DecoderError::Custom("Cannot recover public key"))
                }
            })
            .collect();

        Ok(Block {
            block_header: rlp.val_at(0)?,
            transactions: signed_transactions?,
        })
    }
}

use bytes::Bytes;
use ethereum_types::H256;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use {SignedTransaction, TransactionWithSignature};

/// A block, encoded as it is on the block chain.
#[derive(Default, Debug, Clone, PartialEq)]
pub struct Block {
    /// The header hash of this block.
    pub hash: H256,
    /// The transactions in this block.
    pub transactions: Vec<SignedTransaction>,
}

impl Block {
    /// Returns true if the given bytes form a valid encoding of a block in RLP.
    pub fn is_good(b: &[u8]) -> bool { Rlp::new(b).as_val::<Block>().is_ok() }

    pub fn hash(&self) -> H256 { self.hash }
}

impl Encodable for Block {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream
            .begin_list(2)
            .append(&self.hash)
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
        let txs_with_sig: Vec<TransactionWithSignature> = rlp.list_at(1)?;
        let mut signed_txs: Vec<SignedTransaction> = Vec::new();
        for tx_with_sig in txs_with_sig {
            let public = tx_with_sig.recover_public();
            let mut signed_tx: SignedTransaction;
            match public {
                Ok(p) => {
                    signed_tx = SignedTransaction::new(p, tx_with_sig);
                }
                Err(_) => {
                    return Err(DecoderError::RlpIncorrectListLen);
                }
            }
            signed_txs.push(signed_tx);
        }

        Ok(Block {
            hash: rlp.val_at(0)?,
            transactions: signed_txs,
        })
    }
}

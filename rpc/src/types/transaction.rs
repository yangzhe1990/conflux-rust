use primitives::SignedTransaction;

use types::{H160, H256, U256};

#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub struct Transaction {
    pub hash: H256,
    pub nonce: U256,
    #[serde(rename = "blockHash")]
    pub block_hash: Option<H256>,
    #[serde(rename = "blockNumber")]
    pub block_number: Option<u64>,
    #[serde(rename = "transactionIndex")]
    pub transaction_index: Option<U256>,
    pub from: H160,
    pub to: Option<H160>,
    pub value: U256,
    #[serde(rename = "gasPrice")]
    pub gas_price: U256,
    pub gas: U256,
}

impl Transaction {
    pub fn from_signed(t: &SignedTransaction) -> Transaction {
        Transaction {
            hash: t.transaction.hash().into(),
            nonce: U256::from(t.nonce),
            block_hash: None,
            block_number: None,
            transaction_index: None,
            from: t.sender().into(),
            to: Some(t.receiver.into()),
            value: U256::from(t.value),
            gas_price: U256::from(t.gas_price),
            gas: U256::from(t.gas),
        }
    }
}

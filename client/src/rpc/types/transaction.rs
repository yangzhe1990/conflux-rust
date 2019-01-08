use crate::rpc::types::{H160, H256, U256};
use primitives::{transaction::Action, SignedTransaction};
use serde_derive::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transaction {
    pub hash: H256,
    pub nonce: U256,
    pub block_hash: Option<H256>,
    pub block_number: Option<u64>,
    pub transaction_index: Option<U256>,
    pub from: H160,
    pub to: Option<H160>,
    pub value: U256,
    pub gas_price: U256,
    pub gas: U256,
    pub data: Vec<u8>,
}

impl Transaction {
    #[allow(dead_code)]
    pub fn from_signed(t: &SignedTransaction) -> Transaction {
        Transaction {
            hash: t.transaction.hash().into(),
            nonce: U256::from(t.nonce),
            block_hash: None,
            block_number: None,
            transaction_index: None,
            from: t.sender().into(),
            to: match t.action {
                Action::Create => None,
                Action::Call(ref address) => Some(address.clone().into()),
            },
            value: U256::from(t.value),
            gas_price: U256::from(t.gas_price),
            gas: U256::from(t.gas),
            data: t.data.clone(),
        }
    }
}

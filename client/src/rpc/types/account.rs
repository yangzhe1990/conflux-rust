use crate::rpc::types::{BlockTransactions, U256};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub balance: U256,
    pub transactions: BlockTransactions,
}

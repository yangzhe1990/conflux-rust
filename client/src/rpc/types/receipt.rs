use crate::rpc::types::U256;
use serde_derive::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    /// The total gas used in the block following execution of the transaction.
    pub gas_used: U256,
    /// Transaction outcome.
    pub outcome_status: u8,
}

impl Receipt {
    pub fn new(gas_used: U256, outcome_status: u8) -> Receipt {
        Receipt {
            gas_used,
            outcome_status,
        }
    }
}

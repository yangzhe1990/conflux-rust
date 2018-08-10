use ethereum_types::Address;

pub struct Transaction {
    pub nonce: u64,
    pub value: f64,
    pub sender: Address,
    pub receiver: Address,
}

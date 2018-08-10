use std::collections::HashMap;

/// Single account in the system
pub struct Account {
    balance: f64,
    nonce: u64,
}

/// Representation of the entire state of all accounts in the system.
pub struct State {
    accounts: HashMap<Address, Account>,
    checkpoints: Vec<HashMap<Address, Account>>,
}

impl State {
    pub fn new() -> State {
        State {
            accounts: HashMap::new(),
            checkpoints: Vec::new(),
        }
    }

    pub fn execute(&mut self) {}
}

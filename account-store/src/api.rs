use ethereum_types::{Address};
use super::AccountEntry;

pub trait AccountStoreInterface: Sync + Send {
    /// update state of account ac, insert if ac not exists
    /// return old entry if ac exists, otherwise None
    fn update_entry(&self, ac: &Address, ent: AccountEntry) -> Option<AccountEntry>;
    
    /// update value of account ac, if ac not exists, do nothing and return false
    fn update_value(&self, ac: &Address, val: f64) -> bool;

    /// update nonce of account ac, if ac not exists, do nothing and return false
    fn set_nonce(&self, ac: &Address, nonce: u64) -> bool;

    /// get value of account
    fn get_value(&self, ac: &Address) -> Option<f64>;

    /// get nonce of account
    fn get_nonce(&self, ac: &Address) -> Option<u64>;

    /// increment nonce of account by 1
    fn inc_nonce(&self, ac: &Address) -> Option<u64>;    

    /// check whether account exists
    fn exist(&self, ac: &Address) -> bool;
}

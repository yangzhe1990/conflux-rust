use ethereum_types::{Address};
use super::AccountEntry;

pub trait AccountStoreInterface {
    /// update state of account ac, insert ac if not exists
    /// return true if inserts, otherwise false
    fn update_entry(ac: &Address, ent: &AccountEntry) -> bool;
    
    /// update value of account ac, if ac not exists, do nothing and return false
    fn update_value(ac: &Address, val: f64) -> bool;

    /// update nonce of account ac, if ac not exists, do nothing and return false
    fn set_nonce(ac: &Address, nonce: u64) -> bool;

    /// get value of account
    fn get_value(ac: &Address) -> Option<f64>;

    /// get nonce of account
    fn get_nonce(ac: &Address) -> Option<u64>;

    /// increment nonce of account by 1, if ac not exists, do nothing and return false
    fn inc_nonce(ac: &Address) -> bool;    

    /// check whether account exists
    fn exist(ac: &Address) -> bool;
}

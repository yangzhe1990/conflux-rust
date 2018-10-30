use super::super::state::*;

use super::super::state_manager::*;
use execution::EpochId;

pub struct State<'a> {
    manager: &'a StateManager,
    // TODO(yz): implement.
}

impl<'a> StateTrait<'a> for State<'a> {
    fn get(&'a mut self, access_key: &[u8]) -> &'a mut Vec<u8> {
        unimplemented!()
    }

    fn set(&mut self, access_key: &[u8], value: &[u8]) { unimplemented!() }

    fn delete(&mut self, access_key: &[u8]) -> Vec<u8> { unimplemented!() }

    fn delete_all<T>(&mut self, access_key_prefix: &[u8], removed_kvs: T) {
        unimplemented!()
    }

    fn commit(epoch_id: EpochId) { unimplemented!() }

    fn drop() { unimplemented!() }
}

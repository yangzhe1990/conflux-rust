use super::super::state::*;

use super::super::state_union::*;
use execution::EpochId;

pub struct State<'a> {
    union: &'a StateUnion,
    target_epoch_id: EpochId,
    // TODO(yz): implement.
}

impl<'a> StateTrait<'a> for State<'a> {
    fn load_from_union(union: &'a StateUnion, epoch_id: EpochId) -> Self {
        unimplemented!()
    }

    fn create_for_transaction_execution(
        union: &'a StateUnion, parent_epoch_id: EpochId, epoch_id: EpochId,
    ) -> Self {
        let mut state = Self::load_from_union(union, parent_epoch_id);
        state.set_target_epoch_id(epoch_id);
        state
    }

    fn get(&'a mut self, access_key: &[u8]) -> &'a mut Vec<u8> {
        unimplemented!()
    }

    fn set(&mut self, access_key: &[u8], value: &[u8]) { unimplemented!() }

    fn delete(&mut self, access_key: &[u8]) -> Vec<u8> { unimplemented!() }

    fn delete_all<T>(&mut self, access_key_prefix: &[u8], removed_kvs: T) {
        unimplemented!()
    }

    fn commit() { unimplemented!() }

    fn drop() { unimplemented!() }
}

impl<'a> State<'a> {
    fn set_target_epoch_id(&mut self, epoch_id: EpochId) -> &mut Self {
        self.target_epoch_id = epoch_id;
        self
    }
}

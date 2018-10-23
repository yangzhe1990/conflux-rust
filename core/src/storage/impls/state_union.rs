use super::super::state_union::*;

use super::{super::state::*, merkle_patricia_trie::*};
use execution::EpochId;
use snapshot::snapshot::Snapshot;

pub struct StateUnion {
    delta_trie: MultiVersionMerklePatriciaTrie,
    // TODO(yz): implement.
}

impl StateUnionTrait for StateUnion {
    fn load_from_snapshot(snapshot: &Snapshot) -> Self { unimplemented!() }

    fn make_snapshot(&self, end_block_id: EpochId) -> Snapshot {
        unimplemented!()
    }

    fn get_state_at<'a>(&'a self, epoch_id: EpochId) -> State<'a> {
        State::load_from_union(self, epoch_id)
    }

    fn get_state_for_transaction_execution<'a>(
        &'a self, epoch_id: EpochId, parent_epoch_id: EpochId,
    ) -> State<'a> {
        State::create_for_transaction_execution(self, parent_epoch_id, epoch_id)
    }

    fn drop_state_outside(&self, end_block_id: EpochId) { unimplemented!() }
}

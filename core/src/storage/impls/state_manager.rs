use super::super::state_manager::*;

use super::{super::state::*, merkle_patricia_trie::*};
use execution::EpochId;
use snapshot::snapshot::Snapshot;

pub struct StateManager {
    delta_trie: MultiVersionMerklePatriciaTrie,
    // TODO(yz): implement.
}

impl StateManager {
    fn new() -> Self { unimplemented!() }
}

impl StateManagerTrait for StateManager {
    fn from_snapshot(snapshot: &Snapshot) -> Self { unimplemented!() }

    fn make_snapshot(&self, epoch_id: EpochId) -> Snapshot { unimplemented!() }

    fn get_state_at(&self, epoch_id: EpochId) -> State { unimplemented!() }

    fn drop_state_outside(&self, epoch_id: EpochId) { unimplemented!() }
}

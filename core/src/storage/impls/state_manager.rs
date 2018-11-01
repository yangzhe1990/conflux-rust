use super::super::state_manager::*;

use super::{
    super::state::*,
    merkle_patricia_trie::{data_structure::*, *},
};
use primitives::EpochId;
use snapshot::snapshot::Snapshot;

#[derive(Default)]
pub struct StateManager {
    delta_trie: MultiVersionMerklePatriciaTrie,
}

impl StateManager {
    pub(super) fn get_trie_memory_allocator(
        &self,
    ) -> &MultiVersionMerklePatriciaTrie {
        &self.delta_trie
    }

    pub(super) fn commit_state_root(
        &self, epoch_id: EpochId, root_node: MaybeNodeRef,
    ) {
        if root_node != MaybeNodeRef::NULL_NODE {
            self.delta_trie.commit_epoch_root(
                epoch_id,
                Option::<NodeRef>::from(root_node).unwrap(),
            );
        }
    }

    pub fn new() -> Self { unimplemented!() }
}

impl StateManagerTrait for StateManager {
    fn from_snapshot(snapshot: &Snapshot) -> Self { unimplemented!() }

    fn make_snapshot(&self, epoch_id: EpochId) -> Snapshot { unimplemented!() }

    fn get_state_at(&self, epoch_id: EpochId) -> State {
        State::new(self, self.delta_trie.get_root_at_epoch(epoch_id).into())
    }

    fn drop_state_outside(&self, epoch_id: EpochId) { unimplemented!() }
}

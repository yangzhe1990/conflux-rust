use super::super::state_manager::*;

use super::{
    super::state::*,
    merkle_patricia_trie::{data_structure::*, *},
};
use ethkey::KeyPair;
use get_account;
use primitives::{Account, EpochId};
use rlp::encode;
use secret_store::SecretStore;
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

    pub fn initialize(&self, genesis: EpochId, secret_store: &SecretStore) {
        let mut state = self.get_state_at(genesis);
        let kp = KeyPair::from_secret(
            "46b9e861b63d3509c88b7817275a30d22d62c8cd8fa6486ddee35ef0d8e0495f"
                .parse()
                .unwrap(),
        )
        .unwrap();
        let addr = kp.address();
        let account = Account {
            balance: 1_000_000_000.into(),
            nonce: 0.into(),
        };
        state.set(addr.as_ref(), encode(&account).as_ref()).unwrap();
        state.commit(genesis);
        secret_store.insert(kp);
        assert_eq!(
            get_account(&self.get_state_at(genesis), &addr)
                .map(|account| account.balance)
                .unwrap(),
            1_000_000_000.into()
        );
    }
}

impl StateManagerTrait for StateManager {
    fn from_snapshot(snapshot: &Snapshot) -> Self { unimplemented!() }

    fn make_snapshot(&self, epoch_id: EpochId) -> Snapshot { unimplemented!() }

    fn get_state_at(&self, epoch_id: EpochId) -> State {
        State::new(self, self.delta_trie.get_root_at_epoch(epoch_id).into())
    }

    fn drop_state_outside(&self, epoch_id: EpochId) { unimplemented!() }
}

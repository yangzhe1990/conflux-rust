use super::super::state_manager::*;

use super::{
    super::{super::db::COL_DELTA_TRIE, state::*},
    errors::*,
    merkle_patricia_trie::{
        node_memory_manager::{MaybeNodeRef, NodeRef},
        row_number::*,
        *,
    },
};
use crate::{
    ext_db::SystemDB, snapshot::snapshot::Snapshot, statedb::StorageKey,
};
use ethereum_types::{H256, U256};
use ethkey::KeyPair;
use kvdb::{DBTransaction, DBValue};
use primitives::{Account, Block, BlockHeaderBuilder, EpochId};
use rlp::encode;
use secret_store::SecretStore;
use std::{
    io, str,
    sync::{Arc, Mutex, MutexGuard},
};

#[derive(Default)]
pub struct AtomicCommit {
    pub row_number: RowNumber,
}

pub struct AtomicCommitTransaction<'a> {
    pub info: MutexGuard<'a, AtomicCommit>,
    pub transaction: DBTransaction,
}

pub struct StateManager {
    delta_trie: MultiVersionMerklePatriciaTrie,
    pub db: Arc<SystemDB>,
    commit_lock: Mutex<AtomicCommit>,
}

impl StateManager {
    pub(super) fn get_delta_trie(&self) -> &MultiVersionMerklePatriciaTrie {
        &self.delta_trie
    }

    pub fn start_commit(&self) -> AtomicCommitTransaction {
        AtomicCommitTransaction {
            info: self.commit_lock.lock().unwrap(),
            transaction: self.db.key_value().transaction(),
        }
    }

    fn load_state_root_node_ref_from_db(
        &self, epoch_id: EpochId,
    ) -> Result<MaybeNodeRef> {
        let db_key_result = Self::parse_row_number(
            self.db.key_value().get(
                COL_DELTA_TRIE,
                [
                    "state_root_db_key_for_epoch_id_".as_bytes(),
                    epoch_id.as_ref(),
                ]
                .concat()
                .as_slice(),
            ),
        )?;
        match db_key_result {
            Some(db_key) => Ok(self
                .delta_trie
                .loaded_root_at_epoch(epoch_id, db_key)
                .into()),
            None => Ok(MaybeNodeRef::NULL_NODE),
        }
    }

    fn get_state_root_node_ref(
        &self, epoch_id: EpochId,
    ) -> Result<MaybeNodeRef> {
        let node_ref = self.delta_trie.get_root_at_epoch(epoch_id).into();
        if node_ref == MaybeNodeRef::NULL_NODE {
            self.load_state_root_node_ref_from_db(epoch_id)
        } else {
            Ok(node_ref)
        }
    }

    // TODO(ming): Should prevent from committing at existing epoch because
    // otherwise the overwritten trie nodes can not be reachable from db.
    // The current codebase overwrites because it didn't check if the state
    // root is already computed, which should eventually be optimized out.
    // TODO(ming): Use self.get_state_root_node_ref(epoch_id).
    pub(super) fn mpt_commit_state_root(
        &self, epoch_id: EpochId, root_node: MaybeNodeRef,
    ) {
        if root_node != MaybeNodeRef::NULL_NODE {
            self.delta_trie.set_epoch_root(
                epoch_id,
                Option::<NodeRef>::from(root_node).unwrap(),
            );
        }
    }

    fn parse_row_number(
        x: io::Result<Option<DBValue>>,
    ) -> Result<Option<RowNumberUnderlyingType>> {
        Ok(match x?.as_ref() {
            None => None,
            Some(row_number_bytes) => Some(
                unsafe { str::from_utf8_unchecked(row_number_bytes.as_ref()) }
                    .parse::<RowNumberUnderlyingType>()?,
            ),
        })
    }

    pub fn new(db: Arc<SystemDB>) -> Self {
        let row_number = Self::parse_row_number(
            db.key_value()
                .get(COL_DELTA_TRIE, "last_row_number".as_bytes()),
        )
        // unwrap() on new is fine.
        .unwrap()
        .unwrap_or_default();

        Self {
            delta_trie: MultiVersionMerklePatriciaTrie::new(
                db.key_value().clone(),
            ),
            db: db,
            commit_lock: Mutex::new(AtomicCommit {
                row_number: RowNumber { value: row_number },
            }),
        }
    }

    pub fn initialize(&self, secret_store: &SecretStore) -> Block {
        let mut state = self.get_state_at(H256::default()).unwrap();
        let kp = KeyPair::from_secret(
            "46b9e861b63d3509c88b7817275a30d22d62c8cd8fa6486ddee35ef0d8e0495f"
                .parse()
                .unwrap(),
        )
        .unwrap();
        let addr = kp.address();
        let account = Account::new_empty_with_balance(
            &addr,
            &U256::from_dec_str("1000000000000000000").expect("Not overflow"),
            &0.into(),
        );
        state
            .set(
                StorageKey::new_account_key(&addr).as_ref(),
                encode(&account).as_ref(),
            )
            .unwrap();
        let root = state.compute_state_root().unwrap();
        let genesis = Block {
            block_header: BlockHeaderBuilder::new()
                .with_deferred_state_root(root)
                .build(),
            transactions: Vec::new(),
        };
        trace!("Genesis Block:{:?}", genesis);
        state.commit(genesis.block_header.hash()).unwrap();
        secret_store.insert(kp);
        genesis
    }
}

impl StateManagerTrait for StateManager {
    fn from_snapshot(snapshot: &Snapshot) -> Self { unimplemented!() }

    fn make_snapshot(&self, epoch_id: EpochId) -> Snapshot { unimplemented!() }

    fn get_state_at(&self, epoch_id: EpochId) -> Result<State> {
        Ok(State::new(self, self.get_state_root_node_ref(epoch_id)?))
    }

    fn contains_state(&self, epoch_id: EpochId) -> bool {
        self.get_state_at(epoch_id).unwrap().does_exist()
    }

    fn drop_state_outside(&self, epoch_id: EpochId) { unimplemented!() }
}

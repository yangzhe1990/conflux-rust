use super::super::state::*;

use super::{
    super::state_manager::*,
    errors::*,
    merkle_patricia_trie::{
        data_structure::*, merkle::*,
        MultiVersionMerklePatriciaTrie,
    },
};
use primitives::EpochId;

pub struct State<'a> {
    manager: &'a StateManager,
    // FIXME: change to memory allocator.
    allocator: &'a MultiVersionMerklePatriciaTrie,
    root_node: MaybeNodeRef,
    owned_node_set: Option<OwnedNodeSet>,
    dirty: bool,
}

impl<'a> State<'a> {
    pub fn new(manager: &'a StateManager, root_node: MaybeNodeRef) -> Self {
        Self {
            manager: manager,
            allocator: manager.get_trie_memory_allocator(),
            root_node: root_node,
            owned_node_set: Some(Default::default()),
            dirty: false,
        }
    }

    fn pre_modification(&mut self) {
        if !self.dirty {
            self.dirty = true
        }
        self.allocator.node_memory_allocator.enlarge().unwrap();
    }

    // FIXME: move to data_structure mod
    fn get_root_node(&self) -> Result<NodeRef> {
        let node: Option<NodeRef> = self.root_node.into();
        match node {
            None => Err(ErrorKind::MPTKeyNotFound.into()),
            Some(node_ref) => Ok(node_ref),
        }
    }

    fn get_or_create_root_node(&mut self) -> Result<NodeRef> {
        if self.root_node == MaybeNodeRef::NULL_NODE {
            let allocator = self.allocator.get_allocator();
            let (mut root_cow, entry) = CowNodeRef::new_uninitialized_node(
                &allocator,
                self.owned_node_set.as_mut().unwrap(),
            )?;
            // Insert empty node.
            entry.insert(Default::default());

            self.root_node = root_cow.into_child();
        }

        self.get_root_node()
    }
}

impl<'a> Drop for State<'a> {
    fn drop(&mut self) {
        if self.dirty {
            panic!("State is dirty however is not committed before free.");
        }
    }
}

impl<'a> StateTrait<'a> for State<'a> {
    fn get(&self, access_key: &[u8]) -> Result<Box<[u8]>> {
        // Get won't create any new nodes so it's fine to pass an empty
        // owned_node_set.
        let mut empty_owned_node_set: Option<OwnedNodeSet> =
            Some(Default::default());
        let value = SubTrieVisitor::new(
            self.allocator,
            self.get_root_node()?,
            &mut empty_owned_node_set,
        )
        .get(access_key);

        value
    }

    fn set(&mut self, access_key: &[u8], value: &[u8]) -> Result<()> {
        self.pre_modification();

        self.root_node = SubTrieVisitor::new(
            self.allocator,
            self.get_or_create_root_node()?,
            &mut self.owned_node_set,
        )
        .set(access_key, value)?;

        Ok(())
    }

    fn delete(&mut self, access_key: &[u8]) -> Result<Vec<u8>> {
        self.pre_modification();

        let (old_value, _, root_node) = SubTrieVisitor::new(
            self.allocator,
            self.get_or_create_root_node()?,
            &mut self.owned_node_set,
        )
        .delete(access_key)?;
        self.root_node = root_node;
        Ok(old_value)
    }

    fn delete_all<T>(
        &mut self, access_key_prefix: &[u8], removed_kvs: T,
    ) -> Result<()> {
        unimplemented!()
    }

    fn commit(&mut self, epoch_id: EpochId) -> MerkleHash {
        self.dirty = false;

        let maybe_root_node: Option<NodeRef> = self.root_node.into();
        let merkle = match maybe_root_node {
            None => MERKLE_NULL_NODE,
            Some(root_node) => {
                let allocator = self.allocator.get_allocator();
                let mut cow_root = CowNodeRef::new(
                    root_node,
                    self.owned_node_set.as_ref().unwrap(),
                );
                let mut trie_node_root = NodeMemoryAllocator::node_as_mut(
                    &allocator,
                    &mut cow_root.node_ref,
                );
                let merkle = cow_root.get_or_compute_merkle(
                    self.allocator,
                    &allocator,
                    self.owned_node_set.as_ref().unwrap(),
                    trie_node_root,
                );
                cow_root.into_child();

                merkle
            }
        };

        self.manager.commit_state_root(epoch_id, self.root_node);

        merkle
    }
}

/// CowNodeRef facilities access and modification to trie nodes in multi-version
/// MPT. It offers read-only access to the original trie node, and creates an
/// unique owned trie node once there is any modification. The ownership is
/// maintained centralized in owned_node_set which is passed into many methods
/// as argument. When CowNodeRef is created from an owned node, the ownership is
/// transferred into the CowNodeRef object. The visitor of MPT makes sure that
/// ownership of any trie node is not transferred more than once at the same
/// time.
pub struct CowNodeRef {
    owned: bool,
    pub node_ref: NodeRefDeltaMpt,
}

impl CowNodeRef {
    pub fn new_uninitialized_node<'a>(
        allocator: AllocatorRefRefDeltaMpt<'a>,
        owned_node_set: &mut OwnedNodeSet,
    ) -> Result<(Self, SlabVacantEntryDeltaMpt<'a>)>
    {
        let (node_ref, new_entry) =
            NodeMemoryManagerDeltaMpt::new_node(allocator)?;
        owned_node_set.insert(node_ref.clone());

        Ok((
            Self {
                owned: true,
                node_ref: node_ref,
            },
            new_entry,
        ))
    }

    pub fn new(
        node_ref: NodeRefDeltaMpt, owned_node_set: &OwnedNodeSet,
    ) -> Self {
        Self {
            owned: owned_node_set.contains(&node_ref),
            node_ref: node_ref,
        }
    }

    /// Take the value out of Self. Self is safe to drop.
    pub fn take(&mut self) -> Self {
        let ret = Self {
            owned: self.owned,
            node_ref: self.node_ref.clone(),
        };

        self.owned = false;
        ret
    }
}

impl Drop for CowNodeRef {
    /// Assert that the CowNodeRef doesn't own something.
    fn drop(&mut self) {
        assert_eq!(false, self.owned);
    }
}

impl CowNodeRef {
    pub fn get_owned(&self) -> bool { self.owned }

    fn convert_to_owned<'a>(
        &mut self, node_memory_manager: &'a NodeMemoryManagerDeltaMpt,
        allocator: AllocatorRefRefDeltaMpt<'a>,
        owned_node_set: &mut OwnedNodeSet,
    ) -> Result<Option<SlabVacantEntryDeltaMpt<'a>>>
    {
        if self.owned {
            Ok(None)
        } else {
            // Similar to Self::new_uninitialized_node().
            let (node_ref, new_entry) =
                NodeMemoryManagerDeltaMpt::new_node(&allocator)?;
            owned_node_set.insert(node_ref.clone());
            self.node_ref = node_ref;
            self.owned = true;

            Ok(Some(new_entry))
        }
    }

    // FIXME: the trie node obtained from CowNodeRef should be invalidated at
    // the same time of delete_node and into_child.
    pub fn delete_node(
        mut self, node_memory_manager: &NodeMemoryManagerDeltaMpt,
    ) {
        if self.owned {
            node_memory_manager.free_node(&mut self.node_ref);
            self.owned = false;
        }
    }

    // FIXME: maybe forbid calling for un-owned node?
    pub fn into_child(mut self) -> Option<NodeRefDeltaMptCompact> {
        if self.owned {
            self.owned = false;
            Some(self.node_ref.clone().into())
        } else {
            None
        }
    }

    fn commit_dirty_recurse_into_children(
        &mut self, trie: &MultiVersionMerklePatriciaTrie,
        owned_node_set: &mut OwnedNodeSet, trie_node: &mut TrieNodeDeltaMpt,
        commit_transaction: &mut AtomicCommitTransaction,
        cache_manager: &mut CacheManagerDeltaMpt,
        allocator_ref: AllocatorRefRefDeltaMpt,
    ) -> Result<()>
    {
        for (i, node_ref_mut) in trie_node.children_table.iter_mut() {
            let node_ref = node_ref_mut.clone();
            let mut cow_child_node = Self::new(node_ref.into(), owned_node_set);
            if cow_child_node.get_owned() {
                let trie_node = unsafe {
                    trie.get_node_memory_manager().dirty_node_as_mut_unchecked(
                        allocator_ref,
                        &mut cow_child_node.node_ref,
                    )
                }?;
                let was_owned = cow_child_node.commit_dirty_recursively(
                    trie,
                    owned_node_set,
                    trie_node,
                    commit_transaction,
                    cache_manager,
                    allocator_ref,
                )?;

                // An owned child TrieNode now have a new NodeRef.
                *node_ref_mut = cow_child_node.into_child().unwrap();
            }
        }
        Ok(())
    }

    fn set_merkle(
        &mut self, children_merkles: MaybeMerkleTableRef,
        trie_node: &mut TrieNodeDeltaMpt,
    ) -> MerkleHash
    {
        let path_merkle = compute_merkle(
            trie_node.compressed_path_ref(),
            children_merkles,
            trie_node.value_as_slice(),
        );
        trie_node.merkle_hash = path_merkle;

        path_merkle
    }

    /// Get if unowned, compute if owned.
    pub fn get_or_compute_merkle(
        &mut self, trie: &MultiVersionMerklePatriciaTrie,
        owned_node_set: &mut OwnedNodeSet,
        allocator_ref: AllocatorRefRefDeltaMpt,
    ) -> Result<MerkleHash>
    {
        if self.owned {
            let trie_node = unsafe {
                trie.get_node_memory_manager().dirty_node_as_mut_unchecked(
                    allocator_ref,
                    &mut self.node_ref,
                )?
            };
            let children_merkles = self.get_or_compute_children_merkles(
                trie,
                owned_node_set,
                trie_node,
                allocator_ref,
            )?;

            let merkle = self.set_merkle(children_merkles.as_ref(), trie_node);

            Ok(merkle)
        } else {
            let trie_node = trie
                .get_node_memory_manager()
                .node_as_ref(allocator_ref, &self.node_ref)?;
            Ok(trie_node.merkle_hash)
        }
    }

    fn get_or_compute_children_merkles(
        &mut self, trie: &MultiVersionMerklePatriciaTrie,
        owned_node_set: &mut OwnedNodeSet, trie_node: &mut TrieNodeDeltaMpt,
        allocator_ref: AllocatorRefRefDeltaMpt,
    ) -> Result<MaybeMerkleTable>
    {
        match trie_node.children_table.get_children_count() {
            0 => Ok(None),
            _ => {
                let mut merkles = ChildrenMerkleTable::default();
                for (i, maybe_node_ref_mut) in
                    trie_node.children_table.iter_non_skip()
                {
                    match maybe_node_ref_mut {
                        None => merkles[i as usize] = MERKLE_NULL_NODE,
                        Some(node_ref_mut) => {
                            let mut cow_child_node = Self::new(
                                (*node_ref_mut).into(),
                                owned_node_set,
                            );
                            let merkle = cow_child_node.get_or_compute_merkle(
                                trie,
                                owned_node_set,
                                allocator_ref,
                            )?;
                            // There is no change to the child_node so the
                            // return value is dropped.
                            cow_child_node.into_child();

                            merkles[i as usize] = merkle;
                        }
                    }
                }
                Ok(Some(merkles))
            }
        }
    }

    // FIXME: unit test.
    pub fn iterate_internal<TrieNodeRef: Deref<Target = TrieNodeDeltaMpt>>(
        &self, owned_node_set: &OwnedNodeSet,
        trie: &MultiVersionMerklePatriciaTrie,
        allocator_ref: AllocatorRefRefDeltaMpt, trie_node: TrieNodeRef,
        key_prefix: CompressedPathRaw, values: &mut Vec<(Vec<u8>, Box<[u8]>)>,
    ) -> Result<()>
    {
        if trie_node.has_value() {
            assert_eq!(key_prefix.end_mask(), 0);
            values.push((
                key_prefix.path_slice().to_vec(),
                trie_node.value_clone().unwrap(),
            ));
        }

        let children_table = trie_node.children_table.clone();
        // Free the lock for trie_node.
        // FIXME: try to share the lock.
        drop(trie_node);
        for (i, node_ref) in children_table.iter() {
            let mut cow_child_node =
                Self::new((*node_ref).into(), owned_node_set);
            let child_node = trie
                .get_node_memory_manager()
                .node_as_ref(allocator_ref, &mut cow_child_node.node_ref)?;
            let key_prefix = CompressedPathRaw::concat(
                &key_prefix,
                &child_node.compressed_path_ref(),
            );
            cow_child_node.iterate_internal(
                owned_node_set,
                trie,
                allocator_ref,
                child_node,
                key_prefix,
                values,
            )?;
        }

        Ok(())
    }

    /// Recursively commit dirty nodes.
    pub fn commit_dirty_recursively(
        &mut self, trie: &MultiVersionMerklePatriciaTrie,
        owned_node_set: &mut OwnedNodeSet, trie_node: &mut TrieNodeDeltaMpt,
        commit_transaction: &mut AtomicCommitTransaction,
        cache_manager: &mut CacheManagerDeltaMpt,
        allocator_ref: AllocatorRefRefDeltaMpt,
    ) -> Result<bool>
    {
        if self.owned {
            self.commit_dirty_recurse_into_children(
                trie,
                owned_node_set,
                trie_node,
                commit_transaction,
                cache_manager,
                allocator_ref,
            )?;

            let db_key = commit_transaction.info.row_number.value;
            commit_transaction.transaction.put(
                COL_DELTA_TRIE,
                commit_transaction.info.row_number.to_string().as_bytes(),
                trie_node.rlp_bytes().as_slice(),
            );
            commit_transaction.info.row_number =
                commit_transaction.info.row_number.get_next()?;

            let slot = match &self.node_ref {
                NodeRefDeltaMpt::Dirty { index } => *index,
                _ => unsafe { unreachable_unchecked() },
            };
            owned_node_set.remove(&self.node_ref);
            self.node_ref = NodeRefDeltaMpt::Committed { db_key: db_key };
            owned_node_set.insert(self.node_ref.clone());

            cache_manager.insert_to_node_ref_map_and_call_cache_access(
                db_key,
                slot,
                trie.get_node_memory_manager(),
            );

            Ok(true)
        } else {
            Ok(false)
        }
    }

    // Upgrade a trie node ref to mut when this object ownes the trie_node,
    // however the precondition is unchecked.
    unsafe fn owned_trie_node_ref_to_mut_unchecked(
        &mut self, trie_node: &TrieNodeDeltaMpt,
    ) -> &mut TrieNodeDeltaMpt {
        &mut *(trie_node as *const TrieNodeDeltaMpt as *mut TrieNodeDeltaMpt)
    }

    pub unsafe fn delete_value_unchecked_if_owned(
        &mut self, trie_node: &TrieNodeDeltaMpt,
    ) -> Box<[u8]> {
        if self.owned {
            self.owned_trie_node_ref_to_mut_unchecked(trie_node)
                .delete_value_unchecked()
        } else {
            trie_node.value_clone().unwrap()
        }
    }

    pub fn cow_set_compressed_path(
        &mut self, node_memory_manager: &NodeMemoryManagerDeltaMpt,
        owned_node_set: &mut OwnedNodeSet, path: CompressedPathRaw,
        trie_node: &TrieNodeDeltaMpt,
    ) -> Result<()>
    {
        let allocator = node_memory_manager.get_allocator();
        let copied = self.convert_to_owned(
            node_memory_manager,
            &allocator,
            owned_node_set,
        )?;
        match copied {
            None => {
                unsafe { self.owned_trie_node_ref_to_mut_unchecked(trie_node) }
                    .set_compressed_path(path);
            }
            Some(new_entry) => {
                new_entry.insert(unsafe {
                    trie_node.copy_and_replace_fields(None, Some(path), None)
                });
            }
        }
        Ok(())
    }

    pub fn cow_delete_value_unchecked(
        &mut self, node_memory_manager: &NodeMemoryManagerDeltaMpt,
        owned_node_set: &mut OwnedNodeSet, trie_node: &TrieNodeDeltaMpt,
    ) -> Result<Box<[u8]>>
    {
        let allocator = node_memory_manager.get_allocator();
        let copied = self.convert_to_owned(
            node_memory_manager,
            &allocator,
            owned_node_set,
        )?;
        Ok(match copied {
            None => unsafe {
                self.owned_trie_node_ref_to_mut_unchecked(trie_node)
                    .delete_value_unchecked()
            },
            Some(new_entry) => {
                new_entry.insert(unsafe {
                    trie_node.copy_and_replace_fields(Some(None), None, None)
                });
                trie_node.value_clone().unwrap()
            }
        })
    }

    pub fn cow_replace_value_valid(
        &mut self, node_memory_manager: &NodeMemoryManagerDeltaMpt,
        owned_node_set: &mut OwnedNodeSet, trie_node: &TrieNodeDeltaMpt,
        value: &[u8],
    ) -> Result<Option<Box<[u8]>>>
    {
        let allocator = node_memory_manager.get_allocator();
        let copied = self.convert_to_owned(
            node_memory_manager,
            &allocator,
            owned_node_set,
        )?;
        Ok(match copied {
            None => {
                unsafe { self.owned_trie_node_ref_to_mut_unchecked(trie_node) }
                    .replace_value_valid(value)
            }
            Some(new_entry) => {
                new_entry.insert(unsafe {
                    trie_node.copy_and_replace_fields(
                        Some(Some(value)),
                        None,
                        None,
                    )
                });
                trie_node.value_clone()
            }
        })
    }

    pub unsafe fn cow_replace_child_unchecked(
        &mut self, node_memory_manager: &NodeMemoryManagerDeltaMpt,
        owned_node_set: &mut OwnedNodeSet, trie_node: &TrieNodeDeltaMpt,
        child_index: u8, child_node: NodeRefDeltaMptCompact,
    ) -> Result<()>
    {
        let allocator = node_memory_manager.get_allocator();
        let copied = self.convert_to_owned(
            node_memory_manager,
            &allocator,
            owned_node_set,
        )?;
        match copied {
            None => {
                self.owned_trie_node_ref_to_mut_unchecked(trie_node)
                    .replace_child_unchecked(child_index, child_node);
            }
            Some(new_entry) => {
                let mut new_trie_node =
                    trie_node.copy_and_replace_fields(None, None, None);
                new_trie_node.replace_child_unchecked(child_index, child_node);
                new_entry.insert(new_trie_node);
            }
        }

        Ok(())
    }

    pub unsafe fn cow_delete_child_unchecked(
        &mut self, node_memory_manager: &NodeMemoryManagerDeltaMpt,
        owned_node_set: &mut OwnedNodeSet, trie_node: &TrieNodeDeltaMpt,
        child_index: u8,
    ) -> Result<()>
    {
        let allocator = node_memory_manager.get_allocator();
        let copied = self.convert_to_owned(
            node_memory_manager,
            &allocator,
            owned_node_set,
        )?;
        match copied {
            None => {
                self.owned_trie_node_ref_to_mut_unchecked(trie_node)
                    .delete_child_unchecked(child_index);
            }
            Some(new_entry) => {
                let mut new_trie_node =
                    trie_node.copy_and_replace_fields(None, None, None);
                new_trie_node.delete_child_unchecked(child_index);
                new_entry.insert(new_trie_node);
            }
        }

        Ok(())
    }

    // FIXME: How to replace duplicated code?
    pub unsafe fn cow_add_new_child_unchecked(
        &mut self, node_memory_manager: &NodeMemoryManagerDeltaMpt,
        owned_node_set: &mut OwnedNodeSet, trie_node: &TrieNodeDeltaMpt,
        child_index: u8, child_node: NodeRefDeltaMptCompact,
    ) -> Result<()>
    {
        let allocator = node_memory_manager.get_allocator();
        let copied = self.convert_to_owned(
            node_memory_manager,
            &allocator,
            owned_node_set,
        )?;
        match copied {
            None => {
                self.owned_trie_node_ref_to_mut_unchecked(trie_node)
                    .add_new_child_unchecked(child_index, child_node);
            }
            Some(new_entry) => {
                let mut new_trie_node =
                    trie_node.copy_and_replace_fields(None, None, None);
                new_trie_node.add_new_child_unchecked(child_index, child_node);
                new_entry.insert(new_trie_node);
            }
        }

        Ok(())
    }
}

use super::{
    super::{
        super::{
            errors::*,
            state::OwnedNodeSet,
            state_manager::{AtomicCommitTransaction, COL_DELTA_TRIE},
        },
        node_memory_manager::*,
        MultiVersionMerklePatriciaTrie,
    },
    merkle::*,
    *,
};
use rlp::*;
use std::{hint::unreachable_unchecked, ops::Deref};

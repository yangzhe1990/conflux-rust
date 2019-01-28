pub struct SubTrieVisitor<'trie> {
    root: CowNodeRef,

    trie_ref: &'trie MultiVersionMerklePatriciaTrie,

    /// We use ReturnAfterUse because only one SubTrieVisitor(the deepest) can
    /// hold the mutable reference of owned_node_set.
    owned_node_set: ReturnAfterUse<'trie, OwnedNodeSet>,
}

impl<'trie> SubTrieVisitor<'trie> {
    pub fn new(
        trie_ref: &'trie MultiVersionMerklePatriciaTrie, root: NodeRefDeltaMpt,
        owned_node_set: &'trie mut Option<OwnedNodeSet>,
    ) -> Self
    {
        Self {
            trie_ref: trie_ref,
            root: CowNodeRef::new(root, owned_node_set.as_ref().unwrap()),
            owned_node_set: ReturnAfterUse::new(owned_node_set),
        }
    }

    fn new_visitor_for_subtree<'a>(
        &'a mut self, child_node: NodeRefDeltaMpt,
    ) -> SubTrieVisitor<'a>
    where 'trie: 'a {
        let trie_ref = self.trie_ref;
        let cow_child_node =
            CowNodeRef::new(child_node, self.owned_node_set.get_ref());
        SubTrieVisitor {
            trie_ref: trie_ref,
            root: cow_child_node,
            owned_node_set: ReturnAfterUse::new_from_origin(
                &mut self.owned_node_set,
            ),
        }
    }

    fn get_trie_ref(&self) -> &'trie MultiVersionMerklePatriciaTrie {
        self.trie_ref
    }

    fn node_memory_manager(&self) -> &'trie NodeMemoryManagerDeltaMpt {
        &self.get_trie_ref().get_node_memory_manager()
    }

    fn memory_allocator_borrow(
        &self,
    ) -> AllocatorRef<'trie, CacheAlgoDataDeltaMpt> {
        self.node_memory_manager().get_allocator()
    }

    fn get_trie_node<'a>(
        &self, key: KeyPart, allocator_ref: AllocatorRefRefDeltaMpt<'a>,
    ) -> Result<
        Option<
            GuardedValue<
                RwLockWriteGuard<'a, CacheManagerDeltaMpt>,
                &'a TrieNodeDeltaMpt,
            >,
        >,
    >
    where 'trie: 'a {
        let mut node_ref = self.root.node_ref.clone();
        let mut key = key;
        loop {
            let trie_node = self
                .node_memory_manager()
                .node_as_ref(allocator_ref, &node_ref)?;
            match trie_node.walk::<Read>(key) {
                WalkStop::Arrived => {
                    return Ok(Some(trie_node));
                }
                WalkStop::Descent {
                    key_remaining,
                    child_index: _,
                    child_node,
                } => {
                    node_ref = child_node;
                    key = key_remaining;
                }
                _ => {
                    return Ok(None);
                }
            }
        }
    }

    pub fn get(&self, key: KeyPart) -> Result<Option<Box<[u8]>>> {
        let allocator = self.node_memory_manager().get_allocator();
        let maybe_trie_node = self.get_trie_node(key, &allocator)?;

        Ok(match maybe_trie_node {
            None => None,
            Some(trie_node) => trie_node.value_clone(),
        })
    }

    pub fn get_merkle_hash_wo_compressed_path(
        &self, key: KeyPart,
    ) -> Result<Option<MerkleHash>> {
        let allocator = self.node_memory_manager().get_allocator();
        let maybe_trie_node = self.get_trie_node(key, &allocator)?;

        match maybe_trie_node {
            None => Ok(None),
            Some(trie_node) => {
                if trie_node.get_compressed_path_size() == 0 {
                    Ok(Some(trie_node.merkle_hash))
                } else {
                    let maybe_value = trie_node.value_clone();
                    let merkles = match trie_node
                        .children_table
                        .get_children_count()
                    {
                        0 => None,
                        _ => {
                            let mut merkles = ChildrenMerkleTable::default();
                            let children_table =
                                trie_node.children_table.clone();
                            drop(trie_node);
                            for (i, maybe_node_ref) in
                                children_table.iter_non_skip()
                            {
                                merkles[i as usize] = match maybe_node_ref {
                                    None => super::merkle::MERKLE_NULL_NODE,
                                    Some(node_ref) => self
                                        .trie_ref
                                        .get_merkle(Some((*node_ref).into()))?
                                        .unwrap(),
                                };
                            }

                            Some(merkles)
                        }
                    };

                    Ok(Some(compute_node_merkle(
                        merkles.as_ref(),
                        maybe_value.as_ref().map(|value| value.as_ref()),
                    )))
                }
            }
        }
    }

    /// The visitor can only be used once to modify.
    /// Returns (deleted value, is root node replaced, the current root node for
    /// the subtree).
    pub fn delete(
        mut self, key: KeyPart,
    ) -> Result<(Option<Box<[u8]>>, bool, Option<NodeRefDeltaMptCompact>)> {
        let node_memory_manager = self.node_memory_manager();
        let allocator = node_memory_manager.get_allocator();
        let mut node_cow = self.root.take();
        // TODO(yz): be compliant to borrow rule and avoid duplicated

        // FIXME: map_split?
        let mut trie_node_mut = node_memory_manager
            .node_as_mut(&allocator, &mut node_cow.node_ref)?;
        match trie_node_mut.walk::<Read>(key) {
            WalkStop::Arrived => {
                // If value doesn't exists, returns invalid key error.
                let result = trie_node_mut.check_delete_value();
                if result.is_err() {
                    return Ok((None, false, node_cow.into_child()));
                }
                let action = result.unwrap();
                match action {
                    TrieNodeAction::Delete => {
                        // The current node is going to be dropped if owned.
                        let value = unsafe {
                            node_cow.delete_value_unchecked_if_owned(
                                &mut trie_node_mut,
                            )
                        };
                        // FIXME: deal with deletion while holding the
                        // trie_node_mut.
                        node_cow.delete_node(self.node_memory_manager());
                        Ok((Some(value), true, None))
                    }
                    TrieNodeAction::MergePath {
                        child_index,
                        child_node_ref,
                    } => {
                        // The current node is going to be dropped if owned.
                        let value = unsafe {
                            node_cow.delete_value_unchecked_if_owned(
                                &mut trie_node_mut,
                            )
                        };

                        let new_path: CompressedPathRaw;
                        let mut child_node_cow: CowNodeRef;
                        {
                            let path_prefix = CompressedPathRaw::new(
                                trie_node_mut
                                    .compressed_path_ref()
                                    .path_slice(),
                                trie_node_mut.compressed_path_ref().end_mask(),
                            );
                            // COW modify child,
                            child_node_cow = CowNodeRef::new(
                                child_node_ref,
                                self.owned_node_set.get_ref(),
                            );
                            // FIXME: try to share the lock.
                            drop(trie_node_mut);
                            new_path = node_memory_manager
                                .node_as_ref(
                                    &allocator,
                                    &child_node_cow.node_ref,
                                )?
                                .path_prepended(path_prefix, child_index);
                        }
                        // FIXME: error processing for OOM.
                        let mut child_trie_node = node_memory_manager
                            .node_as_mut(
                                &allocator,
                                &mut child_node_cow.node_ref,
                            )?;
                        child_node_cow.cow_set_compressed_path(
                            &node_memory_manager,
                            self.owned_node_set.get_mut(),
                            new_path,
                            &mut child_trie_node,
                        )?;

                        // FIXME: how to represent that trie_node_mut is invalid
                        // after call to node_mut.delete_node?
                        // FIXME: trie_node_mut should be considered ref of
                        // node_mut.
                        node_cow.delete_node(self.node_memory_manager());

                        Ok((Some(value), true, child_node_cow.into_child()))
                    }
                    TrieNodeAction::Modify => {
                        let node_changed = !node_cow.get_owned();
                        let value = node_cow.cow_delete_value_unchecked(
                            &node_memory_manager,
                            self.owned_node_set.get_mut(),
                            &mut trie_node_mut,
                        )?;

                        Ok((Some(value), node_changed, node_cow.into_child()))
                    }
                    _ => unsafe { unreachable_unchecked() },
                }
            }
            WalkStop::Descent {
                key_remaining,
                child_node,
                child_index,
            } => {
                drop(trie_node_mut);
                let result = self
                    .new_visitor_for_subtree(child_node)
                    .delete(key_remaining);
                if result.is_err() {
                    node_cow.into_child();
                    return result;
                }
                let mut trie_node_mut = node_memory_manager
                    .node_as_mut(&allocator, &mut node_cow.node_ref)?;
                let (value, child_replaced, new_child_node) = result.unwrap();
                if child_replaced {
                    let action = trie_node_mut
                        .check_replace_or_delete_child_action(
                            child_index,
                            new_child_node,
                        );
                    match action {
                        TrieNodeAction::MergePath {
                            child_index,
                            child_node_ref,
                        } => {
                            // FIXME: how to reuse code?
                            let new_path: CompressedPathRaw;
                            let mut child_node_cow: CowNodeRef;
                            {
                                let path_prefix = CompressedPathRaw::new(
                                    trie_node_mut
                                        .compressed_path_ref()
                                        .path_slice(),
                                    trie_node_mut
                                        .compressed_path_ref()
                                        .end_mask(),
                                );
                                // COW modify child,
                                child_node_cow = CowNodeRef::new(
                                    child_node_ref,
                                    self.owned_node_set.get_ref(),
                                );
                                drop(trie_node_mut);
                                new_path = node_memory_manager
                                    .node_as_ref(
                                        &allocator,
                                        &child_node_cow.node_ref,
                                    )?
                                    .path_prepended(path_prefix, child_index);
                            }
                            let mut child_trie_node = node_memory_manager
                                .node_as_mut(
                                    &allocator,
                                    &mut child_node_cow.node_ref,
                                )?;
                            child_node_cow.cow_set_compressed_path(
                                &node_memory_manager,
                                self.owned_node_set.get_mut(),
                                new_path,
                                &mut child_trie_node,
                            )?;

                            // FIXME: how to represent that trie_node_mut is
                            // invalid after call to node_mut.delete_node?
                            // FIXME: trie_node_mut should be considered ref of
                            // node_mut.
                            node_cow.delete_node(self.node_memory_manager());

                            Ok((value, true, child_node_cow.into_child()))
                        }
                        TrieNodeAction::DeleteChildrenTable => {
                            let node_changed = !node_cow.get_owned();
                            node_cow.cow_delete_children_table(
                                &node_memory_manager,
                                self.owned_node_set.get_mut(),
                                &mut trie_node_mut,
                            )?;

                            Ok((value, node_changed, node_cow.into_child()))
                        }
                        TrieNodeAction::Modify => unsafe {
                            let node_changed = !node_cow.get_owned();
                            match new_child_node {
                                None => {
                                    node_cow.cow_delete_child_unchecked(
                                        &node_memory_manager,
                                        self.owned_node_set.get_mut(),
                                        &mut trie_node_mut,
                                        child_index,
                                    )?;
                                }
                                Some(replacement) => {
                                    node_cow.cow_replace_child_unchecked(
                                        &node_memory_manager,
                                        self.owned_node_set.get_mut(),
                                        &mut trie_node_mut,
                                        child_index,
                                        replacement,
                                    )?;
                                }
                            }

                            Ok((value, node_changed, node_cow.into_child()))
                        },
                        _ => unsafe { unreachable_unchecked() },
                    }
                } else {
                    Ok((value, false, node_cow.into_child()))
                }
            }

            _ => Ok((None, false, node_cow.into_child())),
        }
    }

    /// The visitor can only be used once to modify.
    /// Returns (deleted value, is root node replaced, the current root node for
    /// the subtree).
    pub fn delete_all(
        mut self, key: KeyPart, key_remaining: KeyPart,
    ) -> Result<(
        Option<Vec<(Vec<u8>, Box<[u8]>)>>,
        bool,
        Option<NodeRefDeltaMptCompact>,
    )> {
        let node_memory_manager = self.node_memory_manager();
        let allocator = node_memory_manager.get_allocator();
        let mut node_cow = self.root.take();
        // TODO(yz): be compliant to borrow rule and avoid duplicated

        // FIXME: map_split?
        let trie_node_mut = node_memory_manager
            .node_as_mut(&allocator, &mut node_cow.node_ref)?;

        let key_prefix: CompressedPathRaw;
        match trie_node_mut.walk::<Read>(key_remaining) {
            WalkStop::ChildNotFound {
                key_remaining,
                child_index,
            } => return Ok((None, false, node_cow.into_child())),
            WalkStop::Arrived => {
                // To enumerate the subtree.
                key_prefix = key.into();
            }
            WalkStop::PathDiverted {
                key_child_index,
                key_remaining,
                matched_path,
                unmatched_child_index,
                unmatched_path_remaining,
            } => {
                if key_child_index.is_none() {
                    return Ok((None, false, node_cow.into_child()));
                }
                // To enumerate the subtree.
                key_prefix =
                    CompressedPathRaw::concat(&key, &unmatched_path_remaining);
            }
            WalkStop::Descent {
                key_remaining,
                child_node,
                child_index,
            } => {
                drop(trie_node_mut);
                let result = self
                    .new_visitor_for_subtree(child_node)
                    .delete_all(key, key_remaining);
                if result.is_err() {
                    node_cow.into_child();
                    return result;
                }
                let mut trie_node_mut = node_memory_manager
                    .node_as_mut(&allocator, &mut node_cow.node_ref)?;
                let (value, child_replaced, new_child_node) = result.unwrap();
                // FIXME: copied from delete(). Try to reuse code?
                if child_replaced {
                    let action = trie_node_mut
                        .check_replace_or_delete_child_action(
                            child_index,
                            new_child_node,
                        );
                    match action {
                        TrieNodeAction::MergePath {
                            child_index,
                            child_node_ref,
                        } => {
                            let new_path: CompressedPathRaw;
                            let mut child_node_cow: CowNodeRef;
                            {
                                let path_prefix = CompressedPathRaw::new(
                                    trie_node_mut
                                        .compressed_path_ref()
                                        .path_slice(),
                                    trie_node_mut
                                        .compressed_path_ref()
                                        .end_mask(),
                                );
                                // COW modify child,
                                child_node_cow = CowNodeRef::new(
                                    child_node_ref,
                                    self.owned_node_set.get_ref(),
                                );
                                drop(trie_node_mut);
                                new_path = node_memory_manager
                                    .node_as_ref(
                                        &allocator,
                                        &child_node_cow.node_ref,
                                    )?
                                    .path_prepended(path_prefix, child_index);
                            }
                            let mut child_trie_node = node_memory_manager
                                .node_as_mut(
                                    &allocator,
                                    &mut child_node_cow.node_ref,
                                )?;
                            child_node_cow.cow_set_compressed_path(
                                &node_memory_manager,
                                self.owned_node_set.get_mut(),
                                new_path,
                                &mut child_trie_node,
                            )?;

                            // FIXME: how to represent that trie_node_mut is
                            // invalid after call to node_mut.delete_node?
                            // FIXME: trie_node_mut should be considered ref of
                            // node_mut.
                            node_cow.delete_node(self.node_memory_manager());

                            return Ok((
                                value,
                                true,
                                child_node_cow.into_child(),
                            ));
                        }
                        TrieNodeAction::DeleteChildrenTable => {
                            let node_changed = !node_cow.get_owned();
                            node_cow.cow_delete_children_table(
                                &node_memory_manager,
                                self.owned_node_set.get_mut(),
                                &mut trie_node_mut,
                            )?;

                            return Ok((
                                value,
                                node_changed,
                                node_cow.into_child(),
                            ));
                        }
                        TrieNodeAction::Modify => unsafe {
                            let node_changed = !node_cow.get_owned();
                            match new_child_node {
                                None => {
                                    node_cow.cow_delete_child_unchecked(
                                        &node_memory_manager,
                                        self.owned_node_set.get_mut(),
                                        &mut trie_node_mut,
                                        child_index,
                                    )?;
                                }
                                Some(replacement) => {
                                    node_cow.cow_replace_child_unchecked(
                                        &node_memory_manager,
                                        self.owned_node_set.get_mut(),
                                        &mut trie_node_mut,
                                        child_index,
                                        replacement,
                                    )?;
                                }
                            }

                            return Ok((
                                value,
                                node_changed,
                                node_cow.into_child(),
                            ));
                        },
                        _ => unsafe { unreachable_unchecked() },
                    }
                } else {
                    return Ok((value, false, node_cow.into_child()));
                }
            }
        }

        let mut old_values = vec![];
        node_cow.iterate_internal(
            self.owned_node_set.get_ref(),
            self.get_trie_ref(),
            &allocator,
            trie_node_mut,
            key_prefix,
            &mut old_values,
        )?;
        node_cow.delete_node(self.node_memory_manager());

        Ok((Some(old_values), true, None))
    }

    // In a method we visit node one or 2 times but borrow-checker prevent
    // holding and access other fields so it's visited multiple times.
    // FIXME: Check if we did something like this.
    // It's correct behavior if we first
    // access this node, recurse into children, then access it again, because
    // the accesses in subtree and other threads may in extreme cases evict
    // this node from cache.

    // Assume that the obtained TrieNode will be set valid value (non-empty)
    // later on.
    /// Insert a valid value into MPT.
    /// The visitor can only be used once to modify.
    unsafe fn insert_checked_value<'key>(
        mut self, key: KeyPart<'key>, value: &[u8],
    ) -> Result<(bool, NodeRefDeltaMptCompact)> {
        let node_memory_manager = self.node_memory_manager();
        let allocator = node_memory_manager.get_allocator();
        let mut node_cow = self.root.take();
        // TODO(yz): be compliant to borrow rule and avoid duplicated

        let mut trie_node_mut = node_memory_manager
            .node_as_mut(&allocator, &mut node_cow.node_ref)?;
        match trie_node_mut.walk::<Write>(key) {
            WalkStop::Arrived => {
                let node_changed = !node_cow.get_owned();
                node_cow.cow_replace_value_valid(
                    &node_memory_manager,
                    self.owned_node_set.get_mut(),
                    &mut trie_node_mut,
                    value,
                )?;

                Ok((node_changed, node_cow.into_child().unwrap()))
            }
            WalkStop::Descent {
                key_remaining,
                child_node,
                child_index,
            } => {
                drop(trie_node_mut);
                let result = self
                    .new_visitor_for_subtree(child_node)
                    .insert_checked_value(key_remaining, value);
                if result.is_err() {
                    node_cow.into_child();
                    return result;
                }
                let mut trie_node_mut = node_memory_manager
                    .node_as_mut(&allocator, &mut node_cow.node_ref)?;
                let (child_changed, new_child_node) = result.unwrap();

                if child_changed {
                    let node_changed = !node_cow.get_owned();
                    node_cow.cow_replace_child_unchecked(
                        &node_memory_manager,
                        self.owned_node_set.get_mut(),
                        &mut trie_node_mut,
                        child_index,
                        new_child_node,
                    )?;

                    Ok((node_changed, node_cow.into_child().unwrap()))
                } else {
                    Ok((false, node_cow.into_child().unwrap()))
                }
            }
            WalkStop::PathDiverted {
                key_child_index,
                key_remaining,
                matched_path,
                unmatched_child_index,
                unmatched_path_remaining,
            } => {
                // create a new node to replace self with compressed
                // path = matched_path, modify current
                // node with the remaining path, and attach it as child to the
                // replacement node create a new node for
                // insertion (if key_remaining is non-empty), set it to child,
                // with key_remaining.
                let (new_node_cow, new_node_entry) =
                    CowNodeRef::new_uninitialized_node(
                        &allocator,
                        self.owned_node_set.get_mut(),
                    )?;
                let mut new_node = TrieNode::default();
                // set compressed path.
                new_node.set_compressed_path(matched_path);

                node_cow.cow_set_compressed_path(
                    &node_memory_manager,
                    self.owned_node_set.get_mut(),
                    unmatched_path_remaining,
                    &mut trie_node_mut,
                )?;

                // It's safe because we know that this is the first child.
                new_node.set_first_child_unchecked(
                    unmatched_child_index,
                    // It's safe to unwrap because we know that it's not none.
                    node_cow.into_child().unwrap(),
                );

                // TODO(yz): remove duplicated code.
                match key_child_index {
                    None => {
                        // Insert value at the current node
                        new_node.replace_value_valid(value);
                    }
                    Some(child_index) => {
                        // TODO(yz): Maybe create CowNodeRef on NULL then
                        // cow_set_value then set path.
                        let (child_node_cow, child_node_entry) =
                            CowNodeRef::new_uninitialized_node(
                                &allocator,
                                self.owned_node_set.get_mut(),
                            )?;
                        let mut new_child_node = TrieNode::default();
                        // set compressed path.
                        new_child_node.copy_compressed_path(
                            CompressedPathRef {
                                path_slice: key_remaining,
                                end_mask: 0,
                            },
                        );
                        new_child_node.replace_value_valid(value);
                        child_node_entry.insert(new_child_node);

                        // It's safe because it's guaranteed that
                        // key_child_index != unmatched_child_index
                        new_node.add_new_child_unchecked(
                            child_index,
                            // It's safe to unwrap here because it's not null
                            // node.
                            child_node_cow.into_child().unwrap(),
                        );
                    }
                }
                new_node_entry.insert(new_node);
                Ok((true, new_node_cow.into_child().unwrap()))
            }
            WalkStop::ChildNotFound {
                key_remaining,
                child_index,
            } => {
                // TODO(yz): Maybe create CowNodeRef on NULL then cow_set_value
                // then set path.
                let (child_node_cow, child_node_entry) =
                    CowNodeRef::new_uninitialized_node(
                        &allocator,
                        self.owned_node_set.get_mut(),
                    )?;
                let mut new_child_node = TrieNode::default();
                // set compressed path.
                new_child_node.copy_compressed_path(CompressedPathRef {
                    path_slice: key_remaining,
                    end_mask: 0,
                });
                new_child_node.replace_value_valid(value);
                child_node_entry.insert(new_child_node);

                let node_changed = !node_cow.get_owned();
                node_cow.cow_add_new_child_unchecked(
                    &node_memory_manager,
                    self.owned_node_set.get_mut(),
                    &mut trie_node_mut,
                    child_index,
                    child_node_cow.into_child().unwrap(),
                )?;

                Ok((node_changed, node_cow.into_child().unwrap()))
            }
        }
    }

    pub fn set(self, key: KeyPart, value: &[u8]) -> Result<NodeRefDeltaMpt> {
        TrieNodeDeltaMpt::check_key_size(key)?;
        TrieNodeDeltaMpt::check_value_size(value)?;
        let new_root;
        unsafe {
            new_root = self.insert_checked_value(key, value)?.1;
        }
        Ok(new_root.into())
    }
}

use super::{
    super::{super::state::OwnedNodeSet, MultiVersionMerklePatriciaTrie},
    *,
};

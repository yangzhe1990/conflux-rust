pub type ActualSlabIndex = u32;

pub type VacantEntry<'a, TrieNode> = super::slab::VacantEntry<
    'a,
    TrieNode,
    // FIXME: implement EntryTrait for TrieNode.
    super::slab::Entry<TrieNode>,
>;

type Allocator<CacheAlgoDataT> = Slab<
    TrieNode<CacheAlgoDataT>,
    super::slab::Entry<TrieNode<CacheAlgoDataT>>,
>;
pub type AllocatorRef<'a, CacheAlgoDataT> =
    RwLockReadGuard<'a, Allocator<CacheAlgoDataT>>;
pub type AllocatorRefRef<'a, CacheAlgoDataT> =
    &'a AllocatorRef<'a, CacheAlgoDataT>;

pub type GLFUPosT = u32;
pub type CacheAlgorithmDeltaMpt = RecentLFU<GLFUPosT, DeltaMptDbKey>;
pub type CacheAlgoDataDeltaMpt =
    <CacheAlgorithmDeltaMpt as CacheAlgorithm>::CacheAlgoData;

pub type TrieNodeDeltaMpt = TrieNode<CacheAlgoDataDeltaMpt>;

pub type SlabVacantEntryDeltaMpt<'a> = VacantEntry<'a, TrieNodeDeltaMpt>;
type AllocatorDeltaMpt =
    Slab<TrieNodeDeltaMpt, super::slab::Entry<TrieNodeDeltaMpt>>;
pub type AllocatorRefRefDeltaMpt<'a> =
    &'a AllocatorRef<'a, CacheAlgoDataDeltaMpt>;

pub type NodeMemoryManagerDeltaMpt =
    NodeMemoryManager<CacheAlgoDataDeltaMpt, CacheAlgorithmDeltaMpt>;
pub type CacheManagerDeltaMpt =
    CacheManager<CacheAlgoDataDeltaMpt, CacheAlgorithmDeltaMpt>;

impl CacheIndexTrait for DeltaMptDbKey {}

// TODO: On performance, each access may requires a lock because of calling
// TODO: cache algorithm & cache eviction & TrieNode slab alloc/delete
// TODO: & noderefmap update. The read & write can not be easily broken
// TODO: down because of dependency. Calling cache algorithm always
// TODO: requires write lock to algorithm. cache hit updates can be
// TODO: batched locally. The caller knows whether a node is hit. But the
// TODO: element for batched hit update could be evicted by other threads. cache
// TODO: miss locks mutably for slab alloc/delete, noderefmap update.
// TODO: can also be batched however the lifetime of TrieNode should be managed.
pub struct CacheManager<
    CacheAlgoDataT: CacheAlgoDataTrait,
    CacheAlgorithmT: CacheAlgorithm<CacheAlgoData = CacheAlgoDataT, CacheIndex = DeltaMptDbKey>,
> {
    /// One of the key problem in implementing a cache for tree node is that,
    /// when a node is swapped-out from cache into disk, the eviction of
    /// children should be independent, not only because of cache hit
    /// property, but also because that a node can have multiple parents. When
    /// a node is loaded into cache again, it should automatically connects
    /// to its children in cache, even if the children is shared with some
    /// other node unknown.
    ///
    /// Another problem is that, when a node is swapped-out, the parent's
    /// children reference must be updated unless the children reference is
    /// the db key, or something stable. The db key in Ethereum is the
    /// Merkle Hash, which is too large for a Trie node: 16*64B are
    /// required to store only ChildrenTable.
    ///
    /// To solve these problems, we introduce CacheableNodeRef, which should
    /// remain stable for the lifetime of the TrieNode of the
    /// ChildrenTable. The key of NodeRefMap shall be db key, and the value
    /// of NodeRefMap shall point to where the node is cached.
    ///
    /// The db key could be made smaller for Delta MPT (4B)
    /// and maybe for Persistent MPT (8B) by simply using the row number.
    ///
    /// If we have to use Merkle Hash (64B) as db key for Persistent MPT,
    /// storing the key for non-cached node is costly. There are two options,
    /// a) store key only for cached nodes, there is little addition cost (8B),
    /// however to load an un-cached child node from disk, caller should first
    /// read the child's access key by loading the current node, then check
    /// NodeRefMap, if actual missing load it from disk for the second
    /// time.
    /// b) create NodeRef for some children of cached node, store a the 64B key
    /// for disk access, and keep the reference count so that we don't store
    /// NodeRef for nodes which later becomes irrelevant indefinitely.
    ///
    /// Note that there are also dirty nodes, which always live in memory.
    /// The NodeRef / MaybeNodeRef also covers dirty nodes, but NodeRefMap
    /// covers only commited nodes.
    node_ref_map: NodeRefMapDeltaMpt<CacheAlgoDataT, CacheAlgorithmT>,
    cache_algorithm: CacheAlgorithmT,
}

pub struct NodeMemoryManager<
    CacheAlgoDataT: CacheAlgoDataTrait,
    CacheAlgorithmT: CacheAlgorithm<CacheAlgoData = CacheAlgoDataT, CacheIndex = DeltaMptDbKey>,
> {
    /// The max number of nodes.
    size_limit: u32,
    /// Unless size limit reached, there should be at lease idle_size available
    /// after each resize.
    idle_size: u32,
    /// Always get the read lock first because only resizing requires write
    /// lock, which we don't want other lock wait for.
    allocator: RwLock<Allocator<CacheAlgoDataT>>,
    cache: RwLock<CacheManager<CacheAlgoDataT, CacheAlgorithmT>>,
    // TODO(yz): use a more specific db because the row number need to be
    // managed.
    /// This db access is read only.
    db: Arc<KeyValueDB>,
}

impl<
        CacheAlgoDataT: CacheAlgoDataTrait,
        CacheAlgorithmT: CacheAlgorithm<
            CacheAlgoData = CacheAlgoDataT,
            CacheIndex = DeltaMptDbKey,
        >,
    > NodeMemoryManager<CacheAlgoDataT, CacheAlgorithmT>
{
    /// In disk hybrid solution, the nodes in memory are merely LRU cache of
    /// non-leaf nodes. So the memory consumption is (192B Trie + 10B R_LFU +
    /// 12B*4x LRU) * number of nodes + 200M * 4B NodeRef. 5GB + extra 800M
    /// ~ 20_000_000 nodes.
    // TODO(yz): Need to calculate a factor in LRU (currently made up to 4).
    pub const MAX_CACHED_TRIE_NODES_DISK_HYBRID: u32 = 20_000_000;
    pub const MAX_CACHED_TRIE_NODES_R_LFU_COUNTER: u32 = (Self::R_LFU_FACTOR
        * Self::MAX_CACHED_TRIE_NODES_DISK_HYBRID as f64)
        as u32;
    /// Splitting out dirty trie nodes may remove the hard limit, however it
    /// introduces copies for committing.
    // TODO(yz): log the dirty size to monitor if other component produces too
    // many.
    pub const MAX_DIRTY_AND_TEMPORARY_TRIE_NODES: u32 = 200_000;
    /// If we do not swap out any node onto disk, the maximum tolerable nodes is
    /// about 27.6M, where there is about 4.6M leaf node. The total memory
    /// consumption is about (27.6 * 192 - 4.6 * 64) MB ~= 5GB. It can hold new
    /// items for about 38 min assuming 2k updates per second.
    /// The reason of having much more nodes than leaf nodes is that this is a
    /// multiple version tree, so we have a factor of 3.3 (extra layers) per
    /// leaf node. This assumption is for delta_trie.
    pub const MAX_TRIE_NODES_MEM_ONLY: u32 = 27_600_000;
    pub const R_LFU_FACTOR: f64 = 4.0;
    pub const START_CAPACITY: u32 = 1_000_000;
}

impl<
        CacheAlgoDataT: CacheAlgoDataTrait,
        CacheAlgorithmT: CacheAlgorithm<
            CacheAlgoData = CacheAlgoDataT,
            CacheIndex = DeltaMptDbKey,
        >,
    > NodeMemoryManager<CacheAlgoDataT, CacheAlgorithmT>
{
    pub fn new(
        start_size: u32, cache_size: u32, idle_size: u32,
        cache_algorithm: CacheAlgorithmT, kvdb: Arc<KeyValueDB>,
    ) -> Self
    {
        let size_limit = cache_size + idle_size;
        Self {
            size_limit,
            idle_size,
            allocator: RwLock::new(Slab::with_capacity(size_limit as usize)),
            cache: RwLock::new(CacheManager {
                node_ref_map: NodeRefMapDeltaMpt::new(start_size as usize),
                cache_algorithm: cache_algorithm,
            }),
            db: kvdb,
        }
    }

    pub fn get_allocator(&self) -> AllocatorRef<CacheAlgoDataT> {
        self.allocator.read_recursive()
    }

    pub fn get_cache_manager_mut(
        &self,
    ) -> RwLockWriteGuard<CacheManager<CacheAlgoDataT, CacheAlgorithmT>> {
        self.cache.write()
    }

    /// Method that requires mut borrow of allocator.
    pub fn enlarge(&self) -> Result<()> {
        let mut allocator_mut = self.allocator.write();
        let idle = allocator_mut.capacity() - allocator_mut.len();
        let should_idle = self.idle_size as usize;
        if idle >= should_idle {
            return Ok(());
        }
        let mut add_size = should_idle - idle;
        if add_size < allocator_mut.capacity() {
            add_size = allocator_mut.capacity();
        }
        let max_add_size = self.size_limit as usize - allocator_mut.len();
        if add_size >= max_add_size {
            add_size = max_add_size;
        }
        allocator_mut.reserve_exact(add_size)?;
        Ok(())
    }

    pub unsafe fn get_in_memory_node_mut<'a>(
        allocator: AllocatorRefRef<'a, CacheAlgoDataT>, cache_slot: usize,
    ) -> &'a mut TrieNode<CacheAlgoDataT> {
        allocator.get_unchecked_mut(cache_slot)
    }

    // FIXME: return a node.
    fn load_from_db<'c: 'a, 'a>(
        &self, allocator: AllocatorRefRef<'a, CacheAlgoDataT>,
        cache_manager: &'c RwLock<
            CacheManager<CacheAlgoDataT, CacheAlgorithmT>,
        >,
        db_key: DeltaMptDbKey,
    ) -> Result<
        GuardedValue<
            RwLockWriteGuard<'c, CacheManager<CacheAlgoDataT, CacheAlgorithmT>>,
            &'a TrieNode<CacheAlgoDataT>,
        >,
    >
    {
        // We never save null node in db.
        let rlp_bytes = self
            .db
            .get(COL_DELTA_TRIE, db_key.to_string().as_bytes())?
            .unwrap();
        let rlp = Rlp::new(rlp_bytes.as_ref());
        let mut trie_node = TrieNode::decode(&rlp)?;

        let mut cache_manager_write = cache_manager.write();
        let trie_node_ref;
        {
            let cache_mut = &mut *cache_manager_write;

            // If cache_algo_data exists in node_ref_map, move to trie node.
            match cache_mut.node_ref_map.get(db_key) {
                None => {}
                Some(cache_info) => match cache_info.get_cache_info() {
                    TrieCacheSlotOrCacheAlgoData::TrieCacheSlot(_) => unsafe {
                        unreachable_unchecked();
                    },
                    TrieCacheSlotOrCacheAlgoData::CacheAlgoData(
                        cache_algo_data,
                    ) => {
                        trie_node.cache_algo_data = *cache_algo_data;
                    }
                },
            }
            // Insert into slab as temporary, then insert into node_ref_map.
            let slot = allocator.insert(trie_node)?;
            trie_node_ref = unsafe { allocator.get_unchecked(slot) };
            cache_mut.node_ref_map.insert(
                db_key,
                slot as ActualSlabIndex,
                &self,
                &mut cache_mut.cache_algorithm,
            );
        }
        Ok(GuardedValue::new(cache_manager_write, trie_node_ref))
    }

    /// This method is called when loading from db.
    /// unsafe because the key must be existing.
    pub unsafe fn delete_from_cache(
        &self, cache_algorithm: &mut CacheAlgorithmT,
        node_ref_map: &mut NodeRefMapDeltaMpt<CacheAlgoDataT, CacheAlgorithmT>,
        db_key: DeltaMptDbKey,
        cache_info: CacheableNodeRefDeltaMpt<CacheAlgoDataT>,
    )
    {
        cache_algorithm
            .delete(db_key, &mut NodeCacheUtil::new(self, node_ref_map));

        match cache_info.get_cache_info() {
            TrieCacheSlotOrCacheAlgoData::TrieCacheSlot(slot) => {
                self.get_allocator().remove((*slot) as usize).unwrap();
            }
            _ => {}
        }
    }

    unsafe fn delete_cache_evicted_unchecked(
        &self, cache_mut: &mut CacheManager<CacheAlgoDataT, CacheAlgorithmT>,
        evicted_db_key: DeltaMptDbKey,
    )
    {
        // Remove evicted content from cache.
        let cache_info = cache_mut.node_ref_map.delete(evicted_db_key).unwrap();
        match cache_info.get_cache_info() {
            TrieCacheSlotOrCacheAlgoData::TrieCacheSlot(slot) => {
                self.get_allocator().remove((*slot) as usize).unwrap();
            }
            _ => {}
        }
    }

    unsafe fn delete_cache_evicted_keep_cache_algo_data_unchecked(
        &self, cache_mut: &mut CacheManager<CacheAlgoDataT, CacheAlgorithmT>,
        evicted_db_key_keep_cache_algo_data: DeltaMptDbKey,
    )
    {
        // Remove evicted content from cache.
        // Safe to unwrap because it's guaranteed by cache algorithm that the
        // slot exists.
        let slot = *cache_mut
            .node_ref_map
            .get(evicted_db_key_keep_cache_algo_data)
            .unwrap()
            .get_slot()
            .unwrap() as usize;

        cache_mut.node_ref_map.set_cache_info(
            evicted_db_key_keep_cache_algo_data,
            Some(CacheableNodeRefDeltaMpt::new(
                TrieCacheSlotOrCacheAlgoData::CacheAlgoData(
                    self.get_allocator().get_unchecked(slot).cache_algo_data,
                ),
            )),
        );
        self.get_allocator().remove(slot).unwrap();
    }

    // TODO(yz): special thread local batching logic for access_hit?
    pub fn call_cache_algorithm_access(
        &self, cache_mut: &mut CacheManager<CacheAlgoDataT, CacheAlgorithmT>,
        db_key: DeltaMptDbKey,
    )
    {
        let cache_access_result;
        {
            let mut cache_store_util =
                NodeCacheUtil::new(self, &mut cache_mut.node_ref_map);
            cache_access_result = cache_mut
                .cache_algorithm
                .access(db_key, &mut cache_store_util);
        }
        match cache_access_result {
            CacheAccessResult::MissReplaced {
                evicted: evicted_db_keys,
                evicted_keep_cache_algo_data:
                    evicted_keep_cache_algo_data_db_keys,
            } => unsafe {
                for evicted_db_key in evicted_db_keys {
                    self.delete_cache_evicted_unchecked(
                        cache_mut,
                        evicted_db_key,
                    );
                }
                for evicted_keep_cache_algo_data_db_key in
                    evicted_keep_cache_algo_data_db_keys
                {
                    self.delete_cache_evicted_keep_cache_algo_data_unchecked(
                        cache_mut,
                        evicted_keep_cache_algo_data_db_key,
                    );
                }
            },
            _ => {}
        }
    }

    /// Get mutable reference to TrieNode from dirty (owned) trie node. There is
    /// no need to lock cache manager in this case.
    ///
    /// unsafe because it's unchecked that the node is dirty.
    pub unsafe fn dirty_node_as_mut_unchecked<'a>(
        &self, allocator: AllocatorRefRef<'a, CacheAlgoDataT>,
        node: &mut NodeRefDeltaMpt,
    ) -> &'a mut TrieNode<CacheAlgoDataT>
    {
        match node {
            NodeRefDeltaMpt::Committed { ref db_key } => {
                unreachable_unchecked();
            }
            NodeRefDeltaMpt::Dirty { ref index } => NodeMemoryManager::<
                CacheAlgoDataT,
                CacheAlgorithmT,
            >::get_in_memory_node_mut(
                &allocator,
                *index as usize,
            ),
        }
    }

    /// Unsafe because node is assumed to be committed.
    unsafe fn load_unowned_node_internal_unchecked<'c: 'a, 'a>(
        &self, allocator: AllocatorRefRef<'a, CacheAlgoDataT>,
        node: &NodeRefDeltaMpt,
        cache_manager: &'c RwLock<
            CacheManager<CacheAlgoDataT, CacheAlgorithmT>,
        >,
    ) -> Result<
        GuardedValue<
            Option<
                RwLockWriteGuard<
                    'c,
                    CacheManager<CacheAlgoDataT, CacheAlgorithmT>,
                >,
            >,
            &'a TrieNode<CacheAlgoDataT>,
        >,
    >
    {
        match node {
            NodeRefDeltaMpt::Committed { ref db_key } => {
                let mut cache_manager_write;
                // Use mut because compiler isn't smart enough to know that it's
                // initialized once.
                let mut trie_node: &TrieNode<CacheAlgoDataT>;
                let mut load_from_db = false;
                {
                    let cache_manager_read = cache_manager.upgradable_read();
                    let maybe_cache_slot = cache_manager_read
                        .node_ref_map
                        .get(*db_key)
                        .and_then(|x| x.get_slot());

                    match maybe_cache_slot {
                        None => {
                            // We would like to release the lock to
                            // cache_manager during db IO.
                            load_from_db = true;
                            // Compiler isn't smart enough to know that
                            // the variables are always initialized.
                            trie_node = mem::uninitialized();
                            cache_manager_write = mem::uninitialized()
                        }
                        Some(cache_slot) => {
                            trie_node = NodeMemoryManager::<
                                CacheAlgoDataT,
                                CacheAlgorithmT,
                            >::get_in_memory_node_mut(
                                &allocator,
                                *cache_slot as usize,
                            );
                            cache_manager_write =
                                RwLockUpgradableReadGuard::upgrade(
                                    cache_manager_read,
                                );
                        }
                    }
                }
                if load_from_db {
                    let guard_with_value = self
                        .load_from_db(allocator, cache_manager, *db_key)?
                        .into();
                    cache_manager_write = guard_with_value.0;
                    trie_node = guard_with_value.1;
                }
                self.call_cache_algorithm_access(
                    &mut cache_manager_write,
                    *db_key,
                );

                Ok(GuardedValue::new(Some(cache_manager_write), trie_node))
            }
            NodeRefDeltaMpt::Dirty { ref index } => unreachable_unchecked(),
        }
    }

    // FIXME: apply 'a to node?
    fn load_node<'c: 'a, 'a>(
        &'c self, allocator: AllocatorRefRef<'a, CacheAlgoDataT>,
        node: &NodeRefDeltaMpt,
    ) -> Result<
        GuardedValue<
            Option<
                RwLockWriteGuard<
                    'c,
                    CacheManager<CacheAlgoDataT, CacheAlgorithmT>,
                >,
            >,
            &'a TrieNode<CacheAlgoDataT>,
        >,
    >
    {
        match node {
            NodeRefDeltaMpt::Committed { ref db_key } => unsafe {
                self.load_unowned_node_internal_unchecked(
                    allocator,
                    node,
                    &self.cache,
                )
            },
            NodeRefDeltaMpt::Dirty { ref index } => unsafe {
                Ok(GuardedValue::new(None, NodeMemoryManager::<
                CacheAlgoDataT,
                CacheAlgorithmT,
            >::get_in_memory_node_mut(
                &allocator,
                *index as usize,
            )))
            },
        }
    }

    // FIXME: pass a cache manager / node_ref_map to prove ownership.
    unsafe fn get_cached_node_mut_unchecked<'a>(
        &self, allocator: AllocatorRefRef<'a, CacheAlgoDataT>,
        slot: DeltaMptDbKey,
    ) -> &'a mut TrieNode<CacheAlgoDataT>
    {
        NodeMemoryManager::<CacheAlgoDataT, CacheAlgorithmT>::get_in_memory_node_mut(
            &allocator,
            slot as usize,
        )
    }

    // FIXME: apply 'a to node?
    pub fn node_as_ref<'c: 'a, 'a>(
        &'c self, allocator: AllocatorRefRef<'a, CacheAlgoDataT>,
        node: &NodeRefDeltaMpt,
    ) -> Result<
        GuardedValue<
            Option<
                RwLockWriteGuard<
                    'c,
                    CacheManager<CacheAlgoDataT, CacheAlgorithmT>,
                >,
            >,
            &'a TrieNode<CacheAlgoDataT>,
        >,
    >
    {
        self.load_node(allocator, node)
    }

    // FIXME: is this method useful to share lock guard in some cases?
    // FIXME: if lock guard can be shared, we must implement load_unowned_node
    // FIXME: one more time with lock guard instead of lock itself.
    pub fn node_as_ref_with_cache_manager<'a>(
        &self, allocator: AllocatorRefRef<'a, CacheAlgoDataT>,
        node: &NodeRefDeltaMpt,
        cache_manager: &'a RwLock<
            CacheManager<CacheAlgoDataT, CacheAlgorithmT>,
        >,
    ) -> Result<&'a TrieNode<CacheAlgoDataT>>
    {
        match node {
            NodeRefDeltaMpt::Committed { ref db_key } => unsafe {
                self.load_unowned_node_internal_unchecked(
                    allocator,
                    node,
                    cache_manager,
                )
                .map(|guarded| guarded.into().1)
            },
            NodeRefDeltaMpt::Dirty { ref index } => unsafe {
                Ok(NodeMemoryManager::<
                CacheAlgoDataT,
                CacheAlgorithmT,
            >::get_in_memory_node_mut(
                &allocator,
                *index as usize,
            ))
            },
        }
    }

    pub fn new_node<'a>(
        allocator: AllocatorRefRef<'a, CacheAlgoDataT>,
    ) -> Result<(NodeRefDeltaMpt, VacantEntry<'a, TrieNode<CacheAlgoDataT>>)>
    {
        let vacant_entry = allocator.vacant_entry()?;
        let node = NodeRefDeltaMpt::Dirty {
            index: vacant_entry.key() as ActualSlabIndex,
        };
        Ok((node, vacant_entry))
    }

    /// Usually the node to free is dirty (i.e. not committed), however it's
    /// also possible that the state db commitment fails so that the succeeded
    /// nodes in the commitment should be removed from cache and deleted.
    pub fn free_node(&self, node: &mut NodeRefDeltaMpt) {
        let slot = match node {
            NodeRefDeltaMpt::Committed { ref db_key } => {
                let maybe_cache_info =
                    self.cache.write().node_ref_map.delete(*db_key);
                let maybe_cache_slot = maybe_cache_info
                    .as_ref()
                    .and_then(|cache_info| cache_info.get_slot());

                match maybe_cache_slot {
                    None => return,
                    Some(slot) => *slot,
                }
            }
            NodeRefDeltaMpt::Dirty { ref index } => *index,
        };

        // This unwrap is fine because we return early if slot doesn't exist.
        self.get_allocator().remove(slot as usize).unwrap();
    }
}

struct NodeCacheUtil<
    'a,
    CacheAlgoDataT: CacheAlgoDataTrait,
    CacheAlgorithmT: CacheAlgorithm<CacheAlgoData = CacheAlgoDataT, CacheIndex = DeltaMptDbKey>,
> {
    node_memory_manager: &'a NodeMemoryManager<CacheAlgoDataT, CacheAlgorithmT>,
    node_ref_map: &'a mut NodeRefMapDeltaMpt<CacheAlgoDataT, CacheAlgorithmT>,
}

impl<
        'a,
        CacheAlgoDataT: CacheAlgoDataTrait,
        CacheAlgorithmT: CacheAlgorithm<
            CacheAlgoData = CacheAlgoDataT,
            CacheIndex = DeltaMptDbKey,
        >,
    > NodeCacheUtil<'a, CacheAlgoDataT, CacheAlgorithmT>
{
    fn new(
        node_memory_manager: &'a NodeMemoryManager<
            CacheAlgoDataT,
            CacheAlgorithmT,
        >,
        node_map: &'a mut NodeRefMapDeltaMpt<CacheAlgoDataT, CacheAlgorithmT>,
    ) -> Self
    {
        NodeCacheUtil {
            node_memory_manager: node_memory_manager,
            node_ref_map: node_map,
        }
    }
}

impl<
        'a,
        CacheAlgoDataT: CacheAlgoDataTrait,
        CacheAlgorithmT: CacheAlgorithm<
            CacheAlgoData = CacheAlgoDataT,
            CacheIndex = DeltaMptDbKey,
        >,
    > CacheStoreUtil for NodeCacheUtil<'a, CacheAlgoDataT, CacheAlgorithmT>
{
    type CacheAlgoData = CacheAlgoDataT;
    type ElementIndex = DeltaMptDbKey;

    fn get(&self, db_key: DeltaMptDbKey) -> Self::CacheAlgoData {
        match self.node_ref_map.get(db_key).unwrap().get_cache_info() {
            TrieCacheSlotOrCacheAlgoData::TrieCacheSlot(slot) => {
                let allocator = self.node_memory_manager.get_allocator();
                unsafe {
                    self.node_memory_manager
                        .get_cached_node_mut_unchecked(&allocator, *slot)
                        .cache_algo_data
                }
            }
            TrieCacheSlotOrCacheAlgoData::CacheAlgoData(cache_algo_data) => {
                cache_algo_data.clone()
            }
        }
    }

    fn set(&mut self, db_key: DeltaMptDbKey, algo_data: &Self::CacheAlgoData) {
        match self.node_ref_map.get(db_key).unwrap().get_cache_info() {
            TrieCacheSlotOrCacheAlgoData::TrieCacheSlot(slot) => {
                let allocator = self.node_memory_manager.get_allocator();
                unsafe {
                    self.node_memory_manager
                        .get_cached_node_mut_unchecked(&allocator, *slot)
                        .cache_algo_data = *algo_data;
                }
            }
            TrieCacheSlotOrCacheAlgoData::CacheAlgoData(_) => {
                self.node_ref_map.set_cache_info(
                    db_key,
                    Some(CacheableNodeRefDeltaMpt::new(
                        TrieCacheSlotOrCacheAlgoData::CacheAlgoData(*algo_data),
                    )),
                );
            }
        }
    }
}

impl<
        CacheAlgoDataT: CacheAlgoDataTrait,
        CacheAlgorithmT: CacheAlgorithm<
            CacheAlgoData = CacheAlgoDataT,
            CacheIndex = DeltaMptDbKey,
        >,
    > CacheManager<CacheAlgoDataT, CacheAlgorithmT>
{
    pub fn insert_to_node_ref_map_and_call_cache_access(
        &mut self, db_key: DeltaMptDbKey, slot: ActualSlabIndex,
        node_memory_manager: &NodeMemoryManager<
            CacheAlgoDataT,
            CacheAlgorithmT,
        >,
    )
    {
        self.node_ref_map.insert(
            db_key,
            slot,
            node_memory_manager,
            &mut self.cache_algorithm,
        );
        node_memory_manager.call_cache_algorithm_access(self, db_key);
    }
}

use super::{
    super::{super::super::db::COL_DELTA_TRIE, errors::*},
    cache::algorithm::{
        recent_lfu::RecentLFU, CacheAccessResult, CacheAlgoDataTrait,
        CacheAlgorithm, CacheIndexTrait, CacheStoreUtil,
    },
    guarded_value::*,
    merkle_patricia_trie::*,
    node_ref_map::*,
    slab::Slab,
};
use kvdb::KeyValueDB;
use parking_lot::{
    RwLock, RwLockReadGuard, RwLockUpgradableReadGuard, RwLockWriteGuard,
};
use rlp::*;
use std::{hint::unreachable_unchecked, mem, sync::Arc};

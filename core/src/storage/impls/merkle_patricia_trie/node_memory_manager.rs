use super::{
    super::{super::super::db::COL_DELTA_TRIE, errors::*},
    cache::algorithm::{
        lru::{LRUHandle, LRU},
        CacheAccessResult, CacheAlgorithm, CacheIndexTrait, CacheStoreUtil,
    },
    data_structure::*,
    guarded_value::*,
    node_ref_map::*,
    slab::Slab,
};
use kvdb::KeyValueDB;
use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use rlp::*;
use std::sync::Arc;

pub type LRUPosT = u32;

pub type ActualSlabIndex = u32;
type Allocator = Slab<TrieNode, TrieNodeSlabEntry>;
pub type AllocatorRef<'a> = RwLockReadGuard<'a, Allocator>;
pub type AllocatorRefRef<'a> = &'a AllocatorRef<'a>;

pub type CacheAlgorithmDeltaMpt = LRU<LRUPosT, DeltaMptDbKey>;

pub type CacheManagerMut<'a> = RwLockWriteGuard<'a, CacheManager>;

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
pub struct CacheManager {
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
    node_ref_map: NodeRefMapDeltaMpt,
    lru_cache_algorithm: CacheAlgorithmDeltaMpt,
}

pub struct NodeMemoryManager {
    /// The max number of nodes.
    size_limit: u32,
    /// Unless size limit reached, there should be at lease idle_size available
    /// after each resize.
    idle_size: u32,
    /// Always get the read lock first because only resizing requires write
    /// lock, which we don't want other lock wait for.
    allocator: RwLock<Allocator>,
    cache: RwLock<CacheManager>,
    // TODO(yz): use a more specific db because the row number need to be
    // managed.
    /// This db access is read only.
    db: Arc<KeyValueDB>,
}

impl NodeMemoryManager {
    pub const LFRU_FACTOR: f64 = 4.0;
    /// In disk hybrid solution, the nodes in memory are merely LRU cache of
    /// non-leaf nodes. So the memory consumption is (192B Trie + 10B LFRU +
    /// 12B*4x LRU) * number of nodes + 200M * 4B NodeRef. 5GB + extra 800M
    /// ~ 20_000_000 nodes.
    // TODO(yz): Need to calculate a factor in LRU (currently made up to 4).
    pub const MAX_CACHED_TRIE_NODES_DISK_HYBRID: u32 = 20_000_000;
    pub const MAX_CACHED_TRIE_NODES_LFRU_COUNTER: u32 = (Self::LFRU_FACTOR
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
    pub const START_CAPACITY: u32 = 1_000_000;
}

impl NodeMemoryManager {
    pub fn new(kvdb: Arc<KeyValueDB>) -> Self {
        Self::new_with_size(
            Self::START_CAPACITY,
            Self::MAX_CACHED_TRIE_NODES_DISK_HYBRID,
            Self::MAX_DIRTY_AND_TEMPORARY_TRIE_NODES,
            Self::LFRU_FACTOR,
            kvdb,
        )
    }

    pub fn new_with_size(
        start_size: u32, cache_size: u32, idle_size: u32, lfru_factor: f64,
        kvdb: Arc<KeyValueDB>,
    ) -> Self
    {
        let size_limit = cache_size + idle_size;
        Self {
            size_limit,
            idle_size,
            allocator: RwLock::new(Slab::with_capacity(size_limit as usize)),
            cache: RwLock::new(CacheManager {
                node_ref_map: NodeRefMapDeltaMpt::new(size_limit as usize),
                lru_cache_algorithm: LRU::<u32, DeltaMptDbKey>::new(
                    cache_size,
                    // TODO(yz): add back when LFRU is finally ready.
                    //(cache_size as f64 * lfru_factor) as u32,
                ),
            }),
            db: kvdb,
        }
    }

    pub fn get_allocator(&self) -> AllocatorRef {
        self.allocator.read_recursive()
    }

    pub fn get_cache_manager_mut(&self) -> CacheManagerMut {
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

    fn get_cache_slot(
        node_ref_map: &NodeRefMapDeltaMpt, db_key: DeltaMptDbKey,
    ) -> Option<ActualSlabIndex> {
        node_ref_map.get(db_key).and_then(|x| x.to_slot())
    }

    fn get_in_memory_node_mut<'a>(
        allocator: AllocatorRefRef<'a>, cache_slot: usize,
    ) -> &'a mut TrieNode {
        unsafe { allocator.get_unchecked_mut(cache_slot) }
    }

    fn load_from_db(
        &self, cache_mut: &mut CacheManager, db_key: DeltaMptDbKey,
    ) -> Result<ActualSlabIndex> {
        // We never save null node in db.
        let rlp_bytes = self
            .db
            .get(COL_DELTA_TRIE, db_key.to_string().as_bytes())?
            .unwrap();
        let rlp = Rlp::new(rlp_bytes.as_ref());
        let trie_node = TrieNode::decode(&rlp)?;
        // Insert into slab as temporary, then insert into node_ref_map.
        let slot = self.get_allocator().insert(trie_node)? as ActualSlabIndex;
        cache_mut.node_ref_map.insert(
            db_key,
            slot,
            &self,
            &mut cache_mut.lru_cache_algorithm,
        );

        Ok(slot)
    }

    /// Called after node are marked committed in Slab.
    /// The updating of the NodeRef from its parent are already done because
    /// dirty nodes comes from a tree when recursively committing.
    pub fn dirty_node_committed(&self, node_ref: &NodeRefDeltaMpt) {
        match node_ref {
            NodeRefDeltaMpt::Committed { ref db_key } => {
                self.call_cache_algorithm_access(
                    &mut *self.cache.write(),
                    *db_key,
                );
            }
            _ => {}
        }
    }

    /// This method is called when loading from db.
    /// unsafe because the key must be existing.
    pub unsafe fn delete_from_cache(
        &self, cache_algorithm: &mut CacheAlgorithmDeltaMpt,
        node_ref_map: &NodeRefMapDeltaMpt, db_key: DeltaMptDbKey,
        slot: ActualSlabIndex,
    )
    {
        cache_algorithm
            .delete(db_key, &mut NodeCacheUtil::new(self, node_ref_map));

        self.get_allocator().remove(slot as usize).unwrap();
    }

    unsafe fn delete_cache_evicted_unchecked(
        &self, cache_mut: &mut CacheManager, evicted_db_key: DeltaMptDbKey,
    ) {
        // Remove evicted content from cache.
        let slot = cache_mut.node_ref_map.delete(evicted_db_key).unwrap();
        self.get_allocator().remove(slot as usize).unwrap();
    }

    // TODO(yz): special thread local batching logic for access_hit?
    fn call_cache_algorithm_access(
        &self, cache_mut: &mut CacheManager, db_key: DeltaMptDbKey,
    ) {
        let cache_access_result;
        {
            let mut cache_store_util =
                NodeCacheUtil::new(self, &cache_mut.node_ref_map);
            cache_access_result = cache_mut
                .lru_cache_algorithm
                .access(db_key, &mut cache_store_util);
        }
        match cache_access_result {
            CacheAccessResult::MissReplaced {
                evicted: evicted_db_key,
            } => unsafe {
                self.delete_cache_evicted_unchecked(cache_mut, evicted_db_key);
            },
            _ => {}
        }
    }

    fn load_node_internal<'a>(
        &self, allocator: AllocatorRefRef<'a>, node: &NodeRefDeltaMpt,
        cache_manager: &mut CacheManager,
    ) -> Result<&'a mut TrieNode>
    {
        match node {
            NodeRefDeltaMpt::Committed { ref db_key } => {
                let maybe_cache_slot =
                    Self::get_cache_slot(&cache_manager.node_ref_map, *db_key);
                let cache_slot;
                match maybe_cache_slot {
                    None => {
                        // Miss.
                        cache_slot =
                            self.load_from_db(cache_manager, *db_key)?;
                    }
                    Some(inner_cache_slot) => {
                        // Hit.
                        cache_slot = inner_cache_slot;
                    }
                }
                let node = NodeMemoryManager::get_in_memory_node_mut(
                    &allocator,
                    cache_slot as usize,
                );
                self.call_cache_algorithm_access(cache_manager, *db_key);

                Ok(node)
            }
            NodeRefDeltaMpt::Dirty { ref index } => {
                Ok(NodeMemoryManager::get_in_memory_node_mut(
                    &allocator,
                    *index as usize,
                ))
            }
        }
    }

    unsafe fn get_cached_node_mut_unchecked<'a>(
        &self, allocator: AllocatorRefRef<'a>,
        node_ref_map: &NodeRefMapDeltaMpt, db_key: DeltaMptDbKey,
    ) -> &'a mut TrieNode
    {
        // Unwrap because cache_slot is protected by &CacheManagement, and the
        // precondition is guaranteed by caller.
        let cache_slot = Self::get_cache_slot(node_ref_map, db_key).unwrap();
        NodeMemoryManager::get_in_memory_node_mut(
            &allocator,
            cache_slot as usize,
        )
    }

    // FIXME: CacheManager is only relevant for committed node.
    pub fn node_as_ref<'a>(
        &self, allocator: AllocatorRefRef<'a>, node: &NodeRefDeltaMpt,
    ) -> Result<GuardedValue<RwLockWriteGuard<CacheManager>, &'a TrieNode>>
    {
        let mut cache_manager_lock = self.cache.write();
        let trie_node =
            self.load_node_internal(allocator, node, &mut *cache_manager_lock)?;

        Ok(GuardedValue::new(cache_manager_lock, trie_node))
    }

    // FIXME: CacheManager is only relevant for committed node.
    pub fn node_as_mut<'a>(
        &self, allocator: AllocatorRefRef<'a>, node: &mut NodeRefDeltaMpt,
    ) -> Result<GuardedValue<RwLockWriteGuard<CacheManager>, &'a mut TrieNode>>
    {
        let mut cache_manager_lock = self.cache.write();
        let trie_node =
            self.load_node_internal(allocator, node, &mut *cache_manager_lock)?;

        Ok(GuardedValue::new(cache_manager_lock, trie_node))
    }

    pub fn node_as_mut_with_cache_manager<'a>(
        &self, allocator: AllocatorRefRef<'a>, node: &mut NodeRefDeltaMpt,
        cache_manager: &mut CacheManager,
    ) -> Result<&'a mut TrieNode>
    {
        self.load_node_internal(allocator, node, cache_manager)
    }

    pub fn new_node<'a>(
        allocator: AllocatorRefRef<'a>,
    ) -> Result<(NodeRefDeltaMpt, VacantEntry<'a>)> {
        let vacant_entry = allocator.vacant_entry()?;
        let node = NodeRefDeltaMpt::Dirty {
            index: vacant_entry.key() as ActualSlabIndex,
        };
        Ok((node, vacant_entry))
    }

    /// Usually the node to free is dirty (i.e. not committed), however it's
    /// also possible that the state db commitment fails so the node is in
    /// memory committed but it should be reverted.
    pub fn free_node(&self, node: &mut NodeRefDeltaMpt) {
        let slot = match node {
            NodeRefDeltaMpt::Committed { ref db_key } => {
                let mut cache_mut = self.cache.write();
                let maybe_slot =
                    Self::get_cache_slot(&cache_mut.node_ref_map, *db_key);
                match maybe_slot {
                    None => return,
                    Some(slot) => {
                        unsafe {
                            cache_mut.node_ref_map.delete(*db_key);
                        }
                        slot
                    }
                }
            }
            NodeRefDeltaMpt::Dirty { ref index } => *index,
        };

        // This unwrap is fine because we return early if slot doesn't exist.
        self.get_allocator().remove(slot as usize).unwrap();
    }
}

struct NodeCacheUtil<'a> {
    node_memory_manager: &'a NodeMemoryManager,
    node_ref_map: &'a NodeRefMapDeltaMpt,
}

impl<'a> NodeCacheUtil<'a> {
    fn new(
        node_memory_manager: &'a NodeMemoryManager,
        node_map: &'a NodeRefMapDeltaMpt,
    ) -> Self
    {
        NodeCacheUtil {
            node_memory_manager: node_memory_manager,
            node_ref_map: node_map,
        }
    }
}

impl<'a> CacheStoreUtil for NodeCacheUtil<'a> {
    type CacheAlgoData = LRUHandle<LRUPosT>;
    type ElementIndex = DeltaMptDbKey;

    fn get(&self, db_key: DeltaMptDbKey) -> Self::CacheAlgoData {
        let allocator = self.node_memory_manager.get_allocator();
        unsafe {
            self.node_memory_manager
                .get_cached_node_mut_unchecked(
                    &allocator,
                    self.node_ref_map,
                    db_key,
                )
                .cache_algo_data
        }
    }

    fn set(&mut self, db_key: DeltaMptDbKey, algo_data: &Self::CacheAlgoData) {
        let allocator = self.node_memory_manager.get_allocator();
        unsafe {
            self.node_memory_manager
                .get_cached_node_mut_unchecked(
                    &allocator,
                    self.node_ref_map,
                    db_key,
                )
                .cache_algo_data = *algo_data;
        }
    }
}

impl CacheManager {
    pub fn insert_to_node_ref_map(
        &mut self, db_key: DeltaMptDbKey, slot: ActualSlabIndex,
        node_memory_manager: &NodeMemoryManager,
    )
    {
        self.node_ref_map.insert(
            db_key,
            slot,
            node_memory_manager,
            &mut self.lru_cache_algorithm,
        );
    }
}

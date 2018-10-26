use std::collections::{VecDeque, HashSet, HashMap};
use std::hash::Hash;

const COLLECTION_QUEUE_SIZE: usize = 8;

pub enum CacheEntryState {
    Clean,
    Dirty,
    Invalid,
}

pub struct WriteBackCacheManager<T> {
	pref_cache_size: usize,
	max_cache_size: usize,
	bytes_per_cache_entry: usize,
	cache_usage: VecDeque<HashMap<T, CacheEntryState>>,
}

impl<T> WriteBackCacheManager<T> where T: Eq + Hash + Clone {
	pub fn new(pref_cache_size: usize, max_cache_size: usize, bytes_per_cache_entry: usize) -> Self {
		WriteBackCacheManager {
			pref_cache_size: pref_cache_size,
			max_cache_size: max_cache_size,
			bytes_per_cache_entry: bytes_per_cache_entry,
			cache_usage: (0..COLLECTION_QUEUE_SIZE).into_iter().map(|_| Default::default()).collect(),
		}
	}

	pub fn note_read(&mut self, id: T) {
		if !self.cache_usage[0].contains_key(&id) {
            let mut entry_state = CacheEntryState::Clean;
			if let Some(c) = self.cache_usage.iter_mut().skip(1).find(|e| e.contains_key(&id)) {
                entry_state = c.remove(&id).unwrap();
			}
			self.cache_usage[0].insert(id, entry_state);
		}
	}

    pub fn note_write(&mut self, id: T) {
        let mut exist = true;
        {
            let entry_state = self.cache_usage[0].entry(id.clone()).or_insert_with(|| {
                exist = false;
                CacheEntryState::Dirty
            });
            *entry_state = CacheEntryState::Dirty;
        }

        if !exist {
            if let Some(c) = self.cache_usage.iter_mut().skip(1).find(|e| e.contains_key(&id)) {
                c.remove(&id);
            }
        }
    }

	/// Collects unused objects from cache.
	/// First params is the current size of the cache.
	/// Second one is an with objects to remove. It should also return new size of the cache.
	pub fn collect_garbage<F>(&mut self, current_size: usize, mut notify_unused: F) where F: FnMut(HashMap<T, CacheEntryState>) -> usize {
		if current_size < self.pref_cache_size {
			self.rotate_cache_if_needed();
			return;
		}

		for _ in 0..COLLECTION_QUEUE_SIZE {
			if let Some(back) = self.cache_usage.pop_back() {
				let current_size = notify_unused(back);
				self.cache_usage.push_front(Default::default());
				if current_size < self.max_cache_size {
					break
				}
			}
		}
	}

    /// Flush all the dirty items in cache to persistent storage
    pub fn flush<F>(&mut self, clear: bool, mut to_flush: F) where F: FnMut(&mut HashMap<T, CacheEntryState>) -> bool {
        for c in self.cache_usage.iter_mut() {
            let need_clean = to_flush(c);
            if clear {
                c.clear();
            } else if need_clean {
                for (_, val) in c.iter_mut() {
                    *val = CacheEntryState::Clean;
                }
            }
        }
    }

	fn rotate_cache_if_needed(&mut self) {
		if self.cache_usage.is_empty() { return }

		if self.cache_usage[0].len() * self.bytes_per_cache_entry > self.pref_cache_size / COLLECTION_QUEUE_SIZE {
			if let Some(cache) = self.cache_usage.pop_back() {
				self.cache_usage.push_front(cache);
			}
		}
	}
}
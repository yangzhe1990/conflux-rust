use super::super::{lfru::*, *};

struct CacheUtil<'a> {
    cache_algo_data: &'a mut [LFRUHandle<u32>],
    most_recent_key: Option<i32>,
}

impl<'a> CacheStoreUtil for CacheUtil<'a> {
    type CacheAlgoData = LFRUHandle<u32>;
    type ElementIndex = i32;

    fn get(&self, element_index: i32) -> LFRUHandle<u32> {
        match self.most_recent_key {
            None => {}
            Some(key) => {
                assert_ne!(key, element_index);
            }
        }

        let ret = self.cache_algo_data[element_index as usize];
        assert_eq!(true, ret.is_lru_hit());
        ret
    }

    fn get_most_recently_accessed(
        &self, element_index: i32,
    ) -> LFRUHandle<u32> {
        assert_eq!(Some(element_index), self.most_recent_key);
        self.cache_algo_data[element_index as usize]
    }

    fn set(&mut self, element_index: i32, algo_data: &LFRUHandle<u32>) {
        match self.most_recent_key {
            None => {}
            Some(key) => {
                assert_ne!(key, element_index);
            }
        }

        let old = self.cache_algo_data[element_index as usize];
        assert_eq!(true, old.is_lru_hit());

        self.cache_algo_data[element_index as usize] = *algo_data;
    }

    fn set_most_recently_accessed(
        &mut self, element_index: i32, algo_data: &LFRUHandle<u32>,
    ) {
        // If access then check the most recently accessed key.
        // If delete the most_recent_key is none and there is nothing to check.
        if self.most_recent_key.is_some() {
            assert_eq!(Some(element_index), self.most_recent_key);
        }
        self.cache_algo_data[element_index as usize] = *algo_data;
    }
}

impl<'a> CacheUtil<'a> {
    fn prepare(&mut self, key: i32) { self.most_recent_key = Some(key); }

    fn done(&mut self, key: i32) { self.most_recent_key.take(); }
}

#[derive(Debug)]
enum KeyActions {
    Access(i32),
    Delete(i32),
}

/// Check the correctness of the algorithm.
#[test]
fn test_lfru_algorithm_small_test() {
    let key_range = 10;

    let mut cache_algo_data =
        vec![LFRUHandle::<u32>::default(); key_range as usize];

    let mut cache_util = CacheUtil {
        cache_algo_data: &mut cache_algo_data,
        most_recent_key: None,
    };

    let mut lfru = LFRU::<u32, i32>::new(3, 6);

    let cache_actions = vec![
        KeyActions::Access(0),
        KeyActions::Access(1),
        KeyActions::Access(1),
        KeyActions::Access(2),
        KeyActions::Access(2),
        KeyActions::Access(2),
        KeyActions::Access(3),
        KeyActions::Access(4),
        KeyActions::Access(5),
        // Test deletion of non LFU item.
        KeyActions::Delete(0),
        KeyActions::Access(5),
        KeyActions::Access(1),
        KeyActions::Access(6),
        KeyActions::Access(7),
        KeyActions::Access(8),
        KeyActions::Access(9),
        // Test frequency counter of ghost item.
        KeyActions::Access(5),
        KeyActions::Access(1),
        KeyActions::Access(9),
        KeyActions::Delete(1),
        KeyActions::Access(7),
        KeyActions::Access(8),
        KeyActions::Access(9),
        KeyActions::Access(5),
        KeyActions::Access(1),
        // Test frequency counter of final state.
    ];

    for action in cache_actions.iter() {
        println!("action: {:?}", action);
        match *action {
            KeyActions::Delete(key) => {
                cache_util.done(key);
                if cache_util.cache_algo_data[key as usize].is_lru_hit() {
                    lfru.delete(key, &mut cache_util);
                    assert_eq!(
                        false,
                        cache_util.cache_algo_data[key as usize].is_lru_hit()
                    );
                }
            }
            KeyActions::Access(key) => {
                cache_util.prepare(key);
                lfru.access(key, &mut cache_util);
                cache_util.done(key);
            }
        }
    }

    // TODO(yz): check final state.
}

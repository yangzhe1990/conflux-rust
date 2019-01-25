mod ghost_lfu;
mod lru;
mod removable_heap;
use rand::{ChaChaRng, SeedableRng};

use super::CacheIndexTrait;

fn get_rng_for_test() -> ChaChaRng { ChaChaRng::from_seed([123; 32]) }

impl CacheIndexTrait for i32 {}

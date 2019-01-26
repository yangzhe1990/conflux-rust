mod lru;
mod recent_lfu;
mod removable_heap;
use rand::{ChaChaRng, SeedableRng};

use super::CacheIndexTrait;

fn get_rng_for_test() -> ChaChaRng { ChaChaRng::from_seed([123; 32]) }

impl CacheIndexTrait for i32 {}

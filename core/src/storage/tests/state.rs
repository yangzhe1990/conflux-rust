use super::super::{
    impls::{
        errors::*,
        merkle_patricia_trie::data_structure::{TrieNode, CHILDREN_COUNT},
    },
    state::*,
    state_manager::*,
};
use ethereum_types::H256;
use rand::{chacha::ChaChaRng, Rng, SeedableRng};
use std::mem;

fn generate_keys() -> Vec<[u8; 4]> {
    let number_of_keys = 100000;
    let mut rng = get_rng_for_test();

    let mut keys_num: Vec<u32> = Default::default();

    for i in 0..number_of_keys {
        keys_num.push(rng.gen());
    }

    keys_num.sort();

    let mut keys: Vec<[u8; 4]> = Default::default();
    let mut old_key = keys_num[0];
    for key in &keys_num[1..number_of_keys] {
        if (*key != old_key) {
            keys.push(unsafe { mem::transmute::<u32, [u8; 4]>(key.clone()) });
        }
        old_key = *key;
    }
    keys
}

fn get_rng_for_test() -> ChaChaRng { ChaChaRng::from_seed([123; 32]) }

#[test]
fn test_set_get() {
    let mut rng = get_rng_for_test();
    let mut state_manager = StateManager::default();
    let mut state = state_manager.get_state_at(H256::default());
    let mut keys: Vec<[u8; 4]> = generate_keys()
        .iter()
        .filter(|_| rng.gen_bool(0.5))
        .cloned()
        .collect();

    println!("Testing with {} set operations.", keys.len());

    for key in &keys {
        state.set(key, key).expect("Failed to insert key.");
    }

    rng.shuffle(keys.as_mut());

    for key in &keys {
        let value = state.get(key).expect("Failed to get key.");
        let equal = key.eq(value.as_ref());
        assert_eq!(equal, true);
    }

    let mut epoch_id = H256::default();
    epoch_id[0] = 1;
    state.commit(epoch_id);
}

#[test]
fn test_get_set_at_second_commit() {
    let mut rng = get_rng_for_test();
    let mut state_manager = StateManager::default();
    let mut keys: Vec<[u8; 4]> = generate_keys();
    rng.shuffle(&mut keys);
    let set_size = 10000;
    let (keys_0, keys_1_new, keys_remain, keys_1_overwritten) = (
        &keys[0..set_size * 2],
        &keys[set_size * 2..set_size * 3],
        &keys[0..set_size],
        &keys[set_size..set_size * 2],
    );

    let parent_epoch_0 = H256::default();
    let mut state_0 = state_manager.get_state_at(parent_epoch_0);
    println!("Setting state_0 0 with {} keys.", keys_0.len());

    for key in keys_0 {
        state_0.set(key, key).expect("Failed to insert key.");
    }

    let mut epoch_id_0 = H256::default();;
    epoch_id_0[0] = 1;
    state_0.commit(epoch_id_0);

    let mut state_1 = state_manager.get_state_at(epoch_id_0);
    println!("Set new {} keys for state_1.", keys_1_new.len(),);
    for key in keys_1_new {
        let value = vec![&key[..], &key[..]].concat();
        state_1
            .set(key, value.as_slice())
            .expect("Failed to insert key.");
    }

    println!(
        "Reading overlapping {} keys from state_0 and set new keys for state_1.",
        keys_1_overwritten.len(),
    );
    for key in keys_1_overwritten {
        let old_value = state_1.get(key).expect("Failed to get key.");
        let equal = key.eq(old_value.as_ref());
        assert_eq!(equal, true);
        let value = vec![&key[..], &key[..]].concat();
        state_1
            .set(key, value.as_slice())
            .expect("Failed to insert key.");
    }

    println!(
        "Reading untouched {} keys from state_0 in state_1.",
        keys_remain.len(),
    );
    for key in keys_remain {
        let value = state_1.get(key).expect("Failed to get key.");
        let equal = key.eq(value.as_ref());
        assert_eq!(equal, true);
    }

    println!(
        "Reading modified {} keys in state_1.",
        keys_1_overwritten.len(),
    );
    for key in keys_1_overwritten {
        let value = state_1.get(key).expect("Failed to get key.");
        let expected_value = vec![&key[..], &key[..]].concat();
        let equal = expected_value.eq(&value.as_ref());
        assert_eq!(equal, true);
    }

    let mut epoch_id_1 = H256::default();;
    epoch_id_1[0] = 2;
    state_1.commit(epoch_id_1);
}

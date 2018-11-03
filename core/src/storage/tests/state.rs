use super::super::{
    impls::{
        errors::*,
        merkle_patricia_trie::data_structure::{TrieNode, CHILDREN_COUNT},
    },
    state::*,
    state_manager::*,
};
use ethereum_types::H256;

fn generate_key_length_4() -> Vec<[u8; 2]> {
    let mut keys: Vec<[u8; 2]> = Default::default();

    for i0 in 0..CHILDREN_COUNT {
        for i1 in 0..CHILDREN_COUNT {
            for i2 in 0..CHILDREN_COUNT {
                for i3 in 0..CHILDREN_COUNT {
                    keys.push([
                        TrieNode::set_second_nibble(i0 as u8, i1 as u8),
                        TrieNode::set_second_nibble(i2 as u8, i3 as u8),
                    ]);
                }
            }
        }
    }

    keys
}

#[test]
fn test_set_get() {
    let mut state_manager = StateManager::default();
    let mut state = state_manager.get_state_at(H256::default());
    let keys = generate_key_length_4();

    for key in &keys {
        state.set(key, key).expect("Failed to insert key.");
    }

    for key in &keys {
        let value = state.get(key).expect("Failed to get key.");
        let equal = key.eq(value.as_ref());
        assert_eq!(equal, true);
    }

    let mut epoch_id_u8: [u8; 32] = Default::default();
    epoch_id_u8[0] = 1;
    state.commit(epoch_id_u8.into());
}

use super::super::impls::merkle_patricia_trie::{
    data_structure::*,
    node_memory_manager::{MaybeNodeRef, NodeRef},
};
use rlp::*;

#[test]
fn test_rlp_encode_decode() {
    // MaybeNodeRef
    let x = MaybeNodeRef::new(1234);
    let rlp_bytes = x.rlp_bytes();
    assert_eq!(
        MaybeNodeRef::decode(&Rlp::new(rlp_bytes.as_slice())).unwrap(),
        x
    );

    // Non-empty OwnedChildrenTable
    let mut owned_children_table = Some(Box::new(ChildrenTable::default()));
    for i in 0..CHILDREN_COUNT {
        owned_children_table.as_mut().unwrap()[i] =
            MaybeNodeRef::new(i as u32 * 16384);
    }
    owned_children_table.as_mut().unwrap()[11] =
        NodeRef::Committed { db_key: 0 }.into();
    let x = OwnedChildrenTableRef {
        children_table_ref: &owned_children_table,
    };
    let rlp_bytes = x.rlp_bytes();
    let rlp_parsed =
        OwnedChildrenTableWrapper::decode(&Rlp::new(rlp_bytes.as_slice()))
            .unwrap();
    assert_eq!(
        owned_children_table.as_ref().unwrap().as_ref().as_ref(),
        rlp_parsed
            .owned_children_table
            .as_ref()
            .unwrap()
            .as_ref()
            .as_ref()
    );

    // Empty OwnedChildrenTable
    let empty_owned_children_table: OwnedChildrenTable = None;
    let x = OwnedChildrenTableRef {
        children_table_ref: &empty_owned_children_table,
    };
    let rlp_bytes = x.rlp_bytes();
    let rlp_parsed =
        OwnedChildrenTableWrapper::decode(&Rlp::new(rlp_bytes.as_slice()))
            .unwrap();
    assert_eq!(rlp_parsed.owned_children_table.is_none(), true);

    // TrieNode without compressed path.
    let x = TrieNode::new(
        &Default::default(),
        owned_children_table.clone(),
        b"asdf",
        Default::default(),
    );
    let rlp_bytes = x.rlp_bytes();
    let rlp_parsed = TrieNode::decode(&Rlp::new(rlp_bytes.as_slice())).unwrap();
}

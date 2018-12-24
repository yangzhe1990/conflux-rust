use ethereum_types::H256;
pub type MerkleHash = H256;
pub type ChildrenMerkleTable = [MerkleHash; CHILDREN_COUNT];
pub type MaybeMerkleTable = Option<ChildrenMerkleTable>;
pub type MaybeMerkleTableRef<'a> = Option<&'a ChildrenMerkleTable>;

use super::data_structure::*;
use crate::hash::{keccak, KECCAK_EMPTY};
use rlp::*;

pub const MERKLE_NULL_NODE: MerkleHash = KECCAK_EMPTY;

pub fn compute_merkle_for_rlp(rlp_stream: &RlpStream) -> MerkleHash {
    keccak(rlp_stream.as_raw())
}

pub fn compute_node_merkle(
    children_merkles: MaybeMerkleTableRef, value: &[u8],
) -> MerkleHash {
    let mut rlp_stream = RlpStream::new();
    rlp_stream.begin_unbounded_list();
    match children_merkles {
        Some(merkles) => {
            rlp_stream.append_list(merkles);
        }
        _ => {}
    }
    rlp_stream.append(&value).complete_unbounded_list();

    compute_merkle_for_rlp(&rlp_stream)
}

fn compute_path_merkle(
    compressed_path: CompressedPathRef, node_merkle: &MerkleHash,
) -> MerkleHash {
    if compressed_path.path_slice.len() != 0 {
        let mut rlp_stream = RlpStream::new_list(3);
        compressed_path.rlp_append_parts(&mut rlp_stream);
        rlp_stream.append(node_merkle);

        compute_merkle_for_rlp(&rlp_stream)
    } else {
        *node_merkle
    }
}

pub fn compute_merkle(
    compressed_path: CompressedPathRef, children_merkles: MaybeMerkleTableRef,
    value: &[u8],
) -> MerkleHash
{
    let node_merkle = compute_node_merkle(children_merkles, value);
    let path_merkle = compute_path_merkle(compressed_path, &node_merkle);

    path_merkle
}

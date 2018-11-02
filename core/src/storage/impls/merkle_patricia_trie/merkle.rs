use ethereum_types::H256;
pub type MerkleHash = H256;
pub type ChildrenMerkleTable = [MerkleHash; CHILDREN_COUNT];
pub type MaybeMerkleTable = Option<ChildrenMerkleTable>;

use super::data_structure::*;
use hash::{keccak, KECCAK_EMPTY};
use rlp::*;

pub const MERKLE_NULL_NODE: MerkleHash = KECCAK_EMPTY;

pub fn compute_merkle_for_rlp(rlp_stream: &RlpStream) -> MerkleHash {
    keccak(rlp_stream.as_raw())
}

pub fn compute_merkle(
    compressed_path: CompressedPathRef, children_merkles: MaybeMerkleTable,
    value: &[u8],
) -> MerkleHash
{
    let mut merkle: MerkleHash;
    {
        let mut rlp_stream = RlpStream::new();
        rlp_stream.begin_unbounded_list();
        match children_merkles {
            Some(ref merkles) => {
                rlp_stream.append_list(merkles);
            }
            _ => {}
        }
        rlp_stream.append(&value).complete_unbounded_list();
        merkle = compute_merkle_for_rlp(&rlp_stream);
    }

    if compressed_path.path_slice.len() != 0 {
        let mut rlp_stream = RlpStream::new_list(3);
        rlp_stream
            .append(&compressed_path.end_mask)
            .append(&compressed_path.path_slice)
            .append(&merkle);

        merkle = compute_merkle_for_rlp(&rlp_stream);
    }

    merkle
}

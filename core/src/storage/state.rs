use super::impls::{errors::*, merkle_patricia_trie::merkle::MerkleHash};
use primitives::EpochId;

/// A block defines a list of transactions that it sees and the sequence of
/// the transactions (ledger). At the view of a block, after all
/// transactions being executed, the data associated with all addresses is
/// a State after the epoch defined by the block.
///
/// A writable state is copy-on-write reference to the base state in the
/// state union. State is supposed to be owned by single user.
pub use super::impls::state::State;

// The trait is created to separate the implementation to another file, and the
// concrete struct is put into inner mod, because the implementation is
// anticipated to be too complex to present in the same file of the API.
// TODO(yz): check if this is the best way to organize code for this library.
pub trait StateTrait {
    // Status.
    /// Check if the state exists. If not any state action operates on an empty
    /// state.
    fn does_exist(&self) -> bool;
    /// Merkle hash
    fn get_merkle_hash(&self) -> Result<Option<MerkleHash>>;

    // Actions.
    fn get(&self, access_key: &[u8]) -> Result<Box<[u8]>>;
    fn set(&mut self, access_key: &[u8], value: &[u8]) -> Result<()>;
    fn delete(&mut self, access_key: &[u8]) -> Result<Vec<u8>>;
    // Delete everything prefixed by access_key and return
    // Contaianer must be a container or type which ignores all inserts. So far
    // there is no standard container traits that we can directly apply.
    // TODO(yz): maybe implement some constrains for Contaianer?
    fn delete_all(&mut self, access_key_prefix: &[u8]) -> Result<()>;

    // Finalize
    /// It's costly to compute state root however it's only necessary to compute
    /// state root once before committing.
    fn compute_state_root(&mut self) -> Result<MerkleHash>;
    fn commit(&mut self, epoch: EpochId) -> Result<()>;
    fn revert(&mut self);

    // TODO(yz): verifiable proof related methods.
}

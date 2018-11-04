use super::{
    impls::{errors::*, merkle_patricia_trie::merkle::MerkleHash},
    state_manager::StateManager,
};
use execution::EpochId;

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
pub trait StateTrait<'a> {
    // Actions.
    fn get(&self, access_key: &[u8]) -> Result<Box<[u8]>>;
    fn set(&mut self, access_key: &[u8], value: &[u8]) -> Result<()>;
    fn delete(&mut self, access_key: &[u8]) -> Result<Vec<u8>>;
    // Delete everything prefixed by access_key and return
    // Contaianer must be a container or type which ignores all inserts. So far
    // there is no standard container traits that we can directly apply.
    // TODO(yz): maybe implement some constrains for Contaianer?
    fn delete_all<Contaianer>(
        &mut self, access_key_prefix: &[u8], removed_kvs: Contaianer,
    ) -> Result<()>;

    // Finalize
    fn commit(&mut self, epoch: EpochId) -> MerkleHash;

    // TODO(yz): verifiable proof related methods.
}

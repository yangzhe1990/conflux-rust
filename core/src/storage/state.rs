use super::state_union::StateUnion;
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
    // Constructors.
    fn load_from_union(union: &'a StateUnion, epoch_id: EpochId) -> Self;
    fn create_for_transaction_execution(
        union: &'a StateUnion, parent_epoch_id: EpochId, epoch_id: EpochId,
    ) -> Self;

    // Actions.
    fn get(&'a mut self, access_key: &[u8]) -> &'a mut Vec<u8>;
    fn set(&mut self, access_key: &[u8], value: &[u8]);
    fn delete(&mut self, access_key: &[u8]) -> Vec<u8>;
    // Delete everything prefixed by access_key and return
    // Contaianer must be a container or type which ignores all inserts. So far
    // there is no standard container traits that we can directly apply.
    // TODO(yz): maybe implement some constrains for Contaianer?
    fn delete_all<Contaianer>(
        &mut self, access_key_prefix: &[u8], removed_kvs: Contaianer,
    );

    // Finalize
    fn commit();

    // TODO(yz): verifiable proof related methods.

    // Panic if commit() isn't called.
    fn drop();
}

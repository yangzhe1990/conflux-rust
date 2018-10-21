use super::state::State;
use execution::EpochId;
use snapshot::snapshot::Snapshot;

// State Union is the single entry-point to access State for any epoch.
// StateUnion has Internal mutability and is thread-safe.
pub use super::impls::state_union::StateUnion;

// The trait is created to separate the implementation to another file, and the
// concrete struct is put into inner mod, because the implementation is
// anticipated to be too complex to present in the same file of the API.
// TODO(yz): check if this is the best way to organize code for this library.
pub trait StateUnionTrait<'a> {
    fn load_from_snapshot(snapshot: &Snapshot) -> Self;
    fn make_snapshot(&self, end_block_id: EpochId) -> Snapshot;
    // Readonly state.
    fn get_state_at(&'a self, epoch_id: EpochId) -> State<'a>;
    // Writable state.
    fn get_state_for_transaction_execution(
        &'a self, epoch_id: EpochId, parent_epoch_id: EpochId,
    ) -> State<'a>;
    fn drop_state_outside(&self, end_block_id: EpochId);
}

use super::state::State;
use primitives::EpochId;
use snapshot::snapshot::Snapshot;

// StateManager is the single entry-point to access State for any epoch.
// StateManager has Internal mutability and is thread-safe.
pub use super::impls::state_manager::StateManager;

// The trait is created to separate the implementation to another file, and the
// concrete struct is put into inner mod, because the implementation is
// anticipated to be too complex to present in the same file of the API.
// TODO(yz): check if this is the best way to organize code for this library.
pub trait StateManagerTrait {
    fn from_snapshot(snapshot: &Snapshot) -> Self;
    fn make_snapshot(&self, epoch_id: EpochId) -> Snapshot;
    fn get_state_at(&self, epoch_id: EpochId) -> State;
    fn drop_state_outside(&self, epoch_id: EpochId);
}

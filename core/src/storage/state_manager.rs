use super::{impls::errors::*, state::State};
use primitives::EpochId;
use crate::snapshot::snapshot::Snapshot;
use std::sync::Arc;

// StateManager is the single entry-point to access State for any epoch.
// StateManager has Internal mutability and is thread-safe.
pub use super::impls::state_manager::StateManager;

pub type SharedStateManager = Arc<StateManager>;

// The trait is created to separate the implementation to another file, and the
// concrete struct is put into inner mod, because the implementation is
// anticipated to be too complex to present in the same file of the API.
// TODO(yz): check if this is the best way to organize code for this library.
pub trait StateManagerTrait {
    fn from_snapshot(snapshot: &Snapshot) -> Self;
    fn make_snapshot(&self, epoch_id: EpochId) -> Snapshot;
    /// Even for non-existing the method returns a State because we need a way
    /// to create the genesis State. However there should be a special
    /// epoch_id to create the genesis State.
    //  TODO(yz): special epoch_id for empty state.
    fn get_state_at(&self, epoch_id: EpochId) -> Result<State>;
    fn drop_state_outside(&self, epoch_id: EpochId);
}

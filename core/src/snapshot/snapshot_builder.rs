use super::snapshot::Snapshot;

// Build snapshot
pub struct SnapshotBuilder {}

trait SnapshotBuilderTrait {
    // TODO(yz): methods to add content into snapshot.

    fn build() -> Snapshot;
}

use super::snapshot::Snapshot;

// Build snapshot
#[allow(dead_code)]
pub struct SnapshotBuilder {}

trait SnapshotBuilderTrait {
    // TODO(yz): methods to add content into snapshot.

    fn build() -> Snapshot;
}

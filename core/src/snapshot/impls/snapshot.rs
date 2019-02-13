use super::super::snapshot::*;

use std::path::Path;

impl SnapshotTrait for Snapshot {
    fn from_file(_path: &Path) -> Self {
        unimplemented!()
    }
}

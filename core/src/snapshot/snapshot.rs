use std::path::Path;

// Conflux snapshot wire-format.
pub struct Snapshot {
    // TODO(yz): implement.
}

// The trait is created to separate the implementation to another file, and the
// concrete struct is put into inner mod, because the implementation is
// anticipated to be too complex to present in the same file of the API.
// TODO(yz): check if this is the best way to organize code for this library.
pub trait SnapshotTrait {
    fn from_file(path: &Path) -> Self;
}

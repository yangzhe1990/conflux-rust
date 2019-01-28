// TODO(yz): remember to remove.
#![allow(dead_code, unused_variables)]

pub mod state;
pub mod state_manager;

#[cfg(test)]
pub mod tests;

mod impls;

pub use self::{
    impls::{
        errors::{Error, ErrorKind, Result},
        multi_version_merkle_patricia_trie::merkle_patricia_trie::MerkleHash,
    },
    state::{State as Storage, StateTrait as StorageTrait},
    state_manager::{
        StateManager as StorageManager,
        StateManagerTrait as StorageManagerTrait,
    },
};

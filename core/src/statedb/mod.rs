use primitives::EpochId;
use storage::{
    Error as StorageError, ErrorKind as StorageErrorKind, MerkleHash, Storage,
    StorageTrait,
};

mod error;

pub use self::error::{Error, ErrorKind, Result};

pub struct StateDb<'a> {
    storage: Storage<'a>,
}

impl<'a> StateDb<'a> {
    pub fn new(storage: Storage<'a>) -> Self { StateDb { storage } }

    pub fn get<T>(&self, key: &[u8]) -> Result<Option<T>>
    where T: ::rlp::Decodable {
        let raw = match self.storage.get(key) {
            Ok(raw) => raw,
            Err(StorageError(StorageErrorKind::MPTKeyNotFound, _)) => {
                return Ok(None);
            }
            Err(e) => {
                return Err(e.into());
            }
        };
        //        println!("get key={:?} value={:?}", key, raw);
        Ok(Some(::rlp::decode::<T>(raw.as_ref())?))
    }

    pub fn get_raw(&self, key: &[u8]) -> Result<Option<Box<[u8]>>> {
        let raw = match self.storage.get(key) {
            Ok(raw) => raw,
            Err(StorageError(StorageErrorKind::MPTKeyNotFound, _)) => {
                return Ok(None);
            }
            Err(e) => {
                return Err(e.into());
            }
        };
        Ok(Some(raw))
    }

    pub fn set<T>(&mut self, key: &[u8], value: &T) -> Result<()>
    where T: ::rlp::Encodable {
        //        println!("set key={:?} value={:?}", key,
        // ::rlp::encode(value));
        match self.storage.set(key, ::rlp::encode(value).as_ref()) {
            Ok(_) => Ok(()),
            Err(StorageError(StorageErrorKind::MPTKeyNotFound, _)) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn delete(&mut self, key: &[u8]) -> Result<()> {
        match self.storage.delete(key) {
            Ok(_) => Ok(()),
            Err(StorageError(StorageErrorKind::MPTKeyNotFound, _)) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn delete_all(&mut self, key_prefix: &[u8]) -> Result<()> {
        Ok(self.storage.delete_all(key_prefix)?)
    }

    pub fn commit(&mut self, epoch_id: EpochId) -> Result<MerkleHash> {
        let merkle_hash = self.storage.compute_state_root()?;
        self.storage.commit(epoch_id)?;

        Ok(merkle_hash)
    }
}

// Copyright 2020 Conflux Foundation. All rights reserved.
// Conflux is free software and distributed under GNU General Public License.
// See http://www.gnu.org/licenses/

#[cfg(test)]
#[test]
pub fn test_code_size()
{
    fn dir_size(path: impl Into<PathBuf>) -> io::Result<u64> {
        fn dir_size(mut dir: fs::ReadDir) -> io::Result<u64> {
            dir.try_fold(0, |acc, file| {
                let file = file?;
                let size = match file.metadata()? {
                    data if data.is_dir() => dir_size(fs::read_dir(file.path())?)?,
                    data => data.len(),
                };
                Ok(acc + size)
            })
        }

        dir_size(fs::read_dir(path.into())?)
    }

    const PATH_STR: &'static str = "./tmp/";
    let path = Path::new(&PATH_STR);
    let already_open_snapshots: AlreadyOpenSnapshots<SnapshotDbSqlite> = Default::default();
    let open_snapshot_semaphore: Arc<Semaphore> = Arc::new(Semaphore::new(
        1 as usize,
    ));
    let mut snapshot_db_sqlite = SnapshotDbSqlite::create(
        path,
        &already_open_snapshots,
        &open_snapshot_semaphore,
    ).unwrap();
    
    println!("init {}", dir_size(&PATH_STR).unwrap());

    const LEN: usize = 10 * 1024;
    const ROUNDS: u32 = 10;
    const KV_PER_ROUND: u32 = 10000;

    const CODE_HASH: H256 = KECCAK_EMPTY;

    for _i in 0..ROUNDS {
        for _j in 0..KV_PER_ROUND {
            let address = Address::random();

            let key = StorageKey::new_code_key(&address, &CODE_HASH).to_key_bytes();

            let code = random_string(LEN).as_bytes().to_vec();
            let code_info = CodeInfo {
                code: Arc::new(code),
                owner: address,
            };
            let value = ::rlp::encode(&code_info).into_boxed_slice();

            snapshot_db_sqlite.put(&key, &value).expect("insert kv");
        }
        println!("round {}: {}", _i, dir_size(&PATH_STR).unwrap());
    }

    drop(snapshot_db_sqlite);

    fs::remove_dir_all(path).expect("remove dir");
}

#[cfg(test)]
pub fn open_snapshot_db_for_testing(
    snapshot_path: &Path, readonly: bool,
) -> Result<SnapshotDbSqlite> {
    SnapshotDbSqlite::open(
        snapshot_path,
        readonly,
        &Default::default(),
        &Arc::new(Semaphore::new(DEFAULT_MAX_OPEN_SNAPSHOTS as usize)),
    )
}

pub trait MptValueKind: Debug {
    fn value_eq(&self, maybe_value: Option<&[u8]>) -> bool;
}

impl MptValueKind for () {
    fn value_eq(&self, maybe_value: Option<&[u8]>) -> bool {
        maybe_value.is_none()
    }
}

impl MptValueKind for Box<[u8]> {
    fn value_eq(&self, maybe_value: Option<&[u8]>) -> bool {
        maybe_value.map_or(false, |v| v.eq(&**self))
    }
}

pub fn check_key_value_load<Value: MptValueKind>(
    snapshot_db: &SnapshotDbSqlite,
    mut kv_iter: impl FallibleIterator<Item = (Vec<u8>, Value), Error = Error>,
    check_value: bool,
) -> Result<u64>
{
    let mut checker_count = 0;
    let mut mpt = snapshot_db.open_snapshot_mpt_shared()?;

    let mut cursor = MptCursor::<
        &mut dyn SnapshotMptTraitRead,
        BasicPathNode<&mut dyn SnapshotMptTraitRead>,
    >::new(&mut mpt);
    cursor.load_root()?;
    while let Some((access_key, expected_value)) = kv_iter.next()? {
        let terminal =
            cursor.open_path_for_key::<access_mode::Read>(&access_key)?;
        if check_value {
            let mpt_value = match terminal {
                CursorOpenPathTerminal::Arrived => {
                    cursor.current_node_mut().value_as_slice().into_option()
                }
                CursorOpenPathTerminal::ChildNotFound { .. } => None,
                CursorOpenPathTerminal::PathDiverted(_) => None,
            };
            if !expected_value.value_eq(mpt_value) {
                error!(
                    "mpt value doesn't match snapshot kv. Expected {:?}, got {:?}",
                    expected_value, mpt_value,
                );
            }
        }
        checker_count += 1;
    }
    cursor.finish()?;

    Ok(checker_count)
}

use crate::{
    impls::{
        errors::*,
        merkle_patricia_trie::{
            mpt_cursor::{BasicPathNode, CursorOpenPathTerminal, MptCursor},
            TrieNodeTrait,
        },
        storage_db::snapshot_db_sqlite::SnapshotDbSqlite,
    },
    storage_db::{snapshot_db::OpenSnapshotMptTrait, SnapshotMptTraitRead},
    utils::access_mode,
};
use fallible_iterator::FallibleIterator;
use std::fmt::Debug;

#[cfg(test)]
use crate::impls::{
    defaults::DEFAULT_MAX_OPEN_SNAPSHOTS,
    storage_db::{
        snapshot_db_sqlite::SnapshotDbTrait,
        snapshot_db_manager_sqlite::AlreadyOpenSnapshots,
    },
};
#[cfg(test)]
use crate::{storage_db::KeyValueDbTraitSingleWriter};
#[cfg(test)]
use std::{path::{Path, PathBuf}, sync::Arc, fs, io};
#[cfg(test)]
use tokio::sync::Semaphore;
#[cfg(test)]
use cfx_types::Address;
#[cfg(test)]
use keccak_hash::{KECCAK_EMPTY, H256};
#[cfg(test)]
use primitives::{StorageKey, CodeInfo};
#[cfg(test)]
use cfxstore::random_string;

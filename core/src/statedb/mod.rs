// Copyright 2019 Conflux Foundation. All rights reserved.
// Conflux is free software and distributed under GNU General Public License.
// See http://www.gnu.org/licenses/

use crate::{
    executive::STORAGE_INTEREST_STAKING_CONTRACT_ADDRESS,
    parameters::staking::*,
    storage::{
        utils::access_mode, Error as StorageError,
        ErrorKind as StorageErrorKind, MptKeyValue, NodeMerkleProof,
        StateProof, StateRootWithAuxInfo, StorageState, StorageStateTrait,
    },
};
use cfx_types::{Address, H256, U256};
use primitives::{
    Account, CodeInfo, DepositList, EpochId, StorageKey, StorageLayout,
    StorageRoot, VoteStakeList,
};

mod error;

pub use self::error::{Error, ErrorKind, Result};
use crate::{
    consensus::debug::{ComputeEpochDebugRecord, StateOp},
    storage::state::{NoProof, WithProof},
};
use rlp::Rlp;

pub struct StateDb {
    storage: StorageState,
}

impl StateDb {
    const ACCUMULATE_INTEREST_RATE_KEY: &'static [u8] =
        b"accumulate_interest_rate";
    const INTEREST_RATE_KEY: &'static [u8] = b"interest_rate";
    const TOTAL_BANK_TOKENS_KEY: &'static [u8] = b"total_staking_tokens";
    const TOTAL_STORAGE_TOKENS_KEY: &'static [u8] = b"total_storage_tokens";
    const TOTAL_TOKENS_KEY: &'static [u8] = b"total_issued_tokens";

    pub fn new(storage: StorageState) -> Self { StateDb { storage } }

    #[allow(unused)]
    pub fn get_storage_mut(&mut self) -> &mut StorageState { &mut self.storage }

    pub fn get<T>(&self, key: StorageKey) -> Result<Option<T>>
    where T: ::rlp::Decodable {
        match self.storage.get(key) {
            Ok(None) => Ok(None),
            Ok(Some(raw)) => Ok(Some(::rlp::decode::<T>(raw.as_ref())?)),
            Err(e) => bail!(e),
        }
    }

    pub fn get_code(
        &self, address: &Address, code_hash: &H256,
    ) -> Result<Option<CodeInfo>> {
        self.get::<CodeInfo>(StorageKey::new_code_key(address, code_hash))
    }

    pub fn get_deposit_list(
        &self, address: &Address,
    ) -> Result<Option<DepositList>> {
        self.get::<DepositList>(StorageKey::new_deposit_list_key(address))
    }

    pub fn get_vote_list(
        &self, address: &Address,
    ) -> Result<Option<VoteStakeList>> {
        self.get::<VoteStakeList>(StorageKey::new_vote_list_key(address))
    }

    pub fn get_storage_layout(
        &self, address: &Address,
    ) -> Result<Option<StorageLayout>> {
        match self.get_raw(StorageKey::new_storage_root_key(address))? {
            None => Ok(None),
            Some(raw) => Ok(Some(StorageLayout::from_bytes(raw.as_ref())?)),
        }
    }

    pub fn set_storage_layout(
        &mut self, address: &Address, layout: &StorageLayout,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> Result<()>
    {
        let key = StorageKey::new_storage_root_key(address);
        self.set_raw(key, layout.to_bytes().into_boxed_slice(), debug_record)
    }

    pub fn get_account(&self, address: &Address) -> Result<Option<Account>> {
        match self.storage.get(StorageKey::new_account_key(address)) {
            Ok(None) => Ok(None),
            Ok(Some(raw)) => {
                Ok(Some(Account::new_from_rlp(*address, &Rlp::new(&raw))?))
            }
            Err(e) => bail!(e),
        }
    }

    pub fn get_storage_root(
        &self, address: &Address,
    ) -> Result<Option<StorageRoot>> {
        let key = StorageKey::new_storage_root_key(address);

        let (triplet, _) =
            self.storage.get_node_merkle_all_versions::<NoProof>(key)?;

        Ok(StorageRoot::from_node_merkle_triplet(triplet))
    }

    pub fn get_storage_root_with_proof(
        &self, address: &Address,
    ) -> Result<(Option<StorageRoot>, NodeMerkleProof)> {
        let key = StorageKey::new_storage_root_key(address);

        let (triplet, proof) = self
            .storage
            .get_node_merkle_all_versions::<WithProof>(key)?;

        let root = StorageRoot::from_node_merkle_triplet(triplet);
        Ok((root, proof))
    }

    pub fn get_raw(&self, key: StorageKey) -> Result<Option<Box<[u8]>>> {
        let r = Ok(self.storage.get(key)?);
        trace!("get_raw key={:?}, value={:?}", key, r);
        r
    }

    pub fn get_raw_with_proof(
        &self, key: StorageKey,
    ) -> Result<(Option<Box<[u8]>>, StateProof)> {
        let r = Ok(self.storage.get_with_proof(key)?);
        trace!("get_raw_with_proof key={:?}, value={:?}", key, r);
        r
    }

    pub fn set<T>(
        &mut self, key: StorageKey, value: &T,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> Result<()>
    where
        T: ::rlp::Encodable,
    {
        self.set_raw(key, ::rlp::encode(value).into_boxed_slice(), debug_record)
    }

    pub fn set_raw(
        &mut self, key: StorageKey, value: Box<[u8]>,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> Result<()>
    {
        if let Some(record) = debug_record {
            record.state_ops.push(StateOp::StorageLevelOp {
                op_name: "set".into(),
                key: key.to_key_bytes(),
                maybe_value: Some(value.clone().into()),
            })
        }
        match self.storage.set(key, value) {
            Ok(_) => Ok(()),
            Err(StorageError(StorageErrorKind::MPTKeyNotFound, _)) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn delete(
        &mut self, key: StorageKey,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> Result<()>
    {
        if let Some(record) = debug_record {
            record.state_ops.push(StateOp::StorageLevelOp {
                op_name: "delete".into(),
                key: key.to_key_bytes(),
                maybe_value: None,
            })
        }
        match self.storage.delete(key) {
            Ok(_) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn delete_all<AM: access_mode::AccessMode>(
        &mut self, key_prefix: StorageKey,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> Result<Option<Vec<MptKeyValue>>>
    {
        if let Some(record) = debug_record {
            record.state_ops.push(StateOp::StorageLevelOp {
                op_name: if AM::is_read_only() {
                    "iterate"
                } else {
                    "delete_all"
                }
                .into(),
                key: key_prefix.to_key_bytes(),
                maybe_value: None,
            })
        }
        Ok(self.storage.delete_all::<AM>(key_prefix)?)
    }

    /// This method is only used for genesis block because state root is
    /// required to compute genesis epoch_id. For other blocks there are
    /// deferred execution so the state root computation is merged inside
    /// commit method.
    pub fn compute_state_root(&mut self) -> Result<StateRootWithAuxInfo> {
        Ok(self.storage.compute_state_root()?)
    }

    pub fn commit(
        &mut self, epoch_id: EpochId,
    ) -> Result<StateRootWithAuxInfo> {
        let result = self.compute_state_root();
        self.storage.commit(epoch_id)?;

        result
    }

    pub fn get_annual_interest_rate(&self) -> Result<U256> {
        let interest_rate_key = StorageKey::new_storage_key(
            &STORAGE_INTEREST_STAKING_CONTRACT_ADDRESS,
            Self::INTEREST_RATE_KEY,
        );
        let interest_rate_opt = self.get::<U256>(interest_rate_key)?;
        Ok(interest_rate_opt.unwrap_or(
            *INITIAL_INTEREST_RATE_PER_BLOCK * U256::from(BLOCKS_PER_YEAR),
        ))
    }

    pub fn get_accumulate_interest_rate(&self) -> Result<U256> {
        let acc_interest_rate_key = StorageKey::new_storage_key(
            &STORAGE_INTEREST_STAKING_CONTRACT_ADDRESS,
            Self::ACCUMULATE_INTEREST_RATE_KEY,
        );
        let acc_interest_rate_opt = self.get::<U256>(acc_interest_rate_key)?;
        Ok(acc_interest_rate_opt.unwrap_or(*ACCUMULATED_INTEREST_RATE_SCALE))
    }

    pub fn get_total_issued_tokens(&self) -> Result<U256> {
        let total_issued_tokens_key = StorageKey::new_storage_key(
            &STORAGE_INTEREST_STAKING_CONTRACT_ADDRESS,
            Self::TOTAL_TOKENS_KEY,
        );
        let total_issued_tokens_opt =
            self.get::<U256>(total_issued_tokens_key)?;
        Ok(total_issued_tokens_opt.unwrap_or(U256::zero()))
    }

    pub fn get_total_staking_tokens(&self) -> Result<U256> {
        let total_staking_tokens_key = StorageKey::new_storage_key(
            &STORAGE_INTEREST_STAKING_CONTRACT_ADDRESS,
            Self::TOTAL_BANK_TOKENS_KEY,
        );
        let total_staking_tokens_opt =
            self.get::<U256>(total_staking_tokens_key)?;
        Ok(total_staking_tokens_opt.unwrap_or(U256::zero()))
    }

    pub fn get_total_storage_tokens(&self) -> Result<U256> {
        let total_storage_tokens_key = StorageKey::new_storage_key(
            &STORAGE_INTEREST_STAKING_CONTRACT_ADDRESS,
            Self::TOTAL_STORAGE_TOKENS_KEY,
        );
        let total_storage_tokens_opt =
            self.get::<U256>(total_storage_tokens_key)?;
        Ok(total_storage_tokens_opt.unwrap_or(U256::zero()))
    }

    pub fn set_annual_interest_rate(
        &mut self, interest_rate: &U256,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> Result<()>
    {
        let interest_rate_key = StorageKey::new_storage_key(
            &STORAGE_INTEREST_STAKING_CONTRACT_ADDRESS,
            Self::INTEREST_RATE_KEY,
        );
        self.set::<U256>(interest_rate_key, interest_rate, debug_record)
    }

    pub fn set_accumulate_interest_rate(
        &mut self, accumulate_interest_rate: &U256,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> Result<()>
    {
        let acc_interest_rate_key = StorageKey::new_storage_key(
            &STORAGE_INTEREST_STAKING_CONTRACT_ADDRESS,
            Self::ACCUMULATE_INTEREST_RATE_KEY,
        );
        self.set::<U256>(
            acc_interest_rate_key,
            accumulate_interest_rate,
            debug_record,
        )
    }

    pub fn set_total_issued_tokens(
        &mut self, total_issued_tokens: &U256,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> Result<()>
    {
        let total_issued_tokens_key = StorageKey::new_storage_key(
            &STORAGE_INTEREST_STAKING_CONTRACT_ADDRESS,
            Self::TOTAL_TOKENS_KEY,
        );
        self.set::<U256>(
            total_issued_tokens_key,
            total_issued_tokens,
            debug_record,
        )
    }

    pub fn set_total_staking_tokens(
        &mut self, total_staking_tokens: &U256,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> Result<()>
    {
        let total_staking_tokens_key = StorageKey::new_storage_key(
            &STORAGE_INTEREST_STAKING_CONTRACT_ADDRESS,
            Self::TOTAL_BANK_TOKENS_KEY,
        );
        self.set::<U256>(
            total_staking_tokens_key,
            total_staking_tokens,
            debug_record,
        )
    }

    pub fn set_total_storage_tokens(
        &mut self, total_storage_tokens: &U256,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> Result<()>
    {
        let total_storage_tokens_key = StorageKey::new_storage_key(
            &STORAGE_INTEREST_STAKING_CONTRACT_ADDRESS,
            Self::TOTAL_STORAGE_TOKENS_KEY,
        );
        self.set::<U256>(
            total_storage_tokens_key,
            total_storage_tokens,
            debug_record,
        )
    }
}

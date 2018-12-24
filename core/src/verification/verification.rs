use crate::error::{BlockError, Error};
//use ethereum_types::{H256, U256};
use crate::{pow, triehash::ordered_trie_root};
use primitives::{Block, BlockHeader};
use rlp::encode;
use std::{
    collections::HashSet,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use unexpected::{Mismatch, OutOfBounds};

#[derive(Debug, Copy, Clone)]
pub struct VerificationConfig {
    pub verify_timestamp: bool,
}

impl VerificationConfig {
    pub fn new(test_mode: bool) -> Self {
        if test_mode {
            VerificationConfig {
                verify_timestamp: false,
            }
        } else {
            VerificationConfig {
                verify_timestamp: true,
            }
        }
    }

    /// Check basic header parameters.
    /// This does not require header to be graph or parental tree ready.
    pub fn verify_header_params(
        &self, header: &BlockHeader,
    ) -> Result<(), Error> {
        // verify POW
        let boundary = pow::difficulty_to_boundary(header.difficulty());
        let pow_hash = pow::compute(header.nonce(), &header.problem_hash());
        if pow_hash >= boundary {
            warn!("block {} has invalid proof of work. boundary: {}, pow_hash: {}", header.hash(), boundary.clone(), pow_hash.clone());
            return Err(From::from(BlockError::InvalidProofOfWork(
                OutOfBounds {
                    min: None,
                    max: Some(boundary),
                    found: pow_hash,
                },
            )));
        }

        // verify non-duplicated parent and referee hashes
        let mut direct_ancestor_hashes = HashSet::new();
        let parent_hash = header.parent_hash();
        direct_ancestor_hashes.insert(parent_hash.clone());
        for referee_hash in header.referee_hashes() {
            if direct_ancestor_hashes.contains(referee_hash) {
                warn!(
                    "block {} has duplicate parent or referee hashes",
                    header.hash()
                );
                return Err(From::from(
                    BlockError::DuplicateParentOrRefereeHashes(
                        referee_hash.clone(),
                    ),
                ));
            }
            direct_ancestor_hashes.insert(referee_hash.clone());
        }

        // verify timestamp drift
        if self.verify_timestamp {
            const ACCEPTABLE_DRIFT: Duration = Duration::from_secs(15);
            let max_time = SystemTime::now() + ACCEPTABLE_DRIFT;
            let invalid_threshold = max_time + ACCEPTABLE_DRIFT * 9;
            let timestamp =
                UNIX_EPOCH + Duration::from_secs(header.timestamp());

            if timestamp > invalid_threshold {
                warn!("block {} has incorrect timestamp", header.hash());
                return Err(From::from(BlockError::InvalidTimestamp(
                    OutOfBounds {
                        max: Some(max_time),
                        min: None,
                        found: timestamp,
                    },
                )));
            }

            if timestamp > max_time {
                warn!("block {} has incorrect timestamp", header.hash());
                return Err(From::from(BlockError::TemporarilyInvalid(
                    OutOfBounds {
                        max: Some(max_time),
                        min: None,
                        found: timestamp,
                    },
                )));
            }
        }

        Ok(())
    }

    /// Verify block data against header: transactions root
    fn verify_block_integrity(&self, block: &Block) -> Result<(), Error> {
        let mut tx_rlps: Vec<Vec<u8>> = Vec::new();
        for t in &block.transactions {
            let t_rlp = encode(t);
            tx_rlps.push(t_rlp);
        }

        let expected_root =
            ordered_trie_root(tx_rlps.iter().map(|r| r.as_slice()));
        if &expected_root != block.block_header.transactions_root() {
            warn!("Invalid transaction root");
            bail!(BlockError::InvalidTransactionsRoot(Mismatch {
                expected: expected_root,
                found: *block.block_header.transactions_root(),
            }));
        }
        Ok(())
    }

    /// Phase 1 quick block verification. Only does checks that are cheap.
    /// Operates on a single block
    pub fn verify_block_basic(&self, block: &Block) -> Result<(), Error> {
        self.verify_block_integrity(block)?;

        for t in &block.transactions {
            t.transaction.verify_basic()?;
        }

        Ok(())
    }
}

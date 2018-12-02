use error::{BlockError, Error};
use ethereum_types::{H256, U256};
use pow;
use primitives::{Block, BlockHeader, SignedTransaction};
use rlp::{encode, RlpStream};
use std::{
    collections::HashSet,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use triehash::ordered_trie_root;
use unexpected::{Mismatch, OutOfBounds};

/// Check basic header parameters.
pub fn verify_header_params(header: &BlockHeader) -> Result<(), Error> {
    // verify POW
    let difficulty = pow::boundary_to_difficulty(&pow::compute(
        header.nonce(),
        &header.problem_hash(),
    ));
    if &difficulty < header.difficulty() {
        return Err(From::from(BlockError::InvalidProofOfWork(OutOfBounds {
            min: Some(header.difficulty().clone()),
            max: None,
            found: difficulty,
        })));
    }

    // verify non-duplicated parent and referee hashes
    let mut direct_ancestor_hashes = HashSet::new();
    let parent_hash = header.parent_hash();
    direct_ancestor_hashes.insert(parent_hash.clone());
    for referee_hash in header.referee_hashes() {
        if direct_ancestor_hashes.contains(referee_hash) {
            return Err(From::from(BlockError::DuplicateParentOrRefereeHashes(
                referee_hash.clone(),
            )));
        }
        direct_ancestor_hashes.insert(referee_hash.clone());
    }

    // verify timestamp drift
    const ACCEPTABLE_DRIFT: Duration = Duration::from_secs(15);
    let max_time = SystemTime::now() + ACCEPTABLE_DRIFT;
    let invalid_threshold = max_time + ACCEPTABLE_DRIFT * 9;
    let timestamp = UNIX_EPOCH + Duration::from_secs(header.timestamp());

    if timestamp > invalid_threshold {
        return Err(From::from(BlockError::InvalidTimestamp(OutOfBounds {
            max: Some(max_time),
            min: None,
            found: timestamp,
        })));
    }

    if timestamp > max_time {
        return Err(From::from(BlockError::TemporarilyInvalid(OutOfBounds {
            max: Some(max_time),
            min: None,
            found: timestamp,
        })));
    }

    Ok(())
}

/// Verify block data against header: transactions root
fn verify_block_integrity(block: &Block) -> Result<(), Error> {
    let mut tx_rlps: Vec<Vec<u8>> = Vec::new();
    for t in &block.transactions {
        let t_rlp = encode(t);
        tx_rlps.push(t_rlp);
    }

    let expected_root = ordered_trie_root(tx_rlps.iter().map(|r| r.as_slice()));
    if &expected_root != block.block_header.transactions_root() {
        bail!(BlockError::InvalidTransactionsRoot(Mismatch {
            expected: expected_root,
            found: *block.block_header.transactions_root(),
        }));
    }
    Ok(())
}

/// Phase 1 quick block verification. Only does checks that are cheap. Operates
/// on a single block
pub fn verify_block_basic(block: &Block) -> Result<(), Error> {
    verify_header_params(&block.block_header)?;
    verify_block_integrity(block)?;

    for t in &block.transactions {
        t.transaction.verify_basic()?;
    }

    Ok(())
}

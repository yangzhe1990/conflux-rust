use ethereum_types::{H256, U256, U512};
use hash::keccak;
use rlp::RlpStream;

pub const DIFFICULTY_ADJUSTMENT_EPOCH_PERIOD: u64 = 200;
pub const TARGET_AVERAGE_BLOCK_GENERATION_PERIOD: u64 = 5;
pub const INITIAL_DIFFICULTY: u64 = 200000;

#[derive(Debug, Copy, Clone, PartialOrd, PartialEq)]
pub struct ProofOfWorkProblem {
    pub block_hash: H256,
    pub difficulty: U256,
    pub boundary: H256,
}

#[derive(Debug, Copy, Clone)]
pub struct ProofOfWorkSolution {
    pub nonce: u64,
}

/// Convert boundary to its original difficulty. Basically just `f(x) = 2^256 /
/// x`.
pub fn boundary_to_difficulty(boundary: &H256) -> U256 {
    difficulty_to_boundary_aux(&**boundary)
}

/// Convert difficulty to the target boundary. Basically just `f(x) = 2^256 /
/// x`.
pub fn difficulty_to_boundary(difficulty: &U256) -> H256 {
    difficulty_to_boundary_aux(difficulty).into()
}

pub fn difficulty_to_boundary_aux<T: Into<U512>>(difficulty: T) -> U256 {
    let difficulty = difficulty.into();
    assert!(!difficulty.is_zero());
    if difficulty == U512::one() {
        U256::max_value()
    } else {
        // difficulty > 1, so result should never overflow 256 bits
        U256::from((U512::one() << 256) / difficulty)
    }
}

pub fn compute(nonce: u64, block_hash: &H256) -> H256 {
    let mut rlp = RlpStream::new_list(2);
    rlp.append(block_hash).append(&nonce);
    keccak(rlp.out())
}

pub fn validate(
    problem: &ProofOfWorkProblem, solution: &ProofOfWorkSolution,
) -> bool {
    let nonce = solution.nonce;
    let hash = compute(nonce, &problem.block_hash);
    hash < problem.boundary
}

pub fn target_difficulty(
    block_count: u64, timespan: u64, cur_difficulty: &U256,
) -> U256 {
    if timespan == 0 || block_count == 0 {
        return INITIAL_DIFFICULTY.into();
    }

    let target = (U512::from(*cur_difficulty)
        * U512::from(TARGET_AVERAGE_BLOCK_GENERATION_PERIOD)
        * U512::from(block_count))
        / U512::from(timespan);
    if target.is_zero() {
        return 1.into();
    }
    if target > U256::max_value().into() {
        return U256::max_value();
    }
    U256::from(target)
}

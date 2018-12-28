use crate::{bytes::Bytes, statedb::Error as DbError, vm};
use ethereum_types::{Address, U256, U512};
use primitives::LogEntry;

#[derive(Debug, PartialEq, Clone)]
pub struct Executed {
    /// True if the outer call/create resulted in an exceptional exit.
    pub exception: Option<vm::Error>,

    /// Gas paid up front for execution of transaction.
    pub gas: U256,

    /// Gas used during execution of transaction.
    pub gas_used: U256,

    /// Gas refunded after the execution of transaction.
    /// To get gas that was required up front, add `refunded` and `gas_used`.
    pub refunded: U256,

    /// Fee that need to be paid by execution of this transaction.
    pub fee: U256,

    /// Cumulative gas used in current block so far.
    ///
    /// `cumulative_gas_used = gas_used(t0) + gas_used(t1) + ... gas_used(tn)`
    ///
    /// where `tn` is current transaction.
    pub cumulative_gas_used: U256,

    /// Vector of logs generated by transaction.
    pub logs: Vec<LogEntry>,

    /// Addresses of contracts created during execution of transaction.
    /// Ordered from earliest creation.
    ///
    /// eg. sender creates contract A and A in constructor creates contract B
    ///
    /// B creation ends first, and it will be the first element of the vector.
    pub contracts_created: Vec<Address>,
    /// Transaction output.
    pub output: Bytes,
}

/// Result of executing the transaction.
#[derive(PartialEq, Debug, Clone)]
#[allow(dead_code)]
pub enum ExecutionError {
    /// Returned when there gas paid for transaction execution is
    /// lower than base gas required.
    NotEnoughBaseGas {
        /// Absolute minimum gas required.
        required: U256,
        /// Gas provided.
        got: U256,
    },
    /// Returned when block (gas_used + gas) > gas_limit.
    ///
    /// If gas =< gas_limit, upstream may try to execute the transaction
    /// in next block.
    BlockGasLimitReached {
        /// Gas limit of block for transaction.
        gas_limit: U256,
        /// Gas used in block prior to transaction.
        gas_used: U256,
        /// Amount of gas in block.
        gas: U256,
    },
    /// Returned when transaction nonce does not match state nonce.
    InvalidNonce {
        /// Nonce expected.
        expected: U256,
        /// Nonce found.
        got: U256,
    },
    /// Returned when cost of transaction (value + gas_price * gas) exceeds
    /// current sender balance.
    NotEnoughCash {
        /// Minimum required balance.
        required: U512,
        /// Actual balance.
        got: U512,
    },
    /// When execution tries to modify the state in static context
    MutableCallInStaticContext,
    /// Returned when transacting from a non-existing account with dust
    /// protection enabled.
    SenderMustExist,
    /// Returned when internal evm error occurs.
    Internal(String),
    /// Returned when generic transaction occurs
    TransactionMalformed(String),
}

impl From<DbError> for ExecutionError {
    fn from(err: DbError) -> Self {
        ExecutionError::Internal(format!("{:?}", err))
    }
}

pub type ExecutionResult<T> = Result<T, ExecutionError>;

use super::super::types::{
    Account, Block, EpochNumber, Transaction, H160, H256, U256, U64,
};
use jsonrpc_core::Result;
use jsonrpc_macros::{build_rpc_trait, Trailing};

build_rpc_trait! {
    /// Cfx rpc interface.
    pub trait Cfx {
//        /// Returns protocol version encoded as a string (quotes are necessary).
//        #[rpc(name = "cfx_protocolVersion")]
//        fn protocol_version(&self) -> Result<String>;
//
        /// Returns the number of hashes per second that the node is mining with.
//        #[rpc(name = "cfx_hashrate")]
//        fn hashrate(&self) -> Result<U256>;

//        /// Returns block author.
//        #[rpc(name = "cfx_coinbase")]
//        fn author(&self) -> Result<H160>;

//        /// Returns true if client is actively mining new blocks.
//        #[rpc(name = "cfx_mining")]
//        fn is_mining(&self) -> Result<bool>;

        /// Returns current gas price.
        #[rpc(name = "cfx_gasPrice")]
        fn gas_price(&self) -> Result<U256>;

//        /// Returns accounts list.
//        #[rpc(name = "cfx_accounts")]
//        fn accounts(&self) -> Result<Vec<RpcH160>>;

        /// Returns highest epoch number.
        #[rpc(name = "cfx_epochNumber")]
        fn epoch_number(&self) -> Result<U256>;

        /// Returns balance of the given account.
        #[rpc(name = "cfx_getBalance")]
        fn balance(&self, H160, Trailing<U64>) -> Result<U256>;

//        /// Returns content of the storage at given address.
//        #[rpc(name = "cfx_getStorageAt")]
//        fn storage_at(&self, H160, U256, Trailing<BlockNumber>) -> BoxFuture<H256>;

        /// Returns block with given hash.
        #[rpc(name = "cfx_getBlockByHash")]
        fn block_by_hash(&self, H256, bool) -> Result<Option<Block>>;

        /// Returns best block hash.
        #[rpc(name = "cfx_getBestBlockHash")]
        fn best_block_hash(&self) -> Result<H256>;

//        /// Returns pivot block with given number.
//        #[rpc(name = "cfx_getPivotBlockByNumber")]
//        fn pivot_block_by_number(&self, EpochNumber, bool) -> BoxFuture<Option<Block>>;

        /// Returns the number of transactions sent from given address at given time (epoch number).
        #[rpc(name = "cfx_getTransactionCount")]
        fn transaction_count(&self, H160, Trailing<EpochNumber>) -> Result<U256>;

//        /// Returns the number of transactions in a block with given hash.
//        #[rpc(name = "cfx_getBlockTransactionCountByHash")]
//        fn block_transaction_count_by_hash(&self, H256) -> BoxFuture<Option<U256>>;

//        /// Returns the number of transactions in a block with given block number.
//        #[rpc(name = "cfx_getBlockTransactionCountByNumber")]
//        fn block_trasaction_count_by_number(&self, BlockNumber) -> BoxFuture<Option<U256>>;

//        /// Returns the number of uncles in a block with given hash.
//        #[rpc(name = "cfx_getUncleCountByBlockHash")]
//        fn block_uncles_count_by_hash(&self, H256) -> BoxFuture<Option<U256>>;

//        /// Returns the number of uncles in a block with given block number.
//        #[rpc(name = "cfx_getUnclesCountByBlockNumber")]
//        fn block_uncles_count_by_number(&self, BlockNumber) -> BoxFuture<Option<U256>>;

//        /// Returns the code at given address at given time (block number).
//        #[rpc(name = "cfx_getCode")]
//        fn code_at(&self, H160, Trailing<BlockNumber>) -> BoxFuture<Bytes>;

//        /// Sends signed transaction, returning its hash.
//        #[rpc(name = "cfx_sendRawTransaction")]
//        fn send_raw_transaction(&self, Bytes) -> Result<H256>;

//        /// @alias of `cfx_sendRawTransaction`.
//        #[rpc(name = "cfx_submitTransaction")]
//        fn submit_transaction(&self, Bytes) -> Result<H256>;

//        /// Call contract, returning hte output data.
//        #[rpc(name = "cfx_call")]
//        fn call(&self, CallRequest, Trailing<BlockNumber>) -> BoxFuture<Bytes>;

//        /// Estimate gas needed for execution of given contract.
//        #[rpc(name = "cfx_estimateGas")]
//        fn estimate_gas(&self, CallRequest, Trailing<BlockNumber>) -> BoxFuture<U256>;

        /// Get transaction by its hash.
        #[rpc(name = "cfx_getTransactionByHash")]
        fn transaction_by_hash(&self, H256) -> Result<Option<Transaction>>;

        /// Returns the JSON of whole chain
        #[rpc(name = "cfx_getChain")]
        fn chain(&self) -> Result<Vec<Block>>;

        #[rpc(name = "cfx_getBlocksByEpoch")]
        fn blocks_by_epoch(&self, U64) -> Result<Vec<H256>>;

        #[rpc(name = "cfx_getAccount")]
        fn account(&self, H160, bool, U64) -> Result<Account>;

//        /// Returns transaction at given block hash and index.
//        #[rpc(name = "cfx_getTransactionByBlockHashAndIndex")]
//        fn transaction_by_block_hash_and_index(&self, H256, Index) -> BoxFuture<Option<Transaction>>;

//        /// Returns transaction by given block number and index.
//        #[rpc(name = "cfx_getTransactionByBlockNumberAndIndex")]
//        fn transaction_by_block_number_and_index(&self, BlockNumber, Index) -> BoxFuture<Option<Transaction>>;

//        /// Returns uncles at given block and index.
//        #[rpc(name = "cfx_getUnclesByBlockHashAndIndex")]
//        fn uncles_by_block_hash_and_index(&self, H256, Index) -> BoxFuture<Option<Block>>;

//        /// Returns uncles at given block and index.
//        #[rpc(name = "cfx_getUnclesByBlockNumberAndIndex")]
//        fn uncles_by_block_number_and_index(&self, BlockNumber, Index) -> BoxFuture<Option<Block>>;
    }
}

use crate::rpc::{
    traits::cfx::Cfx,
    types::{
        Account, Block as RpcBlock, BlockTransactions,
        Transaction as RpcTransaction, H160 as RpcH160, H256 as RpcH256,
        U256 as RpcU256, U64 as RpcU64,
    },
};
use core::{
    storage::StorageManager, ConsensusGraph, SharedSynchronizationService,
};
use ethereum_types::{H160, H256};
use jsonrpc_core::{Error as RpcError, Result};
use jsonrpc_macros::Trailing;
use std::sync::Arc;

pub struct CfxHandler {
    pub consensus_graph: Arc<ConsensusGraph>,
    #[allow(dead_code)]
    storage_manager: Arc<StorageManager>,
    sync: SharedSynchronizationService,
}

impl CfxHandler {
    pub fn new(
        consensus_graph: Arc<ConsensusGraph>,
        storage_manager: Arc<StorageManager>,
        sync: SharedSynchronizationService,
    ) -> Self
    {
        CfxHandler {
            consensus_graph,
            storage_manager,
            sync,
        }
    }
}

impl Cfx for CfxHandler {
    fn hashrate(&self) -> Result<RpcU256> { Ok(0.into()) }

    fn best_block_hash(&self) -> Result<RpcH256> {
        info!("RPC Request: cfx_getBestBlockHash()");
        Ok(self.consensus_graph.best_block_hash().into())
    }

    fn gas_price(&self) -> Result<RpcU256> { Ok(RpcU256::default()) }

    fn epoch_number(&self) -> Result<RpcU256> {
        info!("RPC Request: cfx_epochNumber()");
        let best_hash = self.consensus_graph.best_block_hash();
        if let Some(epoch_number) =
            self.consensus_graph.get_block_epoch_number(&best_hash)
        {
            Ok(epoch_number.into())
        } else {
            Err(RpcError::internal_error())
        }
    }

    fn block_by_hash(
        &self, hash: RpcH256, include_txs: bool,
    ) -> Result<Option<RpcBlock>> {
        let hash: H256 = hash.into();
        info!("RPC Request: cfx_getBlockByHash({:?})", hash);

        if let Some(block) = self.sync.block_by_hash(&hash) {
            let result_block = Some(RpcBlock::new(
                &*block,
                self.consensus_graph.clone(),
                include_txs,
            ));
            info!("Block is {:?}", result_block);
            Ok(result_block)
        } else {
            Ok(None)
        }
    }

    fn chain(&self) -> Result<Vec<RpcBlock>> {
        info!("RPC Request: cfx_getChain");
        Ok(self
            .consensus_graph
            .all_blocks_with_topo_order()
            .into_iter()
            .map(|x| RpcBlock::new(&x, self.consensus_graph.clone(), true))
            .collect())
    }

    fn transaction_by_hash(
        &self, hash: RpcH256,
    ) -> Result<Option<RpcTransaction>> {
        let hash: H256 = hash.into();
        info!("RPC Request: cfx_getTransactionByHash({:?})", hash);

        if let Some(transaction) =
            self.consensus_graph.transaction_by_hash(&hash)
        {
            let tx_address = self
                .consensus_graph
                .transaction_address_by_hash(&transaction.hash);
            Ok(Some(RpcTransaction::from_signed(&transaction, tx_address)))
        } else {
            Ok(None)
        }
    }

    fn blocks_by_epoch(&self, epoch_number: RpcU64) -> Result<Vec<RpcH256>> {
        let epoch_number: usize = epoch_number.as_usize();
        info!("RPC Request: cfx_getBlocks epoch_number={:?}", epoch_number);

        Ok(self
            .consensus_graph
            .block_hashes_by_epoch(epoch_number)
            .into_iter()
            .map(|x| x.into())
            .collect())
    }

    fn balance(
        &self, address: RpcH160, epoch_num: Trailing<RpcU64>,
    ) -> Result<RpcU256> {
        let address: H160 = address.into();
        let epoch_num: usize = epoch_num
            .unwrap_or(RpcU64::from(std::usize::MAX))
            .as_usize();

        info!(
            "RPC Request: cfx_getBalance address={:?} epoch_num={:?}",
            address, epoch_num
        );

        self.consensus_graph
            .get_balance(address, epoch_num)
            .map(|x| x.into())
            .map_err(|err| RpcError::invalid_params(err))
    }

    fn account(
        &self, address: RpcH160, include_txs: bool, num_txs: RpcU64,
    ) -> Result<Account> {
        let address: H160 = address.into();
        let num_txs = num_txs.as_usize();
        info!(
            "RPC Request: cfx_getAccount address={:?} include_txs={:?} num_txs={:?}",
            address, include_txs, num_txs
        );
        let balance = self
            .consensus_graph
            .get_balance(address, std::usize::MAX)
            .map_err(|err| RpcError::invalid_params(err))?;
        let transactions = self
            .consensus_graph
            .get_related_transactions(address, num_txs);

        Ok(Account {
            balance: balance.into(),
            transactions: BlockTransactions::new(
                &transactions,
                include_txs,
                self.consensus_graph.clone(),
            ),
        })
    }
}

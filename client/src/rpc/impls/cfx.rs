use crate::rpc::{
    traits::cfx::Cfx,
    types::{
        Account, Block as RpcBlock, BlockTransactions, Bytes, EpochNumber,
        Transaction as RpcTransaction, H160 as RpcH160, H256 as RpcH256,
        U256 as RpcU256, U64 as RpcU64,
    },
};
use cfxcore::{
    storage::StorageManager, ConsensusGraph, SharedSynchronizationService,
    TransactionPool,
};
use ethereum_types::{H160, H256};
use jsonrpc_core::{Error as RpcError, Result};
use jsonrpc_macros::Trailing;
use primitives::EpochNumber as PrimitiveEpochNumber;
use rlp::Rlp;
use std::sync::Arc;

pub struct CfxHandler {
    pub consensus_graph: Arc<ConsensusGraph>,
    #[allow(dead_code)]
    storage_manager: Arc<StorageManager>,
    #[allow(dead_code)]
    sync: SharedSynchronizationService,
    txpool: Arc<TransactionPool>,
}

impl CfxHandler {
    pub fn new(
        consensus_graph: Arc<ConsensusGraph>,
        storage_manager: Arc<StorageManager>,
        sync: SharedSynchronizationService, txpool: Arc<TransactionPool>,
    ) -> Self
    {
        CfxHandler {
            consensus_graph,
            storage_manager,
            sync,
            txpool,
        }
    }

    fn get_primitive_epoch_number(
        &self, number: EpochNumber,
    ) -> PrimitiveEpochNumber {
        match number {
            EpochNumber::Earliest => PrimitiveEpochNumber::Earliest,
            EpochNumber::LatestMined => PrimitiveEpochNumber::LatestMined,
            EpochNumber::LatestState => PrimitiveEpochNumber::LatestState,
            EpochNumber::Num(num) => PrimitiveEpochNumber::Number(num.into()),
        }
    }
}

impl Cfx for CfxHandler {
    //    fn hashrate(&self) -> Result<RpcU256> { Ok(0.into()) }

    fn best_block_hash(&self) -> Result<RpcH256> {
        info!("RPC Request: cfx_getBestBlockHash()");
        Ok(self.consensus_graph.best_block_hash().into())
    }

    fn gas_price(&self) -> Result<RpcU256> {
        info!("RPC Request: cfx_gasPrice()");
        Ok(self.consensus_graph.gas_price().unwrap_or(0.into()).into())
    }

    fn epoch_number(
        &self, epoch_num: Trailing<EpochNumber>,
    ) -> Result<RpcU256> {
        let epoch_num = epoch_num.unwrap_or(EpochNumber::LatestMined);
        info!("RPC Request: cfx_epochNumber({:?})", epoch_num);
        match self.consensus_graph.get_height_from_epoch_number(
            self.get_primitive_epoch_number(epoch_num),
        ) {
            Ok(height) => Ok(height.into()),
            Err(e) => Err(RpcError::invalid_params(e)),
        }
    }

    fn block_by_epoch_number(
        &self, epoch_num: EpochNumber, include_txs: bool,
    ) -> Result<RpcBlock> {
        info!("RPC Request: cfx_getBlockByEpochNumber epoch_number={:?} include_txs={:?}", epoch_num, include_txs);
        self.consensus_graph
            .get_hash_from_epoch_number(
                self.get_primitive_epoch_number(epoch_num),
            )
            .map_err(|err| RpcError::invalid_params(err))
            .and_then(|hash| {
                let block = self.consensus_graph.block_by_hash(&hash).unwrap();
                Ok(RpcBlock::new(
                    &*block,
                    self.consensus_graph.clone(),
                    include_txs,
                ))
            })
    }

    fn block_by_hash(
        &self, hash: RpcH256, include_txs: bool,
    ) -> Result<Option<RpcBlock>> {
        let hash: H256 = hash.into();
        info!(
            "RPC Request: cfx_getBlockByHash hash={:?} include_txs={:?}",
            hash, include_txs
        );

        if let Some(block) = self.consensus_graph.block_by_hash(&hash) {
            let result_block = Some(RpcBlock::new(
                &*block,
                self.consensus_graph.clone(),
                include_txs,
            ));
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

    fn blocks_by_epoch(&self, num: EpochNumber) -> Result<Vec<RpcH256>> {
        info!("RPC Request: cfx_getBlocks epoch_number={:?}", num);

        self.consensus_graph
            .block_hashes_by_epoch(self.get_primitive_epoch_number(num))
            .map_err(|err| RpcError::invalid_params(err))
            .and_then(|vec| Ok(vec.into_iter().map(|x| x.into()).collect()))
    }

    fn balance(
        &self, address: RpcH160, num: Trailing<EpochNumber>,
    ) -> Result<RpcU256> {
        let num = num.unwrap_or(EpochNumber::LatestState);
        let address: H160 = address.into();
        info!(
            "RPC Request: cfx_getBalance address={:?} epoch_num={:?}",
            address, num
        );

        self.consensus_graph
            .get_balance(address, self.get_primitive_epoch_number(num))
            .map(|x| x.into())
            .map_err(|err| RpcError::invalid_params(err))
    }

    fn account(
        &self, address: RpcH160, include_txs: bool, num_txs: RpcU64,
        epoch_num: Trailing<EpochNumber>,
    ) -> Result<Account>
    {
        let address: H160 = address.into();
        let num_txs = num_txs.as_usize();
        let epoch_num = epoch_num.unwrap_or(EpochNumber::LatestState);
        info!(
            "RPC Request: cfx_getAccount address={:?} include_txs={:?} num_txs={:?} epoch_num={:?}",
            address, include_txs, num_txs, epoch_num
        );
        self.consensus_graph
            .get_account(
                address,
                num_txs,
                self.get_primitive_epoch_number(epoch_num),
            )
            .and_then(|(balance, transactions)| {
                Ok(Account {
                    balance: balance.into(),
                    transactions: BlockTransactions::new(
                        &transactions,
                        include_txs,
                        self.consensus_graph.clone(),
                    ),
                })
            })
            .map_err(|err| RpcError::invalid_params(err))
    }

    fn transaction_count(
        &self, address: RpcH160, num: Trailing<EpochNumber>,
    ) -> Result<RpcU256> {
        let num = num.unwrap_or(EpochNumber::LatestState);
        info!(
            "RPC Request: cfx_getTransactionCount address={:?} epoch_num={:?}",
            address, num
        );

        self.consensus_graph
            .transaction_count(
                address.into(),
                self.get_primitive_epoch_number(num),
            )
            .map_err(|err| RpcError::invalid_params(err))
            .map(|x| x.into())
    }

    fn send_raw_transaction(&self, raw: Bytes) -> Result<RpcH256> {
        info!("RPC Request: cfx_sendRawTransaction bytes={:?}", raw);
        Rlp::new(&raw.into_vec())
            .as_val()
            .map_err(|err| {
                RpcError::invalid_params(format!("Error: {:?}", err))
            })
            .and_then(|tx| {
                let result = self.txpool.insert_new_transactions(
                    self.consensus_graph.best_state_block_hash(),
                    vec![tx],
                );
                if result.is_empty() {
                    Ok(H256::new().into())
                } else {
                    Ok(result[0].into())
                }
            })
    }
}

use crate::rpc::types::{Transaction, H160, H256, U256};
use ethereum_types::U256 as Eth256;
use primitives::Block as PrimitiveBlock;
use serde::{Serialize, Serializer};
use serde_derive::Serialize;

#[derive(Debug)]
pub enum BlockTransactions {
    /// Only hashes
    Hashes(Vec<H256>),
    /// Full transactions
    Full(Vec<Transaction>),
}

impl Serialize for BlockTransactions {
    fn serialize<S: Serializer>(
        &self, serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match *self {
            BlockTransactions::Hashes(ref hashes) => {
                hashes.serialize(serializer)
            }
            BlockTransactions::Full(ref txs) => txs.serialize(serializer),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    /// Hash of the block
    pub hash: H256,
    /// Hash of the parent
    pub parent_hash: H256,
    /// Distance to genesis
    pub height: U256,
    /// Author's address
    pub author: H160,
    /// State root hash
    pub deferred_state_root: H256,
    /// Transactions root hash
    pub transactions_root: H256,
    /// Epoch number
    pub number: Option<usize>,
    /// Gas used
    pub gas_used: U256,
    /// Gas limit
    pub gas_limit: U256,
    /// Timestamp
    pub timestamp: U256,
    /// Difficulty
    pub difficulty: U256,
    /// Total difficulty
    pub total_difficulty: Option<U256>,
    /// Referee hashes
    pub referee_hashes: Vec<H256>,
    /// Nonce of the block
    pub nonce: u64,
    /// Transactions
    pub transactions: BlockTransactions,
    /// Size in bytes
    pub size: Option<U256>,
}

impl Block {
    pub fn new(
        b: &PrimitiveBlock, epoch_number: Option<usize>,
        tot_diff: Option<Eth256>,
    ) -> Self
    {
        Block {
            hash: H256::from(b.block_header.hash().clone()),
            parent_hash: H256::from(b.block_header.parent_hash().clone()),
            height: U256::from(b.block_header.height()),
            author: H160::from(b.block_header.author().clone()),
            deferred_state_root: H256::from(
                b.block_header.deferred_state_root().clone(),
            ),
            transactions_root: H256::from(
                b.block_header.transactions_root().clone(),
            ),
            // PrimitiveBlock does not contain this information
            number: epoch_number,
            gas_used: U256::from(b.total_gas()),
            // FIXME: Change the field after we figured out the gas limit and
            // fee system
            gas_limit: U256::from(100000),
            timestamp: U256::from(b.block_header.timestamp()),
            difficulty: U256::from(b.block_header.difficulty().clone()),
            // PrimitiveBlock does not contain this information
            total_difficulty: tot_diff.map(|x| U256::from(x)),
            referee_hashes: b
                .block_header
                .referee_hashes()
                .iter()
                .map(|x| H256::from(*x))
                .collect(),
            nonce: b.block_header.nonce(),
            transactions: BlockTransactions::Hashes(
                b.transactions
                    .iter()
                    .map(|x| H256::from(x.hash()))
                    .collect(),
            ),
            size: Some(U256::from(b.size())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Block, BlockTransactions};
    use crate::types::{Transaction, H160, H2048, H256, H64, U256};
    use serde_json;
    use std::collections::BTreeMap;

    #[test]
    fn test_serialize_block_transactions() {
        let t = BlockTransactions::Full(vec![Transaction::default()]);
        let serialized = serde_json::to_string(&t).unwrap();
        assert_eq!(serialized, r#"[{"hash":"0x0000000000000000000000000000000000000000000000000000000000000000","nonce":"0x0","blockHash":null,"blockNumber":null,"transactionIndex":null,"from":"0x0000000000000000000000000000000000000000","to":null,"value":"0x0","gasPrice":"0x0","gas":"0x0"}]"#);

        let t = BlockTransactions::Hashes(vec![H256::default().into()]);
        let serialized = serde_json::to_string(&t).unwrap();
        assert_eq!(serialized, r#"["0x0000000000000000000000000000000000000000000000000000000000000000"]"#);
    }

    #[test]
    fn test_serialize_block() {
        let block = Block {
            hash: H256::default(),
            parent_hash: H256::default(),
            height: U256::default(),
            author: H160::default(),
            deferred_state_root: H256::default(),
            transactions_root: H256::default(),
            number: Some(0),
            gas_used: U256::default(),
            gas_limit: U256::default(),
            timestamp: U256::default(),
            difficulty: U256::default(),
            total_difficulty: Some(U256::default()),
            referee_hashes: Vec::new(),
            nonce: 0,
            transactions: BlockTransactions::Hashes(vec![].into()),
            size: Some(69.into()),
        };
        let serialized_block = serde_json::to_string(&block).unwrap();

        assert_eq!(serialized_block, r#"{"hash":"0x0000000000000000000000000000000000000000000000000000000000000000","parentHash":"0x0000000000000000000000000000000000000000000000000000000000000000","author":"0x0000000000000000000000000000000000000000","stateRoot":"0x0000000000000000000000000000000000000000000000000000000000000000","transactionsRoot":"0x0000000000000000000000000000000000000000000000000000000000000000","number":"0x0","gasUsed":"0x0","gasLimit":"0x0","timestamp":"0x0","difficulty":"0x0","totalDifficulty":"0x0","transactions":[],"size":"0x45"}"#);
    }
}

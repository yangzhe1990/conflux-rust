use crate::{
    transaction::TxShortId, BlockHeader, SignedTransaction,
    TransactionWithSignature,
};
use byteorder::{ByteOrder, LittleEndian};
use ethereum_types::{H256, U256};
use heapsize::HeapSizeOf;
use keccak_hash::keccak;
use lru::LruCache;
use rand::Rng;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use siphasher::sip::SipHasher24;
use std::{collections::HashMap, hash::Hasher, mem, sync::Arc};

/// A block, encoded as it is on the block chain.
#[derive(Default, Debug, Clone, PartialEq)]
pub struct Block {
    /// The header hash of this block.
    pub block_header: BlockHeader,
    /// The¡ transactions in this block.
    pub transactions: Vec<Arc<SignedTransaction>>,
}

impl HeapSizeOf for Block {
    fn heap_size_of_children(&self) -> usize {
        // Ignores the size of Arc<SignedTransaction>
        self.block_header.heap_size_of_children()
            + self.transactions.len() * mem::size_of::<SignedTransaction>()
    }
}

impl Block {
    pub fn hash(&self) -> H256 { self.block_header.hash() }

    pub fn total_gas(&self) -> U256 {
        let mut sum = U256::from(0);
        for t in &self.transactions {
            sum += t.gas;
        }
        sum
    }

    pub fn size(&self) -> usize {
        let mut ret = self.block_header.size();
        for t in &self.transactions {
            ret += t.size();
        }
        ret
    }

    /// Construct a new compact block with random nonce
    /// This block will be relayed with the new compact block to prevent
    /// adversaries to make tx shortId collision
    pub fn to_compact(&self) -> CompactBlock {
        let nonce: u64 = rand::thread_rng().gen();
        let (k0, k1) = get_shortid_key(&self.block_header, &nonce);
        CompactBlock {
            block_header: self.block_header.clone(),
            nonce,
            tx_short_ids: self
                .transactions
                .iter()
                .map(|tx| from_tx_hash(&tx.hash(), k0, k1))
                .collect(),
            // reconstructed_txes constructed here will not be used
            reconstructed_txes: Vec::new(),
        }
    }
}

impl Encodable for Block {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream.begin_list(2).append(&self.block_header);
        stream.begin_list(self.transactions.len());
        for tx in &self.transactions {
            stream.append(tx.as_ref());
        }
    }
}

impl From<Block> for RawBlock {
    fn from(block: Block) -> RawBlock {
        RawBlock {
            block_header: block.block_header,
            transactions: block
                .transactions
                .into_iter()
                .map(|tx| tx.transaction.clone())
                .collect(),
        }
    }
}

#[derive(Default, Debug, Clone, PartialEq)]
pub struct RawBlock {
    /// The header hash of this block.
    pub block_header: BlockHeader,
    /// The¡ transactions in this block.
    pub transactions: Vec<TransactionWithSignature>,
}

impl RawBlock {
    pub fn into_block_with_signed_tx(
        self, tx_cache: &mut LruCache<H256, Arc<SignedTransaction>>,
    ) -> Result<Block, DecoderError> {
        let signed_transactions: Result<
            Vec<Arc<SignedTransaction>>,
            DecoderError,
        > = SignedTransaction::batch_recover_with_cache(
            &self.transactions,
            tx_cache,
        );
        Ok(Block {
            block_header: self.block_header,
            transactions: signed_transactions?,
        })
    }
}

impl Encodable for RawBlock {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream
            .begin_list(2)
            .append(&self.block_header)
            .append_list(&self.transactions);
    }
}

impl Decodable for RawBlock {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        if rlp.as_raw().len() != rlp.payload_info()?.total() {
            return Err(DecoderError::RlpIsTooBig);
        }
        if rlp.item_count()? != 2 {
            return Err(DecoderError::RlpIncorrectListLen);
        }

        let transactions = rlp.list_at::<TransactionWithSignature>(1)?;

        Ok(RawBlock {
            block_header: rlp.val_at(0)?,
            transactions,
        })
    }
}

// TODO Some optimization may be made if short_id hash collission is detected,
// but should be rare
#[derive(Default, Debug, Clone, PartialEq)]
pub struct CompactBlock {
    /// The block header
    pub block_header: BlockHeader,
    /// The nonce for use in short id calculation
    pub nonce: u64,
    /// A list of tx short ids
    pub tx_short_ids: Vec<TxShortId>,

    /// Store the txes reconstructed, None means not received
    pub reconstructed_txes: Vec<Option<Arc<SignedTransaction>>>,
}

impl CompactBlock {
    /// Find tx in tx_cache that matches tx_short_ids to fill in
    /// reconstruced_txes Return the differentially encoded index of missing
    /// transactions Now should only called once after CompactBlock is
    /// decoded
    pub fn build_partial(
        &mut self, tx_cache: &LruCache<H256, Arc<SignedTransaction>>,
    ) -> Vec<usize> {
        self.reconstructed_txes
            .resize(self.tx_short_ids.len(), None);
        let mut short_id_to_index =
            HashMap::with_capacity(self.tx_short_ids.len());
        for (i, id) in self.tx_short_ids.iter().enumerate() {
            short_id_to_index.insert(id, i);
        }
        let (k0, k1) = get_shortid_key(&self.block_header, &self.nonce);
        for (tx_hash, tx) in tx_cache {
            let short_id = from_tx_hash(tx_hash, k0, k1);
            match short_id_to_index.remove(&short_id) {
                Some(index) => {
                    self.reconstructed_txes[index] = Some(tx.clone());
                }
                None => {}
            }
        }
        let mut missing_index = Vec::new();
        for index in short_id_to_index.values() {
            missing_index.push(*index);
        }
        missing_index.sort();
        let mut last = 0;
        let mut missing_encoded = Vec::new();
        for index in missing_index {
            missing_encoded.push(index - last);
            last = index + 1;
        }
        missing_encoded
    }

    pub fn hash(&self) -> H256 { self.block_header.hash() }
}

fn get_shortid_key(header: &BlockHeader, nonce: &u64) -> (u64, u64) {
    let mut stream = RlpStream::new();
    stream.begin_list(2).append(header).append(nonce);
    let to_hash = stream.out();
    let key_hash: [u8; 32] = keccak(to_hash).into();
    let k0 = LittleEndian::read_u64(&key_hash[0..8]);
    let k1 = LittleEndian::read_u64(&key_hash[8..16]);
    (k0, k1)
}

/// Compute Tx ShortId from hash. The algorithm is from Bitcoin BIP152
fn from_tx_hash(hash: &H256, k0: u64, k1: u64) -> TxShortId {
    let mut hasher = SipHasher24::new_with_keys(k0, k1);
    hasher.write(hash.as_ref());
    hasher.finish() & 0x00ffffff_ffffffff
}

impl Encodable for CompactBlock {
    fn rlp_append(&self, steam: &mut RlpStream) {
        steam
            .begin_list(3)
            .append(&self.block_header)
            .append(&self.nonce)
            .append_list(&self.tx_short_ids);
    }
}

impl Decodable for CompactBlock {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(CompactBlock {
            block_header: rlp.val_at(0)?,
            nonce: rlp.val_at(1)?,
            tx_short_ids: rlp.list_at(2)?,
            reconstructed_txes: Vec::new(),
        })
    }
}

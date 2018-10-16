use ethereum_types::H256;
use parking_lot::RwLock;
use primitives::SignedTransaction;
use std::{
    cmp::Ordering,
    collections::{BinaryHeap, HashMap},
    sync::Arc,
};

#[derive(Debug)]
pub struct TransactionRef {
    transaction: Arc<SignedTransaction>,
}

impl TransactionRef {
    pub fn new(tx: SignedTransaction) -> Self {
        TransactionRef {
            transaction: Arc::new(tx),
        }
    }
}

impl Clone for TransactionRef {
    fn clone(&self) -> Self {
        TransactionRef {
            transaction: self.transaction.clone(),
        }
    }
}

impl Ord for TransactionRef {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.transaction.gas_price < other.transaction.gas_price {
            return Ordering::Less;
        } else if self.transaction.gas_price > other.transaction.gas_price {
            return Ordering::Greater;
        } else {
            return Ordering::Equal;
        }
    }
}

impl PartialOrd for TransactionRef {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for TransactionRef {
    fn eq(&self, other: &Self) -> bool {
        self.transaction.gas_price == other.transaction.gas_price
    }
}

impl Eq for TransactionRef {}

pub struct PoolInner {
    tx_hash_map: HashMap<H256, TransactionRef>,
    tx_priority_queue: BinaryHeap<TransactionRef>,
}

impl PoolInner {
    pub fn new() -> Self {
        PoolInner {
            tx_hash_map: HashMap::new(),
            tx_priority_queue: BinaryHeap::new(),
        }
    }
}

pub struct TransactionPool {
    capacity: usize,
    pool: RwLock<PoolInner>,
}

pub type TransactionPoolRef = Arc<TransactionPool>;

impl TransactionPool {
    pub fn new(cap: usize) -> Self {
        TransactionPool {
            capacity: cap,
            pool: RwLock::new(PoolInner::new()),
        }
    }

    pub fn new_ref(cap: usize) -> TransactionPoolRef {
        Arc::new(Self::new(cap))
    }

    pub fn import(&self, tx: SignedTransaction) -> bool {
        let mut pool_write = self.pool.write();

        if self.capacity <= pool_write.tx_hash_map.len() {
            // pool is full
            return false;
        }

        let tx_hash = tx.transaction.hash();

        if pool_write.tx_hash_map.contains_key(&tx_hash) {
            // already exists
            return false;
        }

        let tx_ref = TransactionRef::new(tx);
        pool_write.tx_hash_map.insert(tx_hash, tx_ref.clone());
        pool_write.tx_priority_queue.push(tx_ref);

        true
    }

    pub fn fetch_transaction(&self) -> Option<SignedTransaction> {
        let mut pool_write = self.pool.write();

        if pool_write.tx_priority_queue.is_empty() {
            return None;
        }

        let tx_ref = pool_write.tx_priority_queue.pop().unwrap();
        pool_write
            .tx_hash_map
            .remove(&tx_ref.transaction.transaction.hash());
        Some((*tx_ref.transaction).clone())
    }
}

use ethereum_types::H256;
use parking_lot::RwLock;
use primitives::SignedTransaction;
use std::{
    cmp::Ordering,
    collections::{BinaryHeap, HashSet},
    sync::Arc,
};

pub struct OrderedTransaction {
    transaction: SignedTransaction,
}

impl OrderedTransaction {
    pub fn new(transaction: SignedTransaction) -> Self {
        OrderedTransaction { transaction }
    }
}

impl Ord for OrderedTransaction {
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

impl PartialOrd for OrderedTransaction {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for OrderedTransaction {
    fn eq(&self, other: &Self) -> bool {
        self.transaction.gas_price == other.transaction.gas_price
    }
}

impl Eq for OrderedTransaction {}

pub struct TransactionPoolInner {
    hashes: HashSet<H256>,
    ordered_transactions: BinaryHeap<OrderedTransaction>,
}

impl TransactionPoolInner {
    pub fn new() -> Self {
        TransactionPoolInner {
            hashes: HashSet::new(),
            ordered_transactions: BinaryHeap::new(),
        }
    }
}

pub struct TransactionPool {
    capacity: usize,
    inner: RwLock<TransactionPoolInner>,
}

pub type SharedTransactionPool = Arc<TransactionPool>;

impl TransactionPool {
    pub fn with_capacity(capacity: usize) -> Self {
        TransactionPool {
            capacity,
            inner: RwLock::new(TransactionPoolInner::new()),
        }
    }

    pub fn add(&self, transaction: SignedTransaction) -> bool {
        let mut inner = self.inner.write();

        if self.capacity <= inner.hashes.len() {
            // pool is full
            return false;
        }

        let hash = transaction.transaction.hash();
        if inner.hashes.contains(&hash) {
            // already exists
            return false;
        }

        inner.hashes.insert(hash);
        inner
            .ordered_transactions
            .push(OrderedTransaction::new(transaction));

        true
    }

    pub fn fetch(&self) -> Option<SignedTransaction> {
        let mut inner = self.inner.write();

        if inner.ordered_transactions.is_empty() {
            return None;
        }

        let transaction = inner.ordered_transactions.pop().unwrap();
        inner.hashes.remove(&transaction.transaction.hash());
        Some(transaction.transaction)
    }
}

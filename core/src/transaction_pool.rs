extern crate rand;

use ethereum_types::{H256, H512, U256, U512};
use parking_lot::RwLock;
use primitives::SignedTransaction;
use rand::prelude::*;
use std::{
    cmp::{min, Ordering},
    collections::{BTreeSet, BinaryHeap, HashSet},
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
    transaction_set: BTreeSet<OrderedTransaction>,
    transaction_heap: BinaryHeap<OrderedTransaction>,
}

impl TransactionPoolInner {
    pub fn new() -> Self {
        TransactionPoolInner {
            hashes: HashSet::new(),
            transaction_set: BTreeSet::new(),
            transaction_heap: BinaryHeap::new(),
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
            .transaction_set
            .insert(OrderedTransaction::new(transaction.clone()));
        inner
            .transaction_heap
            .push(OrderedTransaction::new(transaction.clone()));

        true
    }

    pub fn fetch(&self) -> Option<SignedTransaction> {
        let mut inner = self.inner.write();

        if inner.transaction_heap.is_empty() {
            return None;
        }

        let transaction = inner.transaction_heap.pop().unwrap();
        inner.hashes.remove(&transaction.transaction.hash());
        inner.transaction_set.remove(&transaction);
        Some(transaction.transaction)
    }

    /// pack at most num_txs transactions randomly
    pub fn pack_transactions(&self, num_txs: usize) -> Vec<SignedTransaction> {
        //TODO: should be done by O(num_txs * log)
        let mut transaction_sequence: Vec<SignedTransaction> = Vec::new();
        let mut packed_transaction: Vec<SignedTransaction> = Vec::new();
        let mut gas_price_sequence: Vec<U512> = Vec::new();
        let mut sum_gas_price: U512 = 0.into();

        let inner = self.inner.read();
        for tx in inner.transaction_set.iter() {
            let transaction = tx.transaction.clone();
            if transaction.gas_price == 0.into() {
                continue;
            }
            sum_gas_price += U512::from(transaction.gas_price);
            gas_price_sequence.push(U512::from(transaction.gas_price));
            transaction_sequence.push(transaction);
        }

        let len = transaction_sequence.len();
        let num_txs = min(num_txs, len);
        for _ in 0..num_txs {
            let mut rand_value: U512 = U512::from(H512::random());
            rand_value = rand_value % sum_gas_price;

            for id in 0..len {
                if gas_price_sequence[id] > rand_value {
                    sum_gas_price -= gas_price_sequence[id];
                    gas_price_sequence[id] = 0.into();
                    packed_transaction.push(transaction_sequence[id].clone());
                    break;
                }
                rand_value -= gas_price_sequence[id];
            }
        }
        packed_transaction
    }
}

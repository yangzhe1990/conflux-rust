extern crate rand;

use ethereum_types::{H256, H512, U512};
use parking_lot::RwLock;
use primitives::{SignedTransaction, TransactionWithSignature};
use std::{
    cmp::{min, Ordering},
    collections::{BTreeSet, HashSet},
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
        self.transaction.hash().cmp(&other.transaction.hash())
    }
}

impl PartialOrd for OrderedTransaction {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for OrderedTransaction {
    fn eq(&self, other: &Self) -> bool {
        self.transaction.hash() == other.transaction.hash()
    }
}

impl Eq for OrderedTransaction {}

pub struct TransactionPoolInner {
    hashes: HashSet<H256>,
    transaction_set: BTreeSet<OrderedTransaction>,
}

impl TransactionPoolInner {
    pub fn new() -> Self {
        TransactionPoolInner {
            hashes: HashSet::new(),
            transaction_set: BTreeSet::new(),
        }
    }

    pub fn len(&self) -> usize { self.hashes.len() }
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

    pub fn len(&self) -> usize { self.inner.read().len() }

    pub fn insert_new_transactions(
        &self, transactions: Vec<TransactionWithSignature>,
    ) -> u32 {
        let mut count: u32 = 0;
        for tx in transactions {
            if let Ok(public) = tx.recover_public() {
                if self.add(SignedTransaction::new(public, tx)) {
                    count += 1;
                }
            } else {
                debug!(
                    "Unable to recover the public key of transaction {:?}",
                    tx.hash()
                );
            }
        }
        count
    }

    pub fn add(&self, transaction: SignedTransaction) -> bool {
        let mut inner = self.inner.write();

        if self.capacity <= inner.hashes.len() {
            debug!("Rejected a transaction {:?} because of insufficient transaction pool capacity!", transaction.hash());
            // pool is full
            return false;
        }

        let hash = transaction.hash();
        if inner.hashes.contains(&hash) {
            debug!(
                "Rejected a transaction {:?} because it already exists!",
                transaction.hash()
            );
            // already exists
            return false;
        }

        inner.hashes.insert(hash);
        inner
            .transaction_set
            .insert(OrderedTransaction::new(transaction.clone()));
        debug!(
            "Inserted a transaction {:?}, now txpool size {:?}",
            transaction.hash(),
            inner.len()
        );
        true
    }

    pub fn remove(&self, transaction: SignedTransaction) -> bool {
        let mut inner = self.inner.write();
        let hash = transaction.hash();
        if !inner.hashes.contains(&hash) {
            return false;
        }
        inner.hashes.remove(&hash);
        inner
            .transaction_set
            .remove(&OrderedTransaction::new(transaction.clone()));
        true
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

    pub fn transactions_to_propagate(&self) -> Vec<SignedTransaction> {
        let inner = self.inner.read();
        inner
            .transaction_set
            .iter()
            .map(|x| x.transaction.clone())
            .collect()
    }
}

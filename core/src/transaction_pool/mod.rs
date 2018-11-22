mod impls;
mod ready;

extern crate rand;

use ethereum_types::{Address, H256, H512, U256, U512};
use parking_lot::{Mutex, RwLock};
use primitives::{SignedTransaction, TransactionWithSignature};
use std::{
    cmp::{min, Ordering},
    collections::HashMap,
    ops::DerefMut,
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
    pending_transations: HashMap<H256, OrderedTransaction>,
}

impl TransactionPoolInner {
    pub fn new() -> Self {
        TransactionPoolInner {
            pending_transations: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize { self.pending_transations.len() }
}

pub struct TransactionPool {
    capacity: usize,
    // Locking order:
    // waiters.lock()
    // inner.write()
    pub waiters: Mutex<HashMap<Address, HashMap<U256, Vec<H256>>>>,
    inner: RwLock<TransactionPoolInner>,
}

pub type SharedTransactionPool = Arc<TransactionPool>;

impl TransactionPool {
    pub fn with_capacity(capacity: usize) -> Self {
        TransactionPool {
            capacity,
            waiters: Mutex::new(HashMap::new()),
            inner: RwLock::new(TransactionPoolInner::new()),
        }
    }

    pub fn len(&self) -> usize { self.inner.read().len() }

    pub fn insert_new_transactions(
        &self, transactions: Vec<TransactionWithSignature>,
    ) {
        for tx in transactions {
            if let Ok(public) = tx.recover_public() {
                let signed_tx = SignedTransaction::new(public, tx);
                // verify transaction
                if !self.verify_transaction(&signed_tx) {
                    warn!("Transaction discarded due to failure of passing verification {:?}", signed_tx.hash());
                    continue;
                }

                self.add_with_readiness(signed_tx);
            } else {
                debug!(
                    "Unable to recover the public key of transaction {:?}",
                    tx.hash()
                );
            }
        }
    }

    pub fn verify_transaction(&self, transaction: &SignedTransaction) -> bool {
        true
    }

    pub fn add_with_readiness(&self, transaction: SignedTransaction) {
        let mut waiters = self.waiters.lock();
        let mut inner = self.inner.write();
        let inner = inner.deref_mut();

        if self.check_readiness(&transaction) {
            self.add_ready_without_lock(inner, transaction);
        } else {
            let sender = transaction.sender();
            let nonce = transaction.nonce();
            let hash = transaction.hash();
            if self.add_pending_without_lock(inner, transaction) {
                // subscribe to waiter queue
                let entry = waiters.entry(sender).or_insert(HashMap::new());
                let entry = entry.entry(nonce).or_insert(Vec::new());
                entry.push(hash);
            }
        }
    }

    pub fn check_readiness(&self, transaction: &SignedTransaction) -> bool {
        false
    }

    pub fn add_ready(&self, transaction: SignedTransaction) -> bool { true }

    pub fn add_ready_without_lock(
        &self, inner: &mut TransactionPoolInner, transaction: SignedTransaction,
    ) -> bool {
        true
    }

    pub fn add_pending(&self, transaction: SignedTransaction) -> bool {
        let mut inner = self.inner.write();
        let inner = inner.deref_mut();
        self.add_pending_without_lock(inner, transaction)
    }

    pub fn add_pending_without_lock(
        &self, inner: &mut TransactionPoolInner, transaction: SignedTransaction,
    ) -> bool {
        if self.capacity <= inner.pending_transations.len() {
            debug!("Rejected a transaction {:?} because of insufficient transaction pool capacity!", transaction.hash());
            // pool is full
            return false;
        }

        let hash = transaction.hash();
        if inner.pending_transations.contains_key(&hash) {
            debug!(
                "Rejected a transaction {:?} because it already exists!",
                transaction.hash()
            );
            // already exists
            return false;
        }

        inner
            .pending_transations
            .insert(hash, OrderedTransaction::new(transaction.clone()));
        debug!(
            "Inserted a transaction {:?}, now txpool size {:?}",
            transaction.hash(),
            inner.len()
        );
        true
    }

    pub fn remove_pending(&self, transaction: SignedTransaction) -> bool {
        let mut inner = self.inner.write();
        let inner = inner.deref_mut();
        self.remove_pending_without_lock(inner, transaction)
    }

    pub fn remove_pending_without_lock(
        &self, inner: &mut TransactionPoolInner, transaction: SignedTransaction,
    ) -> bool {
        let hash = transaction.hash();
        if !inner.pending_transations.contains_key(&hash) {
            return false;
        }
        inner.pending_transations.remove(&hash);
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
        for (hash, tx) in inner.pending_transations.iter() {
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
            .pending_transations
            .iter()
            .map(|x| x.1.transaction.clone())
            .collect()
    }

    pub fn notify_ready(&mut self, address: &Address, nonce: &U256) {
        let mut waiters = self.waiters.lock();
        let mut inner = self.inner.write();
        let inner = inner.deref_mut();

        let clear_waiter = if let Some(m) = waiters.get_mut(address) {
            if let Some(h) = m.get_mut(nonce) {
                // on_notify
            }
            m.remove(nonce);
            m.is_empty()
        } else {
            false
        };
        if clear_waiter {
            waiters.remove(address);
        }
    }
}

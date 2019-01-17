mod impls;
mod ready;

#[cfg(test)]
mod tests;

extern crate rand;

pub use self::impls::TreapMap;
use self::ready::Readiness;
use crate::{
    pow::WORKER_COMPUTATION_PARALLELISM,
    state::State,
    statedb::StateDb,
    storage::{Storage, StorageManager, StorageManagerTrait},
};
use ethereum_types::{Address, H256, H512, U256, U512};
use lru::LruCache;
use parking_lot::{Mutex, RwLock};
use primitives::{
    Account, EpochId, SignedTransaction, TransactionWithSignature,
};
use std::{
    cmp::{min, Ordering},
    collections::hash_map::HashMap,
    ops::DerefMut,
    sync::{mpsc::channel, Arc},
};
use threadpool::ThreadPool;

pub const DEFAULT_MIN_TRANSACTION_GAS_PRICE: u64 = 1;
pub const DEFAULT_MAX_TRANSACTION_GAS_LIMIT: u64 = 100_000_000;
pub const DEFAULT_MAX_BLOCK_GAS_LIMIT: u64 = 30_000 * 100_000;

pub const FURTHEST_FUTURE_TRANSACTION_NONCE_OFFSET: u32 = 2000;

pub struct AccountCache<'storage> {
    pub accounts: HashMap<Address, Account>,
    pub storage: StateDb<'storage>,
}

impl<'storage> AccountCache<'storage> {
    pub fn new(storage: Storage<'storage>) -> Self {
        AccountCache {
            accounts: HashMap::new(),
            storage: StateDb::new(storage),
        }
    }

    pub fn get_ready_account(&mut self, address: &Address) -> Option<&Account> {
        self.accounts.get(address)
    }

    fn is_ready(&mut self, tx: &SignedTransaction) -> Readiness {
        let sender = tx.sender();
        if !self.accounts.contains_key(&sender) {
            let account = self
                .storage
                .get_account(&sender, false)
                .ok()
                .and_then(|x| x);
            if let Some(account) = account {
                self.accounts.insert(sender.clone(), account);
            }
        }
        let account = self.accounts.get_mut(&sender);
        if let Some(account) = account {
            match tx.nonce().cmp(&account.nonce) {
                Ordering::Greater => {
                    if (tx.nonce() - account.nonce)
                        > FURTHEST_FUTURE_TRANSACTION_NONCE_OFFSET.into()
                    {
                        Readiness::TooDistantFuture
                    } else {
                        Readiness::Future
                    }
                }
                Ordering::Less => Readiness::Stale,
                Ordering::Equal => Readiness::Ready,
            }
        } else {
            if tx.nonce() > FURTHEST_FUTURE_TRANSACTION_NONCE_OFFSET.into() {
                Readiness::TooDistantFuture
            } else {
                Readiness::Future
            }
        }
    }
}

pub struct PendingTransactionBucket {
    pub bucket: HashMap<U256, Arc<SignedTransaction>>,
}

impl PendingTransactionBucket {
    pub fn new() -> Self {
        PendingTransactionBucket {
            bucket: HashMap::new(),
        }
    }
}

pub struct PendingTransactionPool {
    pub pool: HashMap<Address, PendingTransactionBucket>,
    pub len: usize,
}

impl PendingTransactionPool {
    pub fn new() -> Self {
        PendingTransactionPool {
            pool: HashMap::new(),
            len: 0,
        }
    }

    pub fn len(&self) -> usize { self.len }

    pub fn insert(&mut self, tx: Arc<SignedTransaction>) -> bool {
        let entry = self
            .pool
            .entry(tx.sender.clone())
            .or_insert(PendingTransactionBucket::new());

        let mut res = false;
        let old_len = entry.bucket.len();
        {
            let tx_in_bucket =
                entry.bucket.entry(tx.nonce.clone()).or_insert(tx.clone());
            if tx_in_bucket.gas_price < tx.gas_price {
                *tx_in_bucket = tx;
                res = true;
            }
        }
        let new_len = entry.bucket.len();

        if new_len > old_len {
            debug_assert!(new_len - old_len == 1);
            self.len = self.len + 1;
            res = true;
        }

        res
    }

    pub fn remove(
        &mut self, address: &Address, nonce: &U256,
    ) -> Option<Arc<SignedTransaction>> {
        let (res, clear_bucket) =
            if let Some(entry) = self.pool.get_mut(address) {
                if let Some(tx) = entry.bucket.remove(nonce) {
                    self.len = self.len - 1;
                    (Some(tx), entry.bucket.is_empty())
                } else {
                    (None, false)
                }
            } else {
                (None, false)
            };
        if clear_bucket {
            self.pool.remove(address);
        }
        res
    }

    pub fn get(
        &self, address: &Address, nonce: &U256,
    ) -> Option<&Arc<SignedTransaction>> {
        self.pool
            .get(address)
            .and_then(|bucket| bucket.bucket.get(nonce))
    }
}

pub struct TransactionPoolInner {
    pending_transactions: PendingTransactionPool,
    ready_transactions: TreapMap<H256, Arc<SignedTransaction>, U512>,
}

impl TransactionPoolInner {
    pub fn new() -> Self {
        TransactionPoolInner {
            pending_transactions: PendingTransactionPool::new(),
            ready_transactions: TreapMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.pending_transactions.len() + self.ready_transactions.len()
    }
}

pub struct TransactionPool {
    capacity: usize,
    inner: RwLock<TransactionPoolInner>,
    storage_manager: Arc<StorageManager>,
    pub transaction_pubkey_cache:
        RwLock<LruCache<H256, Arc<SignedTransaction>>>,
    worker_pool: Mutex<ThreadPool>,
}

pub type SharedTransactionPool = Arc<TransactionPool>;

impl TransactionPool {
    pub fn with_capacity(
        capacity: usize, storage_manager: Arc<StorageManager>,
        worker_pool: ThreadPool,
    ) -> Self
    {
        TransactionPool {
            capacity,
            inner: RwLock::new(TransactionPoolInner::new()),
            storage_manager,
            // TODO Cache capacity should be set seperately
            transaction_pubkey_cache: RwLock::new(LruCache::new(capacity )),
            worker_pool: Mutex::new(worker_pool),
        }
    }

    pub fn len(&self) -> usize { self.inner.read().len() }

    pub fn insert_new_transactions(
        &self, latest_epoch: EpochId,
        transactions: Vec<TransactionWithSignature>,
    )
    {
        // FIXME: do not unwrap.
        let mut signed_trans = Vec::new();

        let uncached_trans: Vec<TransactionWithSignature>;
        {
            let mut tx_cache = self.transaction_pubkey_cache.write();
            uncached_trans = transactions
                .into_iter()
                .filter(|tx| tx_cache.get(&tx.hash()).is_none())
                .collect();
        }
        if uncached_trans.len() < WORKER_COMPUTATION_PARALLELISM * 8 {
            let mut signed_txes = Vec::new();
            for tx in uncached_trans {
                if let Ok(public) = tx.recover_public() {
                    let signed_tx =
                        Arc::new(SignedTransaction::new(public, tx));
                    signed_txes.push(signed_tx);
                } else {
                    debug!(
                        "Unable to recover the public key of transaction {:?}",
                        tx.hash()
                    );
                }
            }
            signed_trans.push(signed_txes);
        } else {
            let tx_num = uncached_trans.len();
            let tx_num_per_worker = tx_num / WORKER_COMPUTATION_PARALLELISM;
            let mut remainder =
                tx_num - (tx_num_per_worker * WORKER_COMPUTATION_PARALLELISM);
            let mut start_idx = 0;
            let mut end_idx = 0;
            let mut unsigned_trans = Vec::new();

            for tx in uncached_trans {
                if start_idx == end_idx {
                    // a new segment of transactions
                    end_idx = start_idx + tx_num_per_worker;
                    if remainder > 0 {
                        end_idx += 1;
                        remainder -= 1;
                    }
                    let unsigned_txes = Vec::new();
                    unsigned_trans.push(unsigned_txes);
                }

                unsigned_trans.last_mut().unwrap().push(tx);

                start_idx += 1;
            }

            signed_trans.resize(unsigned_trans.len(), Vec::new());
            let (sender, receiver) = channel();
            let worker_pool = self.worker_pool.lock().clone();
            let mut idx = 0;
            for unsigned_txes in unsigned_trans {
                let sender = sender.clone();
                worker_pool.execute(move || {
                    let mut signed_txes = Vec::new();
                    for tx in unsigned_txes {
                        if let Ok(public) = tx.recover_public() {
                            let signed_tx = Arc::new(SignedTransaction::new(public, tx));
                            signed_txes.push(signed_tx);
                        } else {
                            debug!(
                                "Unable to recover the public key of transaction {:?}",
                                tx.hash()
                            );
                        }
                    }
                    sender.send((idx, signed_txes)).unwrap();
                });
                idx += 1;
            }
            worker_pool.join();

            for (idx, signed_txes) in receiver.iter().take(signed_trans.len()) {
                signed_trans[idx] = signed_txes;
            }
        }

        let mut account_cache = AccountCache::new(
            self.storage_manager.get_state_at(latest_epoch).unwrap(),
        );
        {
            let mut tx_cache = self.transaction_pubkey_cache.write();
            for txes in signed_trans {
                for tx in txes {
                    tx_cache.put(tx.hash(), tx.clone());
                    if !self.verify_transaction(tx.as_ref()) {
                        warn!("Transaction discarded due to failure of passing verification {:?}", tx.hash());
                        continue;
                    }
                    self.add_with_readiness(&mut account_cache, tx);
                }
            }
        }
    }

    // verify transactions based on the rules that
    // have nothing to do with readiness
    pub fn verify_transaction(&self, transaction: &SignedTransaction) -> bool {
        // check transaction gas limit
        if transaction.gas > DEFAULT_MAX_TRANSACTION_GAS_LIMIT.into() {
            warn!(
                "Transaction discarded due to above gas limit: {} > {}",
                transaction.gas(),
                DEFAULT_MAX_TRANSACTION_GAS_LIMIT
            );
            return false;
        }

        // check transaction gas price
        if transaction.gas_price < DEFAULT_MIN_TRANSACTION_GAS_PRICE.into()
            || transaction.gas.is_zero()
        {
            warn!("Transaction {} discarded due to below minimal gas price: price {}, gas {}", transaction.hash(), transaction.gas_price, transaction.gas);
            return false;
        }

        if transaction.transaction.verify_basic().is_err() {
            warn!("Transaction {:?} discarded due to not pass basic verification.", transaction.hash());
            return false;
        }

        true
    }

    // The second step verification for ready transactions
    pub fn verify_ready_transaction(
        &self, account: &Account, transaction: &SignedTransaction,
    ) -> bool {
        // check balance
        let cost = transaction.value + transaction.gas_price * transaction.gas;
        if account.balance < cost {
            trace!(
                "Transaction {} not ready due to not enough balance: {} < {}",
                transaction.hash(),
                account.balance,
                cost
            );
            return false;
        }

        true
    }

    pub fn add_with_readiness(
        &self, account_cache: &mut AccountCache,
        transaction: Arc<SignedTransaction>,
    )
    {
        let mut inner = self.inner.write();
        let inner = inner.deref_mut();

        if self.capacity <= inner.len() {
            warn!("Transaction discarded due to insufficient txpool capacity: {:?}", transaction.hash());
            return;
        }

        match account_cache.is_ready(&transaction) {
            Readiness::Ready => {
                let account =
                    account_cache.accounts.get_mut(&transaction.sender);
                if let Some(mut account) = account {
                    if self
                        .verify_ready_transaction(account, transaction.as_ref())
                    {
                        if self.add_ready_without_lock(inner, transaction) {
                            account.nonce = account.nonce + 1;
                        }
                    } else {
                        self.add_pending_without_lock(inner, transaction);
                    }
                } else {
                    warn!("Ready transaction {} discarded due to sender not exist (should not happen!)", transaction.hash());
                }
            }
            Readiness::Future => {
                self.add_pending_without_lock(inner, transaction);
            }
            Readiness::TooDistantFuture => {
                warn!("Transaction {:?} is discarded due to in too distant future", transaction.hash());
            }
            Readiness::Stale => {
                warn!(
                    "Transaction {:?} is discarded due to stale nonce",
                    transaction.hash()
                );
            }
        }
    }

    pub fn add_ready(&self, transaction: Arc<SignedTransaction>) -> bool {
        let mut inner = self.inner.write();
        let inner = inner.deref_mut();
        self.add_ready_without_lock(inner, transaction)
    }

    pub fn add_ready_without_lock(
        &self, inner: &mut TransactionPoolInner,
        transaction: Arc<SignedTransaction>,
    ) -> bool
    {
        trace!(
            "Insert tx into ready hash={:?} sender={:?}",
            transaction.hash(),
            transaction.sender
        );
        inner
            .ready_transactions
            .insert(
                transaction.hash(),
                transaction.clone(),
                U512::from(transaction.gas_price),
            )
            .is_none()
    }

    pub fn add_pending(&self, transaction: Arc<SignedTransaction>) -> bool {
        let mut inner = self.inner.write();
        let inner = inner.deref_mut();
        self.add_pending_without_lock(inner, transaction)
    }

    pub fn recycle_future_transactions(
        &self, transactions: Vec<Arc<SignedTransaction>>, state: Storage,
    ) {
        let mut account_cache = AccountCache::new(state);
        for tx in transactions {
            self.add_with_readiness(&mut account_cache, tx);
        }
    }

    pub fn add_pending_without_lock(
        &self, inner: &mut TransactionPoolInner,
        transaction: Arc<SignedTransaction>,
    ) -> bool
    {
        trace!(
            "Insert tx into pending hash={:?} sender={:?}",
            transaction.hash(),
            transaction.sender
        );
        inner.pending_transactions.insert(transaction)
    }

    pub fn remove_ready(&self, transaction: Arc<SignedTransaction>) -> bool {
        let mut inner = self.inner.write();
        let inner = inner.deref_mut();
        if self.remove_ready_without_lock(inner, transaction).is_some() {
            true
        } else {
            false
        }
    }

    pub fn remove_ready_without_lock(
        &self, inner: &mut TransactionPoolInner,
        transaction: Arc<SignedTransaction>,
    ) -> Option<Arc<SignedTransaction>>
    {
        let hash = transaction.hash();
        inner.ready_transactions.remove(&hash)
    }

    pub fn remove_pending(&self, transaction: &SignedTransaction) -> bool {
        let mut inner = self.inner.write();
        let inner = inner.deref_mut();
        if self
            .remove_pending_without_lock(inner, transaction)
            .is_some()
        {
            true
        } else {
            false
        }
    }

    pub fn remove_pending_without_lock(
        &self, inner: &mut TransactionPoolInner,
        transaction: &SignedTransaction,
    ) -> Option<Arc<SignedTransaction>>
    {
        inner
            .pending_transactions
            .remove(&transaction.sender, &transaction.nonce)
    }

    /// pack at most num_txs transactions randomly
    pub fn pack_transactions<'a>(
        &self, num_txs: usize, state: State<'a>,
    ) -> Vec<Arc<SignedTransaction>> {
        let mut inner = self.inner.write();
        let mut packed_transactions: Vec<Arc<SignedTransaction>> = Vec::new();
        let num_txs = min(num_txs, inner.ready_transactions.len());
        let mut nonce_map = HashMap::new();
        let mut future_txs = HashMap::new();
        debug!(
            "Before packing ready pool size:{}, pending pool size:{}",
            inner.ready_transactions.len(),
            inner.pending_transactions.len()
        );

        for _ in 0..num_txs {
            let sum_gas_price = inner.ready_transactions.sum_weight();
            let mut rand_value: U512 = U512::from(H512::random());
            assert_ne!(inner.ready_transactions.len(), 0);
            assert_ne!(sum_gas_price, 0.into());
            rand_value = rand_value % sum_gas_price;

            let tx = inner
                .ready_transactions
                .get_by_weight(rand_value)
                .expect("Failed to pick transaction by weight")
                .clone();
            trace!("Get transaction from ready pool. tx: {:?}", tx.clone());
            inner.ready_transactions.remove(&tx.hash());
            let sender = tx.sender;
            let nonce_entry = nonce_map.entry(sender);
            let state_nonce = state.nonce(&sender);
            if state_nonce.is_err() {
                debug!(
                    "state nonce error: {:?}, tx: {:?}",
                    state_nonce,
                    tx.clone()
                );
                inner.pending_transactions.insert(tx);
                continue;
            }
            let nonce =
                nonce_entry.or_insert(state_nonce.expect("Not err here"));
            if tx.nonce > *nonce {
                future_txs
                    .entry(sender)
                    .or_insert(HashMap::new())
                    .insert(tx.nonce, tx);
            } else if tx.nonce == *nonce {
                *nonce += 1.into();
                packed_transactions.push(tx);
                if let Some(tx_map) = future_txs.get_mut(&sender) {
                    loop {
                        if tx_map.is_empty() {
                            break;
                        }
                        if let Some(tx) = tx_map.remove(nonce) {
                            packed_transactions.push(tx);
                            *nonce += 1.into();
                        } else {
                            break;
                        }
                    }
                }
            }
        }

        for tx in packed_transactions.iter() {
            inner.ready_transactions.insert(
                tx.hash(),
                tx.clone(),
                U512::from(tx.gas_price),
            );
        }

        for (_, txs) in future_txs.into_iter() {
            for (_, tx) in txs.into_iter() {
                let gas_price = U512::from(tx.gas_price);
                inner.ready_transactions.insert(tx.hash(), tx, gas_price);
            }
        }
        debug!(
            "After packing ready pool size:{}, pending pool size:{}",
            inner.ready_transactions.len(),
            inner.pending_transactions.len()
        );

        packed_transactions
    }

    pub fn transactions_to_propagate(&self) -> Vec<Arc<SignedTransaction>> {
        let inner = self.inner.read();
        inner
            .pending_transactions
            .pool
            .iter()
            .flat_map(|(_, bucket)| bucket.bucket.iter())
            .map(|(_, x)| x.clone())
            .chain(inner.ready_transactions.iter().map(|(_, x)| x.clone()))
            .collect()
    }

    pub fn notify_ready(&self, address: &Address, account: &Account) {
        let mut inner = self.inner.write();
        let inner = inner.deref_mut();
        let mut nonce = account.nonce.clone();

        trace!("Notify ready {:?} with nonce {:?}", address, nonce);

        loop {
            let mut success = false;
            if let Some(tx) = inner.pending_transactions.get(address, &nonce) {
                trace!(
                    "We got the tx from pending_pool with hash {:?}",
                    tx.hash()
                );
                if self.verify_ready_transaction(account, tx.as_ref()) {
                    success = true;
                    trace!(
                        "Successfully verified tx with hash {:?}",
                        tx.hash()
                    );
                }
            }
            if success {
                if let Some(tx) =
                    inner.pending_transactions.remove(address, &nonce)
                {
                    if !self.add_ready_without_lock(inner, tx) {
                        trace!(
                            "Check passed but fail to insert ready transaction"
                        );
                    }
                    nonce += 1.into();
                    continue;
                }
            }
            break;
        }
    }
}

mod impls;
mod ready;

#[cfg(test)]
mod tests;

extern crate rand;

pub use self::impls::TreapMap;
use self::ready::Readiness;
use super::{SharedStateManager, State, StateManagerTrait, StateTrait};
use ethereum_types::{Address, H256, H512, U256, U512};
use parking_lot::RwLock;
use primitives::{
    Account, EpochId, SignedTransaction, TransactionWithSignature,
};
use rlp::decode;
use std::{
    cmp::{min, Ordering},
    collections::HashMap,
    ops::DerefMut,
    sync::Arc,
};

pub const DEFAULT_MIN_TRANSACTION_GAS_PRICE: u64 = 100;
pub const DEFAULT_MAX_TRANSACTION_GAS_LIMIT: u64 = 100_000;
pub const DEFAULT_MAX_BLOCK_GAS_LIMIT: u64 = 30_000 * 100_000;

pub const FURTHEST_FUTURE_TRANSACTION_NONCE_OFFSET: u32 = 2000;

pub struct AccountCache<'cache, 'state: 'cache> {
    pub accounts: HashMap<Address, Account>,
    pub state: &'cache State<'state>,
}

impl<'cache, 'state> AccountCache<'cache, 'state> {
    pub fn new(state: &'cache State<'state>) -> Self {
        AccountCache {
            accounts: HashMap::new(),
            state,
        }
    }

    pub fn get_ready_account(&mut self, address: &Address) -> Option<&Account> {
        self.accounts.get(address)
    }

    fn is_ready(&mut self, tx: &SignedTransaction) -> Readiness {
        let sender = tx.sender();
        if !self.accounts.contains_key(&sender) {
            let account = self
                .state
                .get(sender.as_ref())
                .map(|rlp| decode::<Account>(rlp.as_ref()).unwrap())
                .ok();
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
    pub bucket: HashMap<U256, SignedTransaction>,
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

    pub fn insert(&mut self, tx: SignedTransaction) -> bool {
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
    ) -> Option<SignedTransaction> {
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
    ) -> Option<&SignedTransaction> {
        self.pool
            .get(address)
            .and_then(|bucket| bucket.bucket.get(nonce))
    }
}

pub struct TransactionPoolInner {
    pending_transactions: PendingTransactionPool,
    ready_transactions: TreapMap<H256, SignedTransaction, U512>,
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
    state_manager: SharedStateManager,
}

pub type SharedTransactionPool = Arc<TransactionPool>;

impl TransactionPool {
    pub fn with_capacity(
        capacity: usize, state_manager: SharedStateManager,
    ) -> Self {
        TransactionPool {
            capacity,
            inner: RwLock::new(TransactionPoolInner::new()),
            state_manager,
        }
    }

    pub fn len(&self) -> usize { self.inner.read().len() }

    pub fn insert_new_transactions(
        &self, latest_epoch: EpochId,
        transactions: Vec<TransactionWithSignature>,
    )
    {
        let state = self.state_manager.get_state_at(latest_epoch);
        let mut account_cache = AccountCache::new(&state);
        for tx in transactions {
            if let Ok(public) = tx.recover_public() {
                let signed_tx = SignedTransaction::new(public, tx);
                if !self.verify_transaction(&signed_tx) {
                    warn!("Transaction discarded due to failure of passing verification {:?}", signed_tx.hash());
                    continue;
                }
                self.add_with_readiness(&mut account_cache, signed_tx);
            } else {
                debug!(
                    "Unable to recover the public key of transaction {:?}",
                    tx.hash()
                );
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
            warn!(
                "Transaction {} discarded due to not enough balance: {} < {}",
                transaction.hash(),
                account.balance,
                cost
            );
            return false;
        }

        true
    }

    pub fn add_with_readiness(
        &self, account_cache: &mut AccountCache, transaction: SignedTransaction,
    ) {
        let mut inner = self.inner.write();
        let inner = inner.deref_mut();

        if self.capacity <= inner.len() {
            warn!("Transaction discarded due to insufficient txpool capacity: {:?}", transaction.hash());
            return;
        }

        match account_cache.is_ready(&transaction) {
            Readiness::Ready => {
                let mut account =
                    account_cache.accounts.get_mut(&transaction.sender);
                if let Some(mut account) = account {
                    if self.verify_ready_transaction(account, &transaction) {
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

    pub fn add_ready(&self, transaction: SignedTransaction) -> bool {
        let mut inner = self.inner.write();
        let inner = inner.deref_mut();
        self.add_ready_without_lock(inner, transaction)
    }

    pub fn add_ready_without_lock(
        &self, inner: &mut TransactionPoolInner, transaction: SignedTransaction,
    ) -> bool {
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

    pub fn add_pending(&self, transaction: SignedTransaction) -> bool {
        let mut inner = self.inner.write();
        let inner = inner.deref_mut();
        self.add_pending_without_lock(inner, transaction)
    }

    pub fn add_pending_without_lock(
        &self, inner: &mut TransactionPoolInner, transaction: SignedTransaction,
    ) -> bool {
        trace!(
            "Insert tx into pending hash={:?} sender={:?}",
            transaction.hash(),
            transaction.sender
        );
        inner.pending_transactions.insert(transaction)
    }

    pub fn remove_ready(&self, transaction: SignedTransaction) -> bool {
        let mut inner = self.inner.write();
        let inner = inner.deref_mut();
        if self.remove_ready_without_lock(inner, transaction).is_some() {
            true
        } else {
            false
        }
    }

    pub fn remove_ready_without_lock(
        &self, inner: &mut TransactionPoolInner, transaction: SignedTransaction,
    ) -> Option<SignedTransaction> {
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
    ) -> Option<SignedTransaction>
    {
        inner
            .pending_transactions
            .remove(&transaction.sender, &transaction.nonce)
    }

    /// pack at most num_txs transactions randomly
    pub fn pack_transactions(&self, num_txs: usize) -> Vec<SignedTransaction> {
        let mut inner = self.inner.write();
        let mut packed_transactions: Vec<SignedTransaction> = Vec::new();
        let num_txs = min(num_txs, inner.ready_transactions.len());

        for _ in 0..num_txs {
            let mut sum_gas_price = inner.ready_transactions.sum_weight();
            let mut rand_value: U512 = U512::from(H512::random());
            assert_ne!(inner.ready_transactions.len(), 0);
            assert_ne!(sum_gas_price, 0.into());
            rand_value = rand_value % sum_gas_price;

            let tx = inner
                .ready_transactions
                .get_by_weight(rand_value)
                .expect("Failed to pick transaction by weight")
                .clone();

            inner.ready_transactions.remove(&tx.hash());
            packed_transactions.push(tx);
        }

        for tx in packed_transactions.iter() {
            inner.ready_transactions.insert(
                tx.hash(),
                tx.clone(),
                U512::from(tx.gas_price),
            );
        }

        packed_transactions
    }

    pub fn transactions_to_propagate(&self) -> Vec<SignedTransaction> {
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
        let nonce = &account.nonce;

        debug!("Notify ready {:?} with nonce {:?}", address, nonce);

        let mut success = false;
        if let Some(tx) = inner.pending_transactions.get(address, nonce) {
            debug!("We got the tx from pending_pool with hash {:?}", tx.hash());
            if self.verify_ready_transaction(account, tx) {
                success = true;
                debug!("Successfully verified tx with hash {:?}", tx.hash());
            }
        }
        if success {
            if let Some(tx) = inner.pending_transactions.remove(address, nonce)
            {
                if !self.add_ready_without_lock(inner, tx) {
                    warn!("Check passed but fail to insert ready transaction");
                }
            }
        }
    }
}

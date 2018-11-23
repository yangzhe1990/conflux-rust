mod impls;
mod ready;

extern crate rand;

use self::ready::Readiness;
use super::{SharedStateManager, State, StateManagerTrait, StateTrait};
use ethereum_types::{Address, H256, H512, U256, U512};
use parking_lot::{Mutex, RwLock};
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
                Ordering::Greater => Readiness::Future,
                Ordering::Less => Readiness::Stale,
                Ordering::Equal => {
                    account.nonce = account.nonce + 1;
                    Readiness::Ready
                }
            }
        } else {
            Readiness::Future
        }
    }
}

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
    pending_transactions: HashMap<H256, OrderedTransaction>,
}

impl TransactionPoolInner {
    pub fn new() -> Self {
        TransactionPoolInner {
            pending_transactions: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize { self.pending_transactions.len() }
}

pub struct TransactionPool {
    capacity: usize,
    // Locking order:
    // waiters.lock()
    // inner.write()
    pub waiters: Mutex<HashMap<Address, HashMap<U256, Vec<H256>>>>,
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
            waiters: Mutex::new(HashMap::new()),
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
        let mut waiters = self.waiters.lock();
        let mut inner = self.inner.write();
        let inner = inner.deref_mut();

        match account_cache.is_ready(&transaction) {
            Readiness::Ready => {
                let account =
                    account_cache.get_ready_account(&transaction.sender);
                if let Some(account) = account {
                    if self.verify_ready_transaction(account, &transaction) {
                        self.add_ready_without_lock(inner, transaction);
                    }
                } else {
                    warn!("Ready transaction {} discarded due to sender not exist (should not happen!)", transaction.hash());
                }
            }
            Readiness::Future => {
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
            Readiness::Stale => {
                warn!(
                    "Transaction {:?} is discarded due to stale nonce",
                    transaction.hash()
                );
            }
        }
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
        if self.capacity <= inner.pending_transactions.len() {
            debug!("Rejected a transaction {:?} because of insufficient transaction pool capacity!", transaction.hash());
            // pool is full
            return false;
        }

        let hash = transaction.hash();
        if inner.pending_transactions.contains_key(&hash) {
            debug!(
                "Rejected a transaction {:?} because it already exists!",
                transaction.hash()
            );
            // already exists
            return false;
        }

        inner
            .pending_transactions
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
        &self, inner: &mut TransactionPoolInner, transaction: SignedTransaction,
    ) -> Option<OrderedTransaction> {
        let hash = transaction.hash();
        inner.pending_transactions.remove(&hash)
    }

    /// pack at most num_txs transactions randomly
    pub fn pack_transactions(&self, num_txs: usize) -> Vec<SignedTransaction> {
        //TODO: should be done by O(num_txs * log)
        let mut transaction_sequence: Vec<SignedTransaction> = Vec::new();
        let mut packed_transaction: Vec<SignedTransaction> = Vec::new();
        let mut gas_price_sequence: Vec<U512> = Vec::new();
        let mut sum_gas_price: U512 = 0.into();

        let inner = self.inner.read();
        for (_, tx) in inner.pending_transactions.iter() {
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
            .pending_transactions
            .iter()
            .map(|x| x.1.transaction.clone())
            .collect()
    }

    pub fn notify_ready(&self, address: &Address, account: &Account) {
        let mut waiters = self.waiters.lock();
        let mut inner = self.inner.write();
        let inner = inner.deref_mut();

        let nonce = &account.nonce;
        let clear_waiter = if let Some(m) = waiters.get_mut(address) {
            if let Some(hvec) = m.get_mut(nonce) {
                for h in hvec.iter() {
                    if let Some(tx) = inner.pending_transactions.remove(h) {
                        if self
                            .verify_ready_transaction(account, &tx.transaction)
                        {
                            self.add_ready_without_lock(inner, tx.transaction);
                        }
                    }
                }
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

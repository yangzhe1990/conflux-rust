use cfxcore;
use clap::{App, Arg};
use env_logger;
use error_chain::*;
use ethcore::{
    ethereum::ethash::EthashParams, spec::CommonParams as EthCommonParams,
};
use ethcore_types::block::Block as EthBlock;
use ethereum_types::*;
use ethjson::spec::Spec as EthSpec;
use ethkey::public_to_address;
use heapsize::HeapSizeOf;
use log::*;
use parking_lot::Mutex;
use rlp::{Decodable, *};
use std::{
    collections::{vec_deque::VecDeque, HashMap},
    fs::File,
    io::{self, Read, Write},
    mem,
    ops::Shr,
    slice,
    sync::{mpsc, Arc},
    thread::{self, JoinHandle},
    vec::Vec,
};

mod errors;

heapsize::known_heap_size!(
    0,
    EthTxVerifierWIPBlockInfo,
    RealizedEthTx,
    EthTxVerifierRequest,
    EthTxExtractor
);

#[derive(Clone)]
struct ArcMutexEthTxExtractor(Arc<Mutex<EthTxExtractor>>);

heapsize::known_heap_size!(0, ArcMutexEthTxExtractor);

#[derive(Clone)]
enum EthTxType {
    BlockRewardAndTxFee,
    UncleReward,
    Transaction,
    Dao,
    GenesisAccount,
}

#[derive(Clone)]
pub struct RealizedEthTx {
    // Sender spends fee + amount.
    // Receiver receives amount.
    sender: Option<H160>,
    // None for conotract creation.
    receiver: Option<H160>,
    tx_fee_wei: U256,
    amount_wei: U256,
    types: EthTxType,
}

impl Encodable for RealizedEthTx {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(5)
            .append(&self.sender)
            .append(&self.receiver)
            .append(&self.tx_fee_wei)
            .append(&self.amount_wei)
            .append(&(self.types.clone() as u32));
    }
}

#[derive(Clone, Default)]
pub struct DaoHardforkInfo {
    dao_hardfork_accounts: Vec<Address>,
    dao_hardfork_beneficiary: Address,
    dao_hardfork_transition: u64,
}

#[derive(Clone)]
pub struct EthTxVerifierRequest {
    block_number: u64,
    base_transaction_number: u64,
    transaction_index: usize,
    block: Arc<EthBlock>,
    sender: Address,

    /// To issue transaction verification call afterwards.
    tx_extractor: Arc<Mutex<EthTxExtractor>>,
}

pub struct EthTxVerifierWorkerThread {
    current_nonce_map: HashMap<H160, U256>,

    n_contract_creation: u64,
    n_nonce_error: u64,

    tx_sender: Mutex<mpsc::Sender<Option<EthTxVerifierRequest>>>,

    out_streamer: Arc<Mutex<EthTxOutStreamer>>,

    thread_handle: Option<JoinHandle<Option<()>>>,
}

impl Drop for EthTxVerifierWorkerThread {
    fn drop(&mut self) {
        println!("stopping verifier worker thread.");

        self.tx_sender.lock().send(None).ok();

        println!(
            "heapsize {}\tEthTxVerifierWorkerThread#current_nonce_map.",
            self.current_nonce_map.heap_size_of_children()
        );

        self.thread_handle.take().unwrap().join().ok();
    }
}

impl EthTxVerifierWorkerThread {
    pub fn new(
        out_streamer: Arc<Mutex<EthTxOutStreamer>>,
    ) -> Arc<Mutex<EthTxVerifierWorkerThread>> {
        let (sender, receiver) = mpsc::channel();

        let worker = Arc::new(Mutex::new(EthTxVerifierWorkerThread {
            current_nonce_map: Default::default(),
            n_contract_creation: 0,
            n_nonce_error: 0,
            tx_sender: Mutex::new(sender),
            out_streamer: out_streamer,
            thread_handle: None,
        }));

        let weak = Arc::downgrade(&worker);
        let join_handle = thread::spawn(move || -> Option<()> {
            loop {
                let maybe_tx = receiver.recv().ok()?;

                if maybe_tx.is_none() {
                    // An empty request is signal for exit,
                    return Some(());
                } else {
                    let worker = match weak.upgrade() {
                        Some(worker) => worker,
                        None => {
                            println!("tx verifier worker thread exits.");
                            return None;
                        }
                    };
                    let mut worker_mut = worker.lock();
                    let result =
                        worker_mut.verify_tx(maybe_tx.as_ref().unwrap());
                    worker_mut
                        .out_streamer
                        .lock()
                        .set_result(maybe_tx.unwrap(), result);
                }
            }
        });

        worker.lock().thread_handle = Some(join_handle);

        worker
    }

    pub fn n_contract_creation(&self) -> u64 { self.n_contract_creation }

    pub fn n_nonce_error(&self) -> u64 { self.n_nonce_error }

    pub fn n_accounts(&self) -> usize { self.current_nonce_map.len() }

    pub fn send_request(&self, req: EthTxVerifierRequest) {
        self.tx_sender.lock().send(Some(req)).unwrap();
    }

    fn check_nonce(&mut self, sender: &Address, nonce: &U256) -> bool {
        let zero_nonce = 0.into();
        let current_nonce =
            self.current_nonce_map.get(sender).unwrap_or(&zero_nonce);
        let result = current_nonce.eq(nonce);
        if !result {
            info!("nonce error: current {}, expected {}", current_nonce, nonce);
        }
        result
    }

    fn verify_tx(
        &mut self, tx_req: &EthTxVerifierRequest,
    ) -> Option<RealizedEthTx> {
        let tx = unsafe {
            tx_req
                .block
                .transactions
                .get_unchecked(tx_req.transaction_index)
        };

        // Verify and update nonce.
        if !self.check_nonce(&tx_req.sender, &tx.nonce) {
            self.n_nonce_error += 1;
            return None;
        } else {
            self.current_nonce_map
                .insert(tx_req.sender.clone(), tx.nonce + 1);
        }

        // We do not verify the balance. Instead, we allow the balance to go
        // negative, because we are only testing the tps capability of
        // Conflux.

        let receiver;
        let tx_fee = tx.gas * tx.gas_price;;

        match tx.action {
            ethcore_types::transaction::Action::Call(ref to) => {
                receiver = Some(*to);
            }
            _ => {
                // Create a contract.
                // we do not admit creation of contract in verifier
                // simulation.
                self.n_contract_creation += 1;
                // We should credit transaction fee to the miner.
                // and advance nonce for sender.
                receiver = None;
            }
        }

        Some(RealizedEthTx {
            sender: Some(tx_req.sender),
            receiver: receiver,
            amount_wei: tx.value,
            tx_fee_wei: tx_fee,
            types: EthTxType::Transaction,
        })
    }

    pub fn exec_reward(&mut self, tx: &RealizedEthTx) {
        // TODO: it's no-op for the moment because we don't current do anything
        // about account balance.
        unimplemented!()
    }
}

pub struct EthTxVerifier {
    workers: Vec<Arc<Mutex<EthTxVerifierWorkerThread>>>,
    pub out_streamer: Arc<Mutex<EthTxOutStreamer>>,
}

impl EthTxVerifier {
    const N_TX_VERIFIERS: usize = 8;

    // FILE
    pub fn new(path_to_tx_file: &str) -> errors::Result<EthTxVerifier> {
        let out_streamer = Arc::new(Mutex::new(EthTxOutStreamer {
            transactions_to_write: Default::default(),
            wip_block_info: Default::default(),
            next_block_number: 0,
            transaction_number: 0,
            tx_rlp_file: File::create(path_to_tx_file)?,
            total_verified_tx_rlp_length: 0,
            n_txs: 0,
        }));

        let mut workers = Vec::with_capacity(Self::N_TX_VERIFIERS);
        for _i in 0..Self::N_TX_VERIFIERS {
            workers.push(EthTxVerifierWorkerThread::new(out_streamer.clone()));
        }

        Ok(EthTxVerifier {
            workers: workers,
            out_streamer: out_streamer,
        })
    }

    pub fn total_verified_tx_rlp_length(&self) -> usize {
        self.out_streamer.lock().total_verified_tx_rlp_length
    }
}

#[derive(Default, Clone)]
pub struct EthTxVerifierWIPBlockInfo {
    total_transaction_fee: U256,
    total_transactions_before_block_reward: u32,
    // This field is 0 iff uninitialized WIPBlockInfo.
    total_transactions: u32,
    remaining_transactions: u32,
}

pub struct EthTxOutStreamer {
    // For a 10 million blocks, the information consumes only about 300MB.
    wip_block_info: VecDeque<EthTxVerifierWIPBlockInfo>,
    pub next_block_number: u64,

    /// For transaction insertion.
    pub transaction_number: u64,
    pub transactions_to_write: VecDeque<Option<RealizedEthTx>>,

    // how to know if a tx is ready to be checked? For each sender it must be
    // processed sequentially.
    tx_rlp_file: File,
    total_verified_tx_rlp_length: usize,
    n_txs: u64,
}

impl Drop for EthTxOutStreamer {
    fn drop(&mut self) {
        println!(
            "{} heapsize\tEthTxOutStreamer#wip_block_info.",
            self.wip_block_info.heap_size_of_children()
        );
        println!(
            "{} heapsize\tEthTxOutStreamer#transactions_to_write.",
            self.transactions_to_write.heap_size_of_children()
        );
    }
}

impl EthTxOutStreamer {
    /// Return the total base transaction number for the next block.
    ///
    /// This method should be called after adding the block reward txs.
    fn initialize_for_block(
        &mut self, block_number: u64, adhoc_txs: u32, unverified_txs: u32,
        block_reward_txs: u32, base_transaction_number: u64,
    ) -> u64
    {
        let block_dequeue_index =
            self.get_block_dequeue_index_for(block_number);

        let total_txs = adhoc_txs + unverified_txs + block_reward_txs;
        if block_dequeue_index >= self.wip_block_info.len() {
            self.wip_block_info
                .resize(block_dequeue_index + 1, Default::default());
        }
        self.wip_block_info[block_dequeue_index] = EthTxVerifierWIPBlockInfo {
            total_transaction_fee: 0.into(),
            total_transactions_before_block_reward: adhoc_txs + unverified_txs,
            total_transactions: total_txs,
            remaining_transactions: total_txs,
        };

        let next_base_transaction_number =
            base_transaction_number + total_txs as u64;
        let tx_result_index_limit = self
            .get_transaction_dequeue_index_for(next_base_transaction_number);
        if tx_result_index_limit > self.transactions_to_write.len() {
            self.transactions_to_write
                .resize(tx_result_index_limit, None);
        }

        next_base_transaction_number
    }

    pub fn set_transaction(
        &mut self, block_number: u64, base_transaction_number: u64,
        transaction_index: u64, maybe_result: Option<RealizedEthTx>,
        has_tx_fee: bool,
    )
    {
        let block_dequeue_index =
            self.get_block_dequeue_index_for(block_number);
        match has_tx_fee {
            true => {
                self.wip_block_info[block_dequeue_index]
                    .total_transaction_fee +=
                    maybe_result.as_ref().unwrap().tx_fee_wei
            }
            false => {}
        }

        self.wip_block_info[block_dequeue_index].remaining_transactions -= 1;

        let tx_dequeue_index = self.get_transaction_dequeue_index_for(
            base_transaction_number + transaction_index,
        );
        self.transactions_to_write[tx_dequeue_index] =
            maybe_result.map_or(None, |result| {
                if result.receiver.is_none() {
                    None
                } else {
                    Some(result)
                }
            });
    }

    fn set_result(
        &mut self, request: EthTxVerifierRequest,
        maybe_result: Option<RealizedEthTx>,
    )
    {
        let is_valid_tx = maybe_result.is_some();
        self.set_transaction(
            request.block_number,
            request.base_transaction_number,
            request.transaction_index as u64,
            maybe_result,
            is_valid_tx,
        );
        let block_dequeue_index =
            self.get_block_dequeue_index_for(request.block_number);

        if self.wip_block_info[block_dequeue_index].remaining_transactions == 0
        {
            if request.block_number < 10 {
                info!("all tx verified for block {}", request.block_number);
            }

            // Credit the total tx fee into the block reward.
            let block_reward_tx_dequeue_index = self
                .get_transaction_dequeue_index_for(
                    request.base_transaction_number
                        + self.wip_block_info[block_dequeue_index]
                            .total_transactions_before_block_reward
                            as u64,
                );
            self.transactions_to_write[block_reward_tx_dequeue_index]
                .as_mut()
                .unwrap()
                .amount_wei +=
                self.wip_block_info[block_dequeue_index].total_transaction_fee;
        }

        self.stream_out();
    }

    fn stream_tx(&mut self, tx: &RealizedEthTx) {
        let tx_rlp = tx.rlp_bytes();
        self.tx_rlp_file.write_all(&tx_rlp).unwrap();
        self.total_verified_tx_rlp_length += tx_rlp.len();
        self.n_txs += 1;
    }

    fn stream_genesis_accounts(&mut self, tx: &RealizedEthTx) {
        self.stream_tx(tx)
    }

    fn stream_out(&mut self) {
        while self.wip_block_info.len() > 0
            && self.wip_block_info[0].remaining_transactions == 0
            && self.wip_block_info[0].total_transactions > 0
        {
            let wip_block_info = self.wip_block_info.pop_front();
            let wip_block_info_ref = wip_block_info.as_ref().unwrap();
            self.next_block_number += 1;

            // loop through transactions.
            let n_txs = wip_block_info_ref.total_transactions;
            for _i in 0..n_txs {
                // Pop a tx and process.
                let maybe_tx = self.transactions_to_write.pop_front().unwrap();
                match maybe_tx {
                    Some(tx) => self.stream_tx(&tx),
                    None => {}
                }
            }
            self.transaction_number += n_txs as u64;
        }
    }

    fn get_block_dequeue_index_for(&self, block_number: u64) -> usize {
        (block_number - self.next_block_number) as usize
    }

    fn get_transaction_dequeue_index_for(
        &self, transaction_number: u64,
    ) -> usize {
        (transaction_number - self.transaction_number) as usize
    }
}

pub struct EthTxBasicVerifierResult {
    /// For transaction insertion.
    basic_verification_index: usize,
    results:
        VecDeque<Option<Result<EthTxVerifierRequest, ArcMutexEthTxExtractor>>>,
}

impl Drop for EthTxBasicVerifierResult {
    fn drop(&mut self) {
        assert_eq!(self.results.len(), 0);
        println!(
            "heapsize {}\tEthTxBasicVerifierResult",
            self.results.heap_size_of_children()
        );
    }
}

impl EthTxBasicVerifierResult {
    fn save_result(
        &mut self, basic_verification_index: usize,
        result: Result<EthTxVerifierRequest, ArcMutexEthTxExtractor>,
    )
    {
        let waiting_for_result = None;
        let index = basic_verification_index - self.basic_verification_index;
        if index >= self.results.len() {
            self.results.resize(index + 1, waiting_for_result);
        }
        self.results[index] = Some(result);

        if index == 0 {
            self.process_results();
        }
    }

    fn process_results(&mut self) {
        while self.results.len() > 0 && self.results[0].is_some() {
            let maybe_request = self.results.pop_front().unwrap().unwrap();
            self.basic_verification_index += 1;
            match maybe_request {
                Ok(request) => {
                    request
                        .tx_extractor
                        .clone()
                        .lock()
                        .verify_tx_then_stream_out(request);
                }
                Err(tx_extractor) => {
                    tx_extractor.0.lock().n_tx_verification_error += 1;
                }
            }
        }
    }
}

pub struct EthTxBasicVerifier {
    task_sender: mpsc::Sender<
        Option<
            Box<
                dyn FnMut(
                        
                    ) -> (
                        usize,
                        Result<EthTxVerifierRequest, ArcMutexEthTxExtractor>,
                    ) + Send,
            >,
        >,
    >,
    thread_handle: Option<JoinHandle<Option<()>>>,
}

impl Drop for EthTxBasicVerifier {
    fn drop(&mut self) {
        self.task_sender.send(None).ok();
        println!("stopping basic verifier thread.");
        // FIXME: why it's not stopping and the memory is increasing?

        self.thread_handle.take().unwrap().join().ok();
        println!("basic verifier thread exits.");
    }
}

impl EthTxBasicVerifier {
    pub fn new_arc(
        basic_verifier_result: Arc<Mutex<EthTxBasicVerifierResult>>,
    ) -> Arc<Mutex<EthTxBasicVerifier>> {
        let (sender, receiver) = mpsc::channel();
        let verifier = Arc::new(Mutex::new(EthTxBasicVerifier {
            task_sender: sender,
            thread_handle: None,
        }));

        let results = basic_verifier_result.clone();
        let join_handle = thread::spawn(move || -> Option<()> {
            loop {
                let receive_result = receiver.recv();
                match receive_result {
                    Err(e) => {
                        println!("receive failure {}", e);
                    }
                    Ok(maybe_task) => match maybe_task {
                        Some(mut task) => {
                            let result = task();
                            results.lock().save_result(result.0, result.1);
                        }
                        None => {
                            println!("basic verifier None received.");
                            return Some(());
                        }
                    },
                }
            }
        });
        verifier.lock().thread_handle = Some(join_handle);

        verifier
    }
}

pub struct EthTxExtractor {
    tx_basic_verifiers: Vec<Arc<Mutex<EthTxBasicVerifier>>>,
    ethash_params: EthashParams,
    params: EthCommonParams,
    dao_hardfork_info: Option<Arc<DaoHardforkInfo>>,
    n_blocks: u64,
    n_tx_seen: usize,
    n_tx_verification_error: u64,
    /// Not verifying balance at the moment.
    //current_balance_map: HashMap<H160, U256>,
    base_transaction_number: u64,

    nonce_verifier: EthTxVerifier,

    shared_self: Option<Arc<Mutex<EthTxExtractor>>>,
}

pub struct EthTxExtractorStopper(Arc<Mutex<EthTxExtractor>>);

impl Drop for EthTxExtractorStopper {
    fn drop(&mut self) {
        println!("stopping eth tx extractor.");

        let mut tx_basic_verifiers = vec![];
        {
            let mut extractor_mut = self.0.lock();
            extractor_mut.shared_self.take();
            mem::swap(
                &mut tx_basic_verifiers,
                &mut extractor_mut.tx_basic_verifiers,
            );
        }
        tx_basic_verifiers.drain(..);

        let tx_extractor_locked = self.0.lock();
        println!("loaded {} blocks {} txs {} nonce error {} contract creation {} accounts",
                 tx_extractor_locked.n_blocks(), tx_extractor_locked.n_txs(),
                 tx_extractor_locked.n_nonce_error(), tx_extractor_locked.n_contract_creation(),
                 tx_extractor_locked.n_accounts());
    }
}

impl EthTxExtractor {
    const N_TX_BASIC_VERIFIERS: usize = 8;

    pub fn new_from_spec(
        path: &str, path_to_tx_file: &str,
    ) -> errors::Result<Arc<Mutex<EthTxExtractor>>> {
        let ethash: ethjson::spec::Ethash;
        match EthSpec::load(File::open(path)?)?.engine {
            ethjson::spec::engine::Engine::Ethash(ethash_engine) => {
                ethash = ethash_engine;
            }
            _ => {
                panic!();
            }
        }

        let dao_hardfork_info = match ethash.params.dao_hardfork_transition {
            Some(transition) => Some(Arc::new(DaoHardforkInfo {
                dao_hardfork_transition: transition.0.as_u64(),
                dao_hardfork_accounts: vec![],
                dao_hardfork_beneficiary: ethash
                    .params
                    .dao_hardfork_beneficiary
                    .as_ref()
                    .unwrap()
                    .0,
            })),
            None => None,
        };

        let basic_verifier_result =
            Arc::new(Mutex::new(EthTxBasicVerifierResult {
                basic_verification_index: 0,
                results: Default::default(),
            }));
        let mut tx_basic_verifiers = vec![];
        for _i in 0..Self::N_TX_BASIC_VERIFIERS {
            tx_basic_verifiers.push(EthTxBasicVerifier::new_arc(
                basic_verifier_result.clone(),
            ));
        }

        let mut result = Ok(Arc::new(Mutex::new(EthTxExtractor {
            tx_basic_verifiers: tx_basic_verifiers,
            ethash_params: ethash.params.into(),
            params: EthSpec::load(File::open(path)?)?.params.into(),
            dao_hardfork_info: dao_hardfork_info,
            n_blocks: 0,
            n_tx_seen: 0,
            n_tx_verification_error: 0,
            base_transaction_number: 0,
            nonce_verifier: EthTxVerifier::new(path_to_tx_file)?,
            shared_self: None,
        })));

        {
            let extractor_arc = result.as_mut().unwrap().clone();
            let mut extractor_mut = extractor_arc.lock();
            extractor_mut.shared_self = Some(extractor_arc.clone());

            let spec = EthSpec::load(File::open(path)?)?;

            // Add genesis accounts.
            // WTF, the spec is consumed and there is no way around.
            let mut genesis_account_counts = 0;
            for (address, account) in spec.accounts {
                extractor_mut
                    .get_out_streamer()
                    .lock()
                    .stream_genesis_accounts(&RealizedEthTx {
                        sender: None,
                        receiver: Some(address.0),
                        tx_fee_wei: 0.into(),
                        amount_wei: account.balance.map_or(0.into(), |v| v.0),
                        types: EthTxType::GenesisAccount,
                    });
                genesis_account_counts += 1;
            }

            // Set base transaction number at all places.
            extractor_mut.base_transaction_number = genesis_account_counts;
            extractor_mut.get_out_streamer().lock().transaction_number =
                genesis_account_counts;
        }

        result
    }

    fn get_out_streamer(&self) -> &Mutex<EthTxOutStreamer> {
        self.nonce_verifier.out_streamer.as_ref()
    }

    fn verify_tx_then_stream_out(
        &mut self, tx_verify_request: EthTxVerifierRequest,
    ) {
        let thread = (tx_verify_request.sender.low_u64() & 7) as usize;

        self.nonce_verifier.workers[thread]
            .lock()
            .send_request(tx_verify_request);
    }

    pub fn get_balance(&self, _address: &H160) -> Option<&U256> { None }

    pub fn add_tx_from_system(
        &mut self, tx: RealizedEthTx, tx_number_in_block: u32,
    ) {
        self.get_out_streamer().lock().set_transaction(
            self.n_blocks,
            self.base_transaction_number,
            tx_number_in_block.into(),
            Some(tx),
            false,
        );
    }

    pub fn get_block_reward_base(&self, block_number: u64) -> U256 {
        let (_, reward) = self.ethash_params.block_reward.iter()
            .rev()
            .find(|&(block, _)| *block <= block_number)
            .expect("Current block's reward is not found; verifier indicates a chain config error; qed");
        *reward
    }

    pub fn n_blocks(&self) -> u64 { self.n_blocks }

    pub fn n_tx_seen(&self) -> usize { self.n_tx_seen }

    pub fn n_txs(&self) -> u64 { self.get_out_streamer().lock().n_txs }

    pub fn n_contract_creation(&self) -> u64 {
        let mut sum = 0;
        for verifier in &self.nonce_verifier.workers {
            sum += verifier.lock().n_contract_creation();
        }
        sum
    }

    pub fn n_tx_verification_error(&self) -> u64 {
        self.n_tx_verification_error
    }

    pub fn n_nonce_error(&self) -> u64 {
        let mut sum = 0;
        for verifier in &self.nonce_verifier.workers {
            sum += verifier.lock().n_nonce_error();
        }
        sum
    }

    pub fn n_accounts(&self) -> usize {
        let mut sum = 0;
        for verifier in &self.nonce_verifier.workers {
            sum += verifier.lock().n_accounts();
        }
        sum
    }

    pub fn total_verified_tx_rlp_length(&self) -> usize {
        self.get_out_streamer().lock().total_verified_tx_rlp_length
    }

    pub fn tx_verify(
        &mut self, check_low_s: bool, chain_id: Option<u64>,
        allow_empty_signature: bool, block: Arc<EthBlock>, base_tx_number: u64,
        transaction_index: usize, worker: usize,
        basic_verification_index: usize,
    )
    {
        let tx_extractor = self.shared_self.as_ref().unwrap().clone();
        self.tx_basic_verifiers[worker]
            .lock()
            .task_sender
            .send(Some(Box::new(move || {
                info!("run basic verifier task.");
                let tx = unsafe {
                    block.transactions.get_unchecked(transaction_index)
                };
                let maybe_sender = tx
                    .verify_basic(check_low_s, chain_id, allow_empty_signature)
                    .ok()
                    .map(|_| public_to_address(&tx.recover_public().unwrap()));
                info!("basic verifier task about to finish.");
                match maybe_sender {
                    Some(sender) => (
                        basic_verification_index,
                        Ok(EthTxVerifierRequest {
                            block: block.clone(),
                            block_number: block.header.number(),
                            transaction_index: transaction_index,
                            base_transaction_number: base_tx_number,
                            sender: sender.clone(),
                            tx_extractor: tx_extractor.clone(),
                        }),
                    ),
                    None => (
                        basic_verification_index,
                        Err(ArcMutexEthTxExtractor(tx_extractor.clone())),
                    ),
                }
            })))
            .unwrap();
        // FIXME: remove debug code.
        info!(
            "run basic verifier for basic_verification_index = {}, worker = {}",
            basic_verification_index, worker
        );
    }

    pub fn add_block(&mut self, block: Arc<EthBlock>) {
        let block_number = block.header.number();
        let dao_hardfork = self.dao_hardfork_info.is_some()
            && block_number
                == self
                    .dao_hardfork_info
                    .as_ref()
                    .unwrap()
                    .dao_hardfork_transition;

        let mut ad_hoc_tx_numbers = 0;
        if dao_hardfork {
            ad_hoc_tx_numbers = self
                .dao_hardfork_info
                .as_ref()
                .unwrap()
                .dao_hardfork_accounts
                .len() as u32;
        }

        let new_base_transaction_number =
            self.get_out_streamer().lock().initialize_for_block(
                block_number,
                ad_hoc_tx_numbers,
                block.transactions.len() as u32,
                1 + block.uncles.len() as u32,
                self.base_transaction_number,
            );

        // Dao
        if dao_hardfork {
            let mut ad_hoc_tx_index = 0;

            let dao_hardfork_info =
                self.dao_hardfork_info.as_ref().unwrap().clone();

            let beneficiary = &dao_hardfork_info.dao_hardfork_beneficiary;
            let accounts = &dao_hardfork_info.dao_hardfork_accounts;

            for account in accounts {
                // TODO: we don't support updating loading the balance because
                // it's extremely slow. and it make parallelism
                // hard.
                let balance =
                    self.get_balance(&account).map_or(0.into(), |x| *x);

                self.add_tx_from_system(
                    RealizedEthTx {
                        sender: Some(account.clone()),
                        receiver: Some(beneficiary.clone()),
                        tx_fee_wei: 0.into(),
                        amount_wei: balance,
                        types: EthTxType::Dao,
                    },
                    ad_hoc_tx_index,
                );

                ad_hoc_tx_index += 1;
            }
        }

        // verify txs
        let check_low_s =
            block_number >= self.ethash_params.homestead_transition;

        let use_tx_chain_id =
            block_number < self.params.validate_chain_id_transition;
        let chain_id =
            if block_number < self.params.validate_chain_id_transition {
                None
            } else if block_number >= self.params.eip155_transition {
                Some(self.params.chain_id)
            } else {
                None
            };

        // Apply block rewards.
        let block_reward_tx_offset =
            ad_hoc_tx_numbers + block.transactions.len() as u32;
        let mut block_reward_txs = 0;
        let block_reward_base = self.get_block_reward_base(block_number);

        let block_reward = block_reward_base
            + block_reward_base.shr(5) * U256::from(block.uncles.len());
        self.add_tx_from_system(
            RealizedEthTx {
                sender: None,
                receiver: Some(block.header.author().clone()),
                tx_fee_wei: 0.into(),
                // The amount will be updated when the last unverified tx in the
                // block is finished.
                amount_wei: block_reward,
                types: EthTxType::BlockRewardAndTxFee,
            },
            block_reward_tx_offset + block_reward_txs,
        );
        block_reward_txs += 1;

        for uncle_header in &block.uncles {
            let uncle_reward = (block_reward_base
                * U256::from(8 + uncle_header.number() - block_number))
            .shr(3);
            self.add_tx_from_system(
                RealizedEthTx {
                    sender: None,
                    receiver: Some(uncle_header.author().clone()),
                    tx_fee_wei: 0.into(),
                    amount_wei: uncle_reward,
                    types: EthTxType::UncleReward,
                },
                block_reward_tx_offset + block_reward_txs,
            );
            block_reward_txs += 1;
        }

        let mut thread = self.n_tx_seen % Self::N_TX_BASIC_VERIFIERS;
        let mut basic_verification_index = self.n_tx_seen;
        self.n_tx_seen += block.transactions.len();
        for i in 0..block.transactions.len() {
            unsafe {
                let tx = block.transactions.get_unchecked(i);
                self.tx_verify(
                    check_low_s,
                    if !use_tx_chain_id {
                        chain_id
                    } else {
                        tx.chain_id()
                    },
                    false,
                    block.clone(),
                    self.base_transaction_number,
                    i,
                    thread,
                    basic_verification_index,
                );
            }
            thread = (thread + 1) % Self::N_TX_BASIC_VERIFIERS;
            basic_verification_index += 1;
        }

        self.n_blocks += 1;
        self.base_transaction_number = new_base_transaction_number;

        // Some progress log.
        if self.n_blocks % 5000 == 0 {
            println!(
                "Block {}, block number = {}, #tx seen {}, #accounts {}, #contract creation {}, \
                #valid txs + awards {}, #total tx rlp len {}, \
                #nonce error {}, #basic verification error {}",
                self.n_blocks,
                block.header.number(),
                self.n_tx_seen,
                self.n_accounts(),
                self.n_contract_creation(),
                self.n_txs(),
                self.total_verified_tx_rlp_length(),
                self.n_nonce_error(),
                self.n_tx_verification_error(),
            );
        }
    }
}

fn main() -> errors::Result<()> {
    env_logger::init();

    let matches = App::new("conflux storage benchmark")
        .arg(
            Arg::with_name("import_eth")
                .value_name("eth")
                .help("Ethereum blockchain file to import.")
                .takes_value(true)
                .last(true),
        )
        .arg(
            Arg::with_name("genesis")
                .value_name("genesis")
                .help("Ethereum genesis json config file.")
                .takes_value(true)
                .short("g")
                .long("genesis"),
        )
        .arg(
            Arg::with_name("txs")
                .value_name("transaction file")
                .help("File of verified transactions.")
                .short("t")
                .takes_value(true),
        )
        .get_matches_from(std::env::args().collect::<Vec<_>>());

    let tx_extractor = EthTxExtractor::new_from_spec(
        matches.value_of("genesis").unwrap(),
        matches.value_of("txs").unwrap(),
    )?;
    let tx_extractor_stopper = EthTxExtractorStopper(tx_extractor.clone());

    // Load block RLP from file.
    let mut rlp_file = File::open(matches.value_of("import_eth").unwrap())?;
    const BUFFER_SIZE: usize = 10000000;
    let mut buffer = Vec::<u8>::with_capacity(BUFFER_SIZE);

    'read: loop {
        let buffer_ptr = buffer.as_mut_ptr();
        let buffer_rest = unsafe {
            slice::from_raw_parts_mut(
                buffer_ptr.offset(buffer.len() as isize),
                buffer.capacity() - buffer.len(),
            )
        };
        info!(
            "buffer rest len {}, buffer len {}",
            buffer_rest.len(),
            buffer.len()
        );
        let read_result = rlp_file.read(buffer_rest);
        match read_result {
            Ok(bytes_read) => {
                // EOF
                if bytes_read == 0 {
                    info!("eof");
                    break 'read;
                }

                unsafe {
                    buffer.set_len(buffer.len() + bytes_read);
                }
                if buffer.len() == buffer.capacity() {
                    buffer.reserve_exact(buffer.capacity());
                }

                let mut to_parse = buffer.as_slice();
                'parse: loop {
                    // Try to parse rlp.
                    let payload_info_result = Rlp::new(to_parse).payload_info();
                    if payload_info_result.is_err() {
                        if *payload_info_result.as_ref().unwrap_err()
                            == DecoderError::RlpIsTooShort
                        {
                            let mut buffer_new =
                                Vec::<u8>::with_capacity(BUFFER_SIZE);
                            buffer_new.extend_from_slice(to_parse);
                            drop(to_parse);
                            buffer = buffer_new;
                            // Reset the buffer.
                            if buffer.len() == buffer.capacity() {
                                buffer.reserve_exact(buffer.capacity());
                            }
                            continue 'read;
                        }
                    }
                    let payload_info = payload_info_result?;

                    // Now the buffer has sufficient length for an Rlp.
                    let rlp_len = payload_info.total();
                    // Finally we have a block.
                    let block = Arc::new(
                        EthBlock::decode(&Rlp::new(&to_parse[0..rlp_len]))
                            .unwrap(),
                    );
                    to_parse = &to_parse[rlp_len..];

                    // FIXME: debug code.
                    let mut tx_extractor_locked = tx_extractor.lock();
                    if tx_extractor_locked.n_blocks() >= 2_000_000 {
                        break 'read;
                    }
                    tx_extractor_locked.add_block(block);
                }
            }
            Err(err) => {
                if err.kind() == io::ErrorKind::Interrupted
                    || err.kind() == io::ErrorKind::WouldBlock
                {
                    // Retry
                    continue;
                }
                eprintln!("{}", err);
                bail!(err);
            }
        }
    }
    Ok(())
}

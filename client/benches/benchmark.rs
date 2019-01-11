extern crate client;
extern crate parking_lot;
#[macro_use]
extern crate criterion;
extern crate ethcore_bytes;

use client::{Client, ClientHandle, Configuration};
use core::{
    executive::Executive,
    machine::{self, new_byzantium_test_machine},
    state::State,
    statedb::StateDb,
    storage::state_manager::StateManagerTrait,
    vm::{EnvInfo, Spec},
    vm_factory::VmFactory,
};
use criterion::Criterion;
use ethcore_bytes::Bytes;
use ethereum_types::{Address, H256, U256, U512};
use ethkey::{Generator, KeyPair, Random};
use parking_lot::{Condvar, Mutex};
use primitives::{Action, Transaction};
use std::sync::Arc;

fn txgen_benchmark(c: &mut Criterion) {
    let mut conf = Configuration::default();
    conf.raw_conf.test_mode = true;
    let exit = Arc::new((Mutex::new(false), Condvar::new()));
    let handler = Client::start(conf, exit.clone()).unwrap();
    c.bench_function("Randomly generate 1 transaction", move |b| {
        b.iter(|| {
            handler.txgen.generate_transaction();
        });
    });
}

fn txexe_benchmark(c: &mut Criterion) {
    let mut conf = Configuration::default();
    conf.raw_conf.test_mode = true;
    let exit = Arc::new((Mutex::new(false), Condvar::new()));
    let handler = Client::start(conf, exit.clone()).unwrap();
    let kp = KeyPair::from_secret(
        "46b9e861b63d3509c88b7817275a30d22d62c8cd8fa6486ddee35ef0d8e0495f"
            .parse()
            .unwrap(),
    )
    .unwrap();
    let addr = kp.address();
    let receiver_kp = Random.generate().expect("Fail to generate KeyPair.");

    let tx = Transaction {
        nonce: 0.into(),
        gas_price: U256::from(100u64),
        gas: U256::from(21000u64),
        value: 1.into(),
        action: Action::Call(receiver_kp.address()),
        data: Bytes::new(),
    };
    let tx = tx.sign(kp.secret());
    let machine = new_byzantium_test_machine();
    let env = EnvInfo {
        number: 0, // TODO: replace 0 with correct cardinal number
        author: Default::default(),
        timestamp: Default::default(),
        difficulty: Default::default(),
        gas_used: U256::zero(),
        gas_limit: tx.gas.clone(),
    };
    let spec = Spec::new_byzantium();
    c.bench_function("Execute 1 transaction", move |b| {
        let mut state = State::new(
            StateDb::new(
                handler
                    .txgen
                    .storage_manager
                    .get_state_at(handler.consensus.best_block_hash())
                    .unwrap(),
            ),
            0.into(),
            VmFactory::new(1024 * 32),
        );
        let mut ex = Executive::new(&mut state, &env, &machine, &spec);
        b.iter(|| {
            ex.transact(&tx);
            ex.state.clear();
        })
    });
}

criterion_group!(benches, txgen_benchmark, txexe_benchmark);
criterion_main!(benches);

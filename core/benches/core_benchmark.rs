#[macro_use]
extern crate criterion;
extern crate core;
extern crate ethereum_types;
extern crate ethkey;
extern crate primitives;
extern crate rand;
use ethereum_types::{H256, U256, U512};
use ethkey::{Public, Signature};
use primitives::{SignedTransaction, Transaction};
use rand::{prng::XorShiftRng, ChaChaRng, Rng, RngCore, SeedableRng};

use core::transaction_pool::TreapMap;
use criterion::Criterion;

fn get_rng_for_test() -> ChaChaRng { ChaChaRng::from_seed([123; 32]) }

fn next_u512(rng: &mut ChaChaRng) -> U512 {
    let mut result = U512::from(0);
    for _ in 0..8 {
        result = (result << 64) + (U512::from(rng.next_u64()));
    }
    result
}

fn next_u256(rng: &mut ChaChaRng) -> U256 {
    let mut result = U256::from(0);
    for _ in 0..4 {
        result = (result << 64) + (U512::from(rng.next_u64()));
    }
    result
}

fn next_signed_transaction(rng: &mut ChaChaRng) -> SignedTransaction {
    SignedTransaction::new(
        0.into(),
        Transaction {
            nonce: 0.into(),
            gas_price: next_u256(rng),
            gas: next_u256(rng),
            value: next_u256(rng),
            receiver: 0.into(),
        }
        .with_signature(Signature::default()),
    )
}

fn treap_map_benchmark(c: &mut Criterion) {
    c.bench_function("TreapMap randomly insert 100000 times", |b| {
        let mut treap_map: TreapMap<H256, SignedTransaction, U512> =
            TreapMap::new_with_rng(XorShiftRng::from_seed([123; 16]));

        let mut operation_rng = get_rng_for_test();
        let num_iter = 100000;
        for _ in 0..num_iter {
            let tx = next_signed_transaction(&mut operation_rng);
            b.iter(|| {
                treap_map.insert(
                    tx.hash(),
                    tx.clone(),
                    U512::from(tx.gas_price().clone()),
                )
            });
        }
    });

    c.bench_function("TreapMap randomly remove 100000 times", |b| {
        let mut treap_map: TreapMap<H256, SignedTransaction, U512> =
            TreapMap::new_with_rng(XorShiftRng::from_seed([123; 16]));

        let mut operation_rng = get_rng_for_test();
        let num_iter: u32 = 100000;
        let mut tx_vec: Vec<SignedTransaction> = vec![];
        for _ in 0..num_iter {
            let tx = next_signed_transaction(&mut operation_rng);
            treap_map.insert(
                tx.hash(),
                tx.clone(),
                U512::from(tx.gas_price().clone()),
            );
            tx_vec.push(tx);
        }

        operation_rng.shuffle(tx_vec.as_mut_slice());

        for oper_id in 0..num_iter {
            let tx = tx_vec.pop().unwrap();
            b.iter(|| {
                treap_map.remove(&tx.hash());
            });
        }
    });

    c.bench_function("TreapMap randomly get by weight 100000 times", |b| {
        let mut treap_map: TreapMap<H256, SignedTransaction, U512> =
            TreapMap::new_with_rng(XorShiftRng::from_seed([123; 16]));

        let mut operation_rng = get_rng_for_test();
        let num_iter: u32 = 100000;
        let mut tx_vec: Vec<SignedTransaction> = vec![];
        for _ in 0..num_iter {
            let tx = next_signed_transaction(&mut operation_rng);
            treap_map.insert(
                tx.hash(),
                tx.clone(),
                U512::from(tx.gas_price().clone()),
            );
            tx_vec.push(tx);
        }

        let sum_weight = treap_map.sum_weight();
        for oper_id in 0..num_iter {
            let rand_value = next_u512(&mut operation_rng) % sum_weight;
            b.iter(|| {
                treap_map.get_by_weight(rand_value);
            });
        }
    });

}

criterion_group!(benches, treap_map_benchmark);
criterion_main!(benches);

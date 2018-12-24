extern crate client;
extern crate parking_lot;
#[macro_use]
extern crate criterion;

use client::{Client, ClientHandle, Configuration};
use criterion::Criterion;
use parking_lot::{Condvar, Mutex};
use std::sync::Arc;

fn txgen_benchmark(c: &mut Criterion) {
    c.bench_function("Randomly generate 1 transaction", |b| {
        let mut conf = Configuration::default();
        conf.raw_conf.test_mode = true;
        let exit = Arc::new((Mutex::new(false), Condvar::new()));
        let handler = Client::start(conf, exit.clone()).unwrap();
        {
            let txgen = &handler.txgen;
            b.iter(|| {
                txgen.generate_transaction();
            });
        }
        Client::close(handler);
    });
}

criterion_group!(benches, txgen_benchmark);
criterion_main!(benches);

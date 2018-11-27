use super::node::Node;
use rand::{prng::XorShiftRng, FromEntropy, RngCore};
use std::{
    convert::From,
    ops::{Add, Sub},
};

pub struct TreapMap<K, V, W> {
    root: Option<Box<Node<K, V, W>>>,
    size: usize,
    rng: XorShiftRng,
}

impl<
        K: Ord,
        V,
        W: Add<Output = W> + Sub<Output = W> + Ord + Clone + From<u32>,
    > TreapMap<K, V, W>
{
    pub fn new() -> TreapMap<K, V, W> {
        TreapMap {
            root: None,
            size: 0,
            rng: XorShiftRng::from_entropy(),
        }
    }

    pub fn new_with_rng(rng: XorShiftRng) -> TreapMap<K, V, W> {
        TreapMap {
            root: None,
            size: 0,
            rng,
        }
    }

    pub fn len(&self) -> usize { self.size }

    pub fn is_empty(&self) -> bool { self.size == 0 }

    pub fn contains_key(&self, key: &K) -> bool { self.get(key).is_some() }

    pub fn insert(&mut self, key: K, value: V, weight: W) -> Option<V> {
        let result = Node::insert(
            &mut self.root,
            Node::new(key, value, weight, self.rng.next_u64()),
        );
        if result.is_none() {
            self.size += 1;
        }
        result
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        let result = Node::remove(&mut self.root, key);
        if result.is_some() {
            self.size -= 1;
        }
        result
    }

    pub fn sum_weight(&self) -> W {
        match &self.root {
            Some(node) => node.sum_weight(),
            None => 0.into(),
        }
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.root.as_ref().and_then(|x| x.get(key))
    }

    pub fn get_by_weight(&self, weight: W) -> Option<&V> {
        self.root.as_ref().and_then(|x| x.get_by_weight(weight))
    }
}

use super::node::Node;
use rand::{self, prng::XorShiftRng};
use std::ops::Add;

pub struct TreapMap<K, V, Rng = rand::XorShiftRng> {
    root: Option<Box<Node<K, V>>>,
    size: usize,
    rng: Rng,
}

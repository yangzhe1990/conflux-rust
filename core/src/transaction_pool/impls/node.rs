use std::ops::Add;

pub struct Node<K, V> {
    pub key: K,
    pub value: V,
    priority: u64,
    pub left: Option<Box<Node<K, V>>>,
    pub right: Option<Box<Node<K, V>>>,
}

impl<K: Ord, V: Add> Node<K, V> {}

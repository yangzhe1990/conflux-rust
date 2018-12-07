use std::{
    cmp::Ordering,
    mem,
    ops::{Add, Sub},
};

pub struct Node<K, V, W> {
    pub key: K,
    pub value: V,
    weight: W,
    sum_weight: W,
    priority: u64,
    pub left: Option<Box<Node<K, V, W>>>,
    pub right: Option<Box<Node<K, V, W>>>,
}

impl<K: Ord, V, W: Add<Output = W> + Sub<Output = W> + Ord + Clone>
    Node<K, V, W>
{
    pub fn new(key: K, value: V, weight: W, priority: u64) -> Node<K, V, W> {
        Node {
            key,
            value,
            sum_weight: weight.clone(),
            weight,
            priority,
            left: None,
            right: None,
        }
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        match key.cmp(&self.key) {
            Ordering::Equal => Some(&self.value),
            Ordering::Less => self.left.as_ref().and_then(|x| x.get(key)),
            Ordering::Greater => self.right.as_ref().and_then(|x| x.get(key)),
        }
    }

    pub fn insert(
        node: &mut Option<Box<Node<K, V, W>>>, new: Node<K, V, W>,
    ) -> Option<V> {
        if node.is_none() {
            mem::replace(node, Some(Box::new(new)));
            return None;
        }
        match new.key.cmp(&node.as_ref().unwrap().key) {
            Ordering::Equal => {
                Some(mem::replace(node, Some(Box::new(new))).unwrap().value)
            }
            Ordering::Less => {
                let result =
                    Node::insert(&mut node.as_mut().unwrap().left, new);
                if node.as_ref().unwrap().priority
                    < node.as_ref().unwrap().left.as_ref().unwrap().priority
                {
                    Node::right_rotate(node);
                }
                node.as_mut().unwrap().update_weight();
                result
            }
            Ordering::Greater => {
                let result =
                    Node::insert(&mut node.as_mut().unwrap().right, new);
                if node.as_ref().unwrap().priority
                    < node.as_ref().unwrap().right.as_ref().unwrap().priority
                {
                    Node::left_rotate(node);
                }
                node.as_mut().unwrap().update_weight();
                result
            }
        }
    }

    pub fn remove(node: &mut Option<Box<Node<K, V, W>>>, key: &K) -> Option<V> {
        if node.is_none() {
            return None;
        }
        let result = match key.cmp(&node.as_ref().unwrap().key) {
            Ordering::Equal => {
                if node.as_ref().unwrap().left.is_none()
                    && node.as_ref().unwrap().right.is_none()
                {
                    return Some(mem::replace(node, None).unwrap().value);
                }
                match node.as_ref().unwrap().left.is_none()
                    || node.as_ref().unwrap().left.is_some()
                        && node.as_ref().unwrap().right.is_some()
                        && &node
                            .as_ref()
                            .unwrap()
                            .left
                            .as_ref()
                            .unwrap()
                            .priority
                            < &node
                                .as_ref()
                                .unwrap()
                                .right
                                .as_ref()
                                .unwrap()
                                .priority
                {
                    true => {
                        // rot left
                        Node::left_rotate(node);
                        Node::remove(&mut node.as_mut().unwrap().left, key)
                    }
                    false => {
                        // rot right
                        Node::right_rotate(node);
                        Node::remove(&mut node.as_mut().unwrap().right, key)
                    }
                }
            }
            Ordering::Less => {
                Node::remove(&mut node.as_mut().unwrap().left, key)
            }
            Ordering::Greater => {
                Node::remove(&mut node.as_mut().unwrap().right, key)
            }
        };
        node.as_mut().unwrap().update_weight();
        result
    }

    pub fn get_by_weight(&self, weight: W) -> Option<&V> {
        let mut pre_weight = self.weight.clone();
        if self.left.is_some() {
            if &weight < &self.left.as_ref().unwrap().sum_weight {
                return self.left.as_ref().and_then(|x| x.get_by_weight(weight));
            } else {
                pre_weight =
                    pre_weight + self.left.as_ref().unwrap().sum_weight.clone();
            }
        }
        if &weight < &pre_weight {
            return Some(&self.value);
        }
        self.right
            .as_ref()
            .and_then(|x| x.get_by_weight(weight - pre_weight))
    }

    fn right_rotate(node: &mut Option<Box<Node<K, V, W>>>) {
        let mut new = mem::replace(&mut node.as_mut().unwrap().left, None);
        if new.is_some() {
            mem::swap(node, &mut new);
            mem::swap(
                &mut node.as_mut().unwrap().right,
                &mut new.as_mut().unwrap().left,
            );
            new.as_mut().unwrap().update_weight();
            mem::replace(&mut node.as_mut().unwrap().right, new);
            node.as_mut().unwrap().update_weight();
        }
    }

    fn left_rotate(node: &mut Option<Box<Node<K, V, W>>>) {
        let mut new = mem::replace(&mut node.as_mut().unwrap().right, None);
        if new.is_some() {
            mem::swap(node, &mut new);
            mem::swap(
                &mut node.as_mut().unwrap().left,
                &mut new.as_mut().unwrap().right,
            );
            new.as_mut().unwrap().update_weight();
            mem::replace(&mut node.as_mut().unwrap().left, new);
            node.as_mut().unwrap().update_weight();
        }
    }

    fn update_weight(&mut self) {
        self.sum_weight = self.weight.clone();
        if self.left.is_some() {
            self.sum_weight = self.sum_weight.clone()
                + self.left.as_ref().unwrap().sum_weight.clone();
        }
        if self.right.is_some() {
            self.sum_weight = self.sum_weight.clone()
                + self.right.as_ref().unwrap().sum_weight.clone();
        }
    }

    pub fn sum_weight(&self) -> W { self.sum_weight.clone() }
}
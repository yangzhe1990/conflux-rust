use ethereum_types::U256;

const NULL: usize = !0;

#[derive(Clone)]
struct Node {
    left_child: usize,
    right_child: usize,
    parent: usize,
    path_parent: usize,
    sum: U256,
    delta: U256,
}

impl Default for Node {
    fn default() -> Self {
        Node {
            left_child: NULL,
            right_child: NULL,
            parent: NULL,
            path_parent: NULL,
            sum: U256::zero(),
            delta: U256::zero(),
        }
    }
}

pub struct LinkCutTree {
    tree: Vec<Node>,
}

impl LinkCutTree {
    pub fn new() -> Self { LinkCutTree { tree: Vec::new() } }

    pub fn make_tree(&mut self, v: usize) {
        if self.tree.len() <= v {
            self.tree.resize(v + 1, Node::default());
        }
    }

    fn rotate(&mut self, v: usize) {
        if v == NULL {
            return;
        }
        if self.tree[v].parent == NULL {
            return;
        }

        let parent = self.tree[v].parent;
        let grandparent = self.tree[parent].parent;

        let sum =
            self.tree[v].sum + self.tree[v].delta + self.tree[parent].delta;
        self.tree[v].sum = sum;
        if self.tree[parent].left_child == v {
            let u = self.tree[v].right_child;
            let w = self.tree[v].left_child;
            self.tree[parent].left_child = u;
            if u != NULL {
                self.tree[u].parent = parent;
                let delta = self.tree[u].delta + self.tree[v].delta;
                self.tree[u].delta = delta;
            }
            if w != NULL {
                let delta = self.tree[w].delta
                    + self.tree[v].delta
                    + self.tree[parent].delta;
                self.tree[w].delta = delta;
            }
            self.tree[v].delta = U256::zero();
            self.tree[v].right_child = parent;
            self.tree[parent].parent = v;
        } else {
            let u = self.tree[v].left_child;
            let w = self.tree[v].right_child;
            self.tree[parent].right_child = u;
            if u != NULL {
                self.tree[u].parent = parent;
                let delta = self.tree[u].delta + self.tree[v].delta;
                self.tree[u].delta = delta;
            }
            if w != NULL {
                let delta = self.tree[w].delta
                    + self.tree[v].delta
                    + self.tree[parent].delta;
                self.tree[w].delta = delta;
            }
            self.tree[v].delta = U256::zero();
            self.tree[v].left_child = parent;
            self.tree[parent].parent = v;
        }
        self.tree[v].parent = grandparent;
        if grandparent != NULL {
            if self.tree[grandparent].left_child == parent {
                self.tree[grandparent].left_child = v;
            } else {
                self.tree[grandparent].right_child = v;
            }
        }
        self.tree[v].path_parent = self.tree[parent].path_parent;
        self.tree[parent].path_parent = NULL;
    }

    fn splay(&mut self, v: usize) {
        if v == NULL {
            return;
        }

        while self.tree[v].parent != NULL {
            let parent = self.tree[v].parent;
            let grandparent = self.tree[parent].parent;
            if grandparent == NULL {
                // zig
                self.rotate(v);
            } else if (self.tree[parent].left_child == v)
                == (self.tree[grandparent].left_child == parent)
            {
                // zig-zig
                self.rotate(parent);
                self.rotate(v);
            } else {
                // zig-zag
                self.rotate(v);
                self.rotate(v);
            }
        }
    }

    fn remove_preferred_child(&mut self, v: usize) {
        if v == NULL {
            return;
        }

        let u = self.tree[v].right_child;
        if u != NULL {
            self.tree[u].path_parent = v;
            self.tree[u].parent = NULL;
            self.tree[v].right_child = NULL;
        }
    }

    fn access(&mut self, v: usize) {
        if v == NULL {
            return;
        }

        self.splay(v);
        self.remove_preferred_child(v);

        while self.tree[v].path_parent != NULL {
            let w = self.tree[v].path_parent;
            self.splay(w);
            let u = self.tree[w].right_child;
            if u != NULL {
                self.tree[u].path_parent = w;
                self.tree[u].parent = NULL;
            }
            self.tree[w].right_child = v;
            self.tree[v].parent = w;
            self.splay(v);
        }
    }

    #[allow(dead_code)]
    fn debug(&self, num: usize) {
        for v in 0..num {
            println!("tree[{}]", v);
            println!("\tleft_child={}", self.tree[v].left_child as i64);
            println!("\tright_child={}", self.tree[v].right_child as i64);
            println!("\tparent={}", self.tree[v].parent as i64);
            println!("\tpath_parent={}", self.tree[v].path_parent as i64);
        }
    }

    /// Make w a new child of v
    pub fn link(&mut self, v: usize, w: usize) {
        if v == NULL || w == NULL {
            return;
        }

        self.access(w);
        self.tree[w].path_parent = v;
    }

    pub fn lca(&mut self, v: usize, w: usize) -> usize {
        self.access(v);

        self.splay(w);
        self.remove_preferred_child(w);

        let mut x = w;
        let mut y = w;
        while self.tree[y].path_parent != NULL {
            let z = self.tree[y].path_parent;
            self.splay(z);
            if self.tree[z].path_parent == NULL {
                x = z;
            }
            let u = self.tree[z].right_child;
            if u != NULL {
                self.tree[u].path_parent = z;
                self.tree[u].parent = NULL;
            }
            self.tree[z].right_child = y;
            self.tree[y].parent = z;
            self.tree[y].path_parent = NULL;
            y = z;
        }
        self.splay(w);

        x
    }

    pub fn update_weight(&mut self, v: usize, weight: &U256) {
        self.access(v);

        self.tree[v].sum += *weight;
        let u = self.tree[v].left_child;
        if u != NULL {
            self.tree[u].delta += *weight;
        }
    }

    pub fn aggregate_subtree_weight(&mut self, v: usize) -> U256 {
        self.access(v);
        self.tree[v].sum.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::{LinkCutTree, U256};

    #[test]
    fn test_lca() {
        let mut tree = LinkCutTree::new();

        /// 0
        /// |\
        /// 1 4
        /// |\
        /// 2 3
        tree.make_tree(0);
        tree.make_tree(1);
        tree.make_tree(2);
        tree.make_tree(3);
        tree.make_tree(4);
        tree.link(0, 1);
        tree.link(1, 2);
        tree.link(1, 3);
        tree.link(0, 4);

        assert_eq!(tree.lca(0, 1), 0);
        assert_eq!(tree.lca(2, 3), 1);
        assert_eq!(tree.lca(1, 4), 0);
        assert_eq!(tree.lca(1, 4), 0);
    }

    #[test]
    fn test_subtree_sum() {
        let mut tree = LinkCutTree::new();
        tree.make_tree(0);
        tree.make_tree(1);
        tree.make_tree(2);
        tree.make_tree(3);
        tree.make_tree(4);
        tree.link(0, 1);
        tree.link(1, 2);
        tree.link(1, 3);
        tree.link(0, 4);
        tree.update_weight(0, &U256::from(1u64));
        tree.update_weight(1, &U256::from(2u64));
        tree.update_weight(2, &U256::from(3u64));
        tree.update_weight(3, &U256::from(4u64));
        tree.update_weight(4, &U256::from(5u64));

        assert_eq!(tree.aggregate_subtree_weight(0), U256::from(15u64));
        assert_eq!(tree.aggregate_subtree_weight(1), U256::from(9u64));
        assert_eq!(tree.aggregate_subtree_weight(2), U256::from(3u64));
        assert_eq!(tree.aggregate_subtree_weight(3), U256::from(4u64));
        assert_eq!(tree.aggregate_subtree_weight(4), U256::from(5u64));
    }
}

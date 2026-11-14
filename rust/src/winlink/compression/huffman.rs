//! Frequency-ordered adaptive Huffman tree for LZHUF tokens.

const SYMBOLS: usize = 314;
const NODES: usize = SYMBOLS * 2 - 1;
const ROOT: usize = NODES - 1;
const REBUILD_AT: u16 = 0x8000;

pub(super) struct Tree {
    frequency: [u16; NODES + 1],
    parent: [usize; NODES + SYMBOLS],
    child: [usize; NODES],
}

impl Tree {
    pub(super) fn new() -> Self {
        let mut tree = Self {
            frequency: [1; NODES + 1],
            parent: [0; NODES + SYMBOLS],
            child: [0; NODES],
        };
        for symbol in 0..SYMBOLS {
            tree.set_child(symbol, NODES + symbol);
        }
        for node in SYMBOLS..NODES {
            let left = 2 * (node - SYMBOLS);
            tree.frequency[node] = tree.frequency[left] + tree.frequency[left + 1];
            tree.set_child(node, left);
        }
        tree.frequency[NODES] = u16::MAX;
        tree
    }

    pub(super) fn encode(&mut self, symbol: usize, mut bit: impl FnMut(u8)) {
        let mut path = [0; NODES];
        let mut len = 0;
        let mut node = self.parent[NODES + symbol];
        while node != ROOT {
            path[len] = (node & 1) as u8;
            len += 1;
            node = self.parent[node];
        }
        for &value in path[..len].iter().rev() {
            bit(value);
        }
        self.update(symbol);
    }

    pub(super) fn decode<E>(&mut self, mut bit: impl FnMut() -> Result<u8, E>) -> Result<usize, E> {
        let mut child = self.child[ROOT];
        while child < NODES {
            child = self.child[child + usize::from(bit()?)];
        }
        let symbol = child - NODES;
        self.update(symbol);
        Ok(symbol)
    }

    fn set_child(&mut self, node: usize, child: usize) {
        self.child[node] = child;
        self.parent[child] = node;
        if child < NODES {
            self.parent[child + 1] = node;
        }
    }

    fn update(&mut self, symbol: usize) {
        if self.frequency[ROOT] == REBUILD_AT {
            self.rebuild();
        }
        let mut node = self.parent[NODES + symbol];
        loop {
            let frequency = self.frequency[node] + 1;
            self.frequency[node] = frequency;
            if frequency > self.frequency[node + 1] {
                let mut next = node + 1;
                while frequency > self.frequency[next + 1] {
                    next += 1;
                }
                self.frequency.swap(node, next);
                let child = self.child[node];
                self.set_child(node, self.child[next]);
                self.set_child(next, child);
                node = next;
            }
            if node == ROOT {
                break;
            }
            node = self.parent[node];
        }
    }

    fn rebuild(&mut self) {
        // Keep leaves in frequency order, halving their weights before
        // combining the lightest pairs back into internal nodes.
        let mut leaf = 0;
        for node in 0..NODES {
            if self.child[node] >= NODES {
                self.frequency[leaf] = self.frequency[node].div_ceil(2);
                self.child[leaf] = self.child[node];
                leaf += 1;
            }
        }
        for node in SYMBOLS..NODES {
            let left = 2 * (node - SYMBOLS);
            let frequency = self.frequency[left] + self.frequency[left + 1];
            let at = self.frequency[..node].partition_point(|&weight| weight <= frequency);
            self.frequency.copy_within(at..node, at + 1);
            self.child.copy_within(at..node, at + 1);
            self.frequency[at] = frequency;
            self.child[at] = left;
        }
        for node in 0..NODES {
            self.set_child(node, self.child[node]);
        }
    }
}

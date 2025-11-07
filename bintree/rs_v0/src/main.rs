use rand::{rngs::StdRng, RngCore, SeedableRng};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::time::Instant;

type Bytes32 = [u8; 32];
type Stem = [u8; 31];

#[inline(always)]
fn sha256_32(data: &[u8]) -> Bytes32 {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let out = hasher.finalize();
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&out);
    arr
}

/// H(data) with the "zeros64 -> zeros32" rule so totally-empty internal nodes are 0x00..00.
#[inline(always)]
fn h32or64(data: &[u8]) -> Bytes32 {
    if data.len() == 64 && data.iter().all(|&b| b == 0) {
        return [0u8; 32];
    }
    sha256_32(data)
}

/// MSB-first bit over the 31-byte stem.
#[inline(always)]
fn stem_bit(stem: &Stem, depth: usize) -> u8 {
    let byte = stem[depth / 8];
    let bit_in_byte = 7 - (depth % 8);
    (byte >> bit_in_byte) & 1
}

/// Hash a concatenation of two 32-byte nodes without allocating.
#[inline(always)]
fn h_pair(left: &Bytes32, right: &Bytes32) -> Bytes32 {
    let mut buf = [0u8; 64];
    buf[..32].copy_from_slice(left);
    buf[32..].copy_from_slice(right);
    h32or64(&buf)
}

/// Compute a 256-leaf subtree root *sparsely* from a small set of present leaves.
/// `leaves` maps subindex (0..255) -> 32B value. We apply leaf-hash = H(value) internally.
fn subtree_root_sparse(leaves: &HashMap<u8, Bytes32>) -> Bytes32 {
    if leaves.is_empty() {
        return [0u8; 32];
    }
    // Level nodes hold (index_at_level, hash). Start with level-0 leaf hashes for present leaves only.
    let mut level_nodes: Vec<(u16, Bytes32)> = Vec::with_capacity(leaves.len());
    for (&idx, val) in leaves.iter() {
        level_nodes.push((idx as u16, sha256_32(val)));
    }

    // Fold up 8 levels. When a sibling is missing, we combine with 0x00..00 (the default).
    for _level in 0..8 {
        level_nodes.sort_unstable_by_key(|(i, _)| *i);
        let mut next: Vec<(u16, Bytes32)> = Vec::with_capacity((level_nodes.len() + 1) / 2);
        let mut i = 0;
        while i < level_nodes.len() {
            let (idx, h) = level_nodes[i];
            if i + 1 < level_nodes.len() && level_nodes[i + 1].0 == (idx ^ 1) {
                // Real sibling present
                let h2 = level_nodes[i + 1].1;
                let (left, right) = if (idx & 1) == 0 { (h, h2) } else { (h2, h) };
                next.push((idx >> 1, h_pair(&left, &right)));
                i += 2;
            } else {
                // Missing sibling -> combine with default zero at this level
                let zero = [0u8; 32];
                let (left, right) = if (idx & 1) == 0 { (h, zero) } else { (zero, h) };
                next.push((idx >> 1, h_pair(&left, &right)));
                i += 1;
            }
        }
        level_nodes = next;
    }
    debug_assert_eq!(level_nodes.len(), 1);
    level_nodes[0].1
}

/// Compute sibling hash at a given level `lvl` for a target index `sub` from the same sparse set.
/// lvl=0 => sibling leaf; lvl=7 => sibling 128-leaf subtree.
fn sibling_at_level_sparse(leaves: &HashMap<u8, Bytes32>, sub: u8, lvl: usize) -> Bytes32 {
    // Sibling subtree = all leaves j with (j >> lvl) == ((sub >> lvl) ^ 1)
    let sib_parent = ((sub as u16) >> lvl) ^ 1;
    let mask: u16 = (1u16 << lvl) - 1;
    let mut nodes: Vec<(u16, Bytes32)> = Vec::new(); // (position within the sibling subtree at this level, leaf-hash)

    for (&j, v) in leaves.iter() {
        let j16 = j as u16;
        if (j16 >> lvl) == sib_parent {
            let pos = j16 & mask;
            nodes.push((pos, sha256_32(v)));
        }
    }
    if nodes.is_empty() {
        return [0u8; 32];
    }
    // Fold only `lvl` levels to get the sibling subtree root.
    for _ in 0..lvl {
        nodes.sort_unstable_by_key(|(i, _)| *i);
        let mut next: Vec<(u16, Bytes32)> = Vec::with_capacity((nodes.len() + 1) / 2);
        let mut i = 0;
        while i < nodes.len() {
            let (idx, h) = nodes[i];
            if i + 1 < nodes.len() && nodes[i + 1].0 == (idx ^ 1) {
                let h2 = nodes[i + 1].1;
                let (left, right) = if (idx & 1) == 0 { (h, h2) } else { (h2, h) };
                next.push((idx >> 1, h_pair(&left, &right)));
                i += 2;
            } else {
                let zero = [0u8; 32];
                let (left, right) = if (idx & 1) == 0 { (h, zero) } else { (zero, h) };
                next.push((idx >> 1, h_pair(&left, &right)));
                i += 1;
            }
        }
        nodes = next;
    }
    debug_assert_eq!(nodes.len(), 1);
    nodes[0].1
}

/// H(stem || 0x00 || stem_subtree_root)
#[inline(always)]
fn stem_node_hash(stem: &Stem, subtree_root: &Bytes32) -> Bytes32 {
    let mut buf = [0u8; 64];
    buf[..31].copy_from_slice(stem);
    buf[31] = 0x00;
    buf[32..].copy_from_slice(subtree_root);
    h32or64(&buf)
}

#[derive(Clone)]
struct StemBucket {
    stem: Stem,
    /// Present leaves only: subindex -> value (32B). (We hash `value` when merkelizing.)
    leaves: HashMap<u8, Bytes32>,
    subtree_root: Bytes32, // 256-leaf subtree root
    stem_hash: Bytes32,    // H(stem || 0x00 || subtree_root)
}

impl StemBucket {
    fn new(stem: Stem) -> Self {
        let leaves = HashMap::new();
        let subtree_root = subtree_root_sparse(&leaves);
        let stem_hash = stem_node_hash(&stem, &subtree_root);
        Self { stem, leaves, subtree_root, stem_hash }
    }
    #[inline(always)]
    fn recompute_hashes(&mut self) {
        self.subtree_root = subtree_root_sparse(&self.leaves);
        self.stem_hash = stem_node_hash(&self.stem, &self.subtree_root);
    }
}

#[derive(Clone)]
enum Node {
    Internal { hash: Bytes32, left: Box<Node>, right: Box<Node> },
    StemLeaf { hash: Bytes32, stem: Stem },
    Empty, // hash = 0x00..00
}
impl Node {
    #[inline(always)]
    fn hash(&self) -> Bytes32 {
        match self {
            Node::Empty => [0u8; 32],
            Node::StemLeaf { hash, .. } => *hash,
            Node::Internal { hash, .. } => *hash,
        }
    }
}

/// Build the minimal binary tree over stems (recursively split by MSB-first stem bits).
fn build_stem_tree(mut stems: Vec<StemBucket>, depth: usize) -> Node {
    if stems.is_empty() {
        return Node::Empty;
    }
    if stems.len() == 1 {
        return Node::StemLeaf {
            hash: stems[0].stem_hash,
            stem: stems[0].stem,
        };
    }
    let mut lefts = Vec::new();
    let mut rights = Vec::new();
    for sb in stems.drain(..) {
        if stem_bit(&sb.stem, depth) == 0 { lefts.push(sb); } else { rights.push(sb); }
    }
    let left = Box::new(build_stem_tree(lefts, depth + 1));
    let right = Box::new(build_stem_tree(rights, depth + 1));
    let hash = h_pair(&left.hash(), &right.hash());
    Node::Internal { hash, left, right }
}

/// Produce proof pieces:
///  - 8 siblings inside the 256-leaf subtree (bottom-up, LSB-first),
///  - siblings along the stem path to root (MSB-first).
fn prove_paths_sparse(
    root: &Node,
    stem: &Stem,
    subindex: u8,
    leaves_for_stem: &HashMap<u8, Bytes32>,
) -> ([Bytes32; 8], Vec<Bytes32>) {
    // 1) Gather the 8 stem-subtree siblings sparsely
    let mut stem_sibs: [Bytes32; 8] = [[0u8; 32]; 8];
    for lvl in 0..8 {
        stem_sibs[lvl] = sibling_at_level_sparse(leaves_for_stem, subindex, lvl);
    }

    // 2) Path-to-root siblings using stem bits (unchanged)
    let mut path_sibs = Vec::new();
    let mut cur = root;
    let mut depth = 0usize;
    loop {
        match cur {
            Node::Internal { left, right, .. } => {
                let b = stem_bit(stem, depth);
                depth += 1;
                if b == 0 {
                    path_sibs.push(right.hash());
                    cur = left;
                } else {
                    path_sibs.push(left.hash());
                    cur = right;
                }
            }
            Node::StemLeaf { stem: s, .. } => {
                assert_eq!(s, stem, "Stem not found in stem tree");
                break;
            }
            Node::Empty => panic!("Empty subtree encountered while proving"),
        }
    }
    (stem_sibs, path_sibs)
}

/// Updatable binary-state tree (SHA-256 merkelization) with sparse 256-leaf stems.
struct BinaryStateTree {
    stems: HashMap<Stem, StemBucket>,
    root: Node,
}
impl BinaryStateTree {
    fn new() -> Self {
        Self { stems: HashMap::new(), root: Node::Empty }
    }

    fn from_entries(entries: &[(Bytes32, Bytes32)]) -> Self {
        let mut bst = Self::new();
        bst.insert_many(entries);
        bst
    }

    /// Insert/overwrite many K/V pairs (keys are 32B; first 31B = stem, last = subindex).
    /// Recomputes only the *touched* stems, then rebuilds the stem tree root once.
    fn insert_many(&mut self, entries: &[(Bytes32, Bytes32)]) {
        let mut touched: HashSet<Stem> = HashSet::new();

        for (k, v) in entries {
            let mut stem = [0u8; 31];
            stem.copy_from_slice(&k[..31]);
            let sub = k[31];

            let sb = self.stems.entry(stem).or_insert_with(|| StemBucket::new(stem));
            sb.leaves.insert(sub, *v); // overwrite if present
            touched.insert(stem);
        }

        for stem in touched {
            if let Some(sb) = self.stems.get_mut(&stem) {
                sb.recompute_hashes();
            }
        }

        // Rebuild stem tree root from current stems (O(#stems) hashes).
        let vec = self.stems.values().cloned().collect::<Vec<_>>();
        self.root = build_stem_tree(vec, 0);
    }

    fn state_root(&self) -> Bytes32 {
        self.root.hash()
    }

    /// Return proof for an existing key in the current state.
    fn prove_for_key(&self, key: &Bytes32) -> Option<(Bytes32, [Bytes32; 8], Vec<Bytes32>)> {
        let mut stem = [0u8; 31];
        stem.copy_from_slice(&key[..31]);
        let sub = key[31];

        let sb = self.stems.get(&stem)?;
        let value = *sb.leaves.get(&sub)?; // absent => no proof
        let (sibs256, path) = prove_paths_sparse(&self.root, &stem, sub, &sb.leaves);
        Some((value, sibs256, path))
    }

    fn insert_many_incremental(&mut self, entries: &[(Bytes32, Bytes32)]) {
        use std::collections::HashSet;
        let mut touched: HashSet<Stem> = HashSet::new();

        // 1) Update buckets sparsely (same as before)
        for (k, v) in entries {
            let mut stem = [0u8; 31];
            stem.copy_from_slice(&k[..31]);
            let sub = k[31];
            let sb = self.stems.entry(stem).or_insert_with(|| StemBucket {
                stem,
                leaves: HashMap::new(),
                subtree_root: [0u8; 32],
                stem_hash: [0u8; 32],
            });
            sb.leaves.insert(sub, *v);   // overwrite if present
            touched.insert(stem);
        }

        // 2) Recompute *only* the touched stem hashes and upsert them into the tree
        for stem in touched {
            let sb = self.stems.get_mut(&stem).unwrap();
            sb.subtree_root = subtree_root_sparse(&sb.leaves);        // ≤ 8 hashes per new/changed leaf
            sb.stem_hash    = stem_node_hash(&sb.stem, &sb.subtree_root);
            upsert_stem(&mut self.root, sb.stem, sb.stem_hash, 0);    // ≈ log2(#stems) rehashes
        }
    }
}

/// Build the minimal subtree that contains *both* stems, starting at `depth`.
/// This only creates as many Internal nodes as the two stems share common prefix
/// from `depth` to the first differing bit.
fn merge_two_to_subtree(
    a_stem: Stem, a_hash: Bytes32,
    b_stem: Stem, b_hash: Bytes32,
    depth: usize
) -> Node {
    if depth >= 248 {
        // identical stems (pathological) – last write wins
        return Node::StemLeaf { stem: a_stem, hash: b_hash };
    }
    let a = stem_bit(&a_stem, depth);
    let b = stem_bit(&b_stem, depth);
    if a != b {
        // Diverge here: one internal with two leaves
        let (left, right) = if a == 0 {
            (Node::StemLeaf { stem: a_stem, hash: a_hash },
             Node::StemLeaf { stem: b_stem, hash: b_hash })
        } else {
            (Node::StemLeaf { stem: b_stem, hash: b_hash },
             Node::StemLeaf { stem: a_stem, hash: a_hash })
        };
        let lh = left.hash();
        let rh = right.hash();
        Node::Internal { hash: h_pair(&lh, &rh), left: Box::new(left), right: Box::new(right) }
    } else {
        // Same bit – build below and wrap once at this level with the child on that side
        let child = merge_two_to_subtree(a_stem, a_hash, b_stem, b_hash, depth + 1);
        if a == 0 {
            let left = Box::new(child);
            let right = Box::new(Node::Empty);
            let h = h_pair(&left.hash(), &right.hash());
            Node::Internal { hash: h, left, right }
        } else {
            let left = Box::new(Node::Empty);
            let right = Box::new(child);
            let h = h_pair(&left.hash(), &right.hash());
            Node::Internal { hash: h, left, right }
        }
    }
}

/// Upsert a `stem` with its `stem_hash` into the existing stem tree *in place*.
/// Rehashes only the path that changes.
fn upsert_stem(root: &mut Node, stem: Stem, stem_hash: Bytes32, depth: usize) {
    match root {
        Node::Empty => {
            *root = Node::StemLeaf { stem, hash: stem_hash };
        }
        Node::StemLeaf { stem: s, hash: h } => {
            if *s == stem {
                *h = stem_hash; // overwrite existing stem
            } else {
                // Replace this leaf with the minimal subtree holding both stems
                let old_stem = *s;
                let old_hash = *h;
                *root = merge_two_to_subtree(old_stem, old_hash, stem, stem_hash, depth);
            }
        }
        Node::Internal { left, right, hash } => {
            let b = stem_bit(&stem, depth);
            if b == 0 {
                upsert_stem(left, stem, stem_hash, depth + 1);
            } else {
                upsert_stem(right, stem, stem_hash, depth + 1);
            }
            *hash = h_pair(&left.hash(), &right.hash());
        }
    }
}

fn main() {
    let start = Instant::now();

    use hex::encode as hex;
    let mut rng = StdRng::seed_from_u64(0xE1F5_7864);

    // -------- 1) Build initial tree with 10,000 random pairs
    let mut initial = Vec::with_capacity(10_000_000);
    for _ in 0..10_000_000 {
        let mut rkey = [0u8; 32];
        let mut rval = [0u8; 32];
        rng.fill_bytes(&mut rkey);
        rng.fill_bytes(&mut rval);
        let key = sha256_32(&rkey);
        let val = sha256_32(&rval);
        initial.push((key, val));
    }
    println!("1 {}", start.elapsed().as_millis());

    let t0 = Instant::now();
    let mut tree = BinaryStateTree::from_entries(&initial);
    let build_ms = t0.elapsed().as_millis();
    let root_before = tree.state_root();
    println!("STATE ROOT (before, 10k): 0x{}", hex(root_before));
    println!("Initial build: {} ms", build_ms);

    // Show 3 proofs from the initial state
    for i in 0..3 {
        let (k, _) = &initial[(i * 1234 + 567) % initial.len()];
        if let Some((v, sibs256, path)) = tree.prove_for_key(k) {
            println!("\n=== INITIAL PROOF {} ===", i + 1);
            println!("key:          0x{}", hex(k));
            println!("value:        0x{}", hex(v));
            println!("subindex:     {}", k[31] as usize);
            println!("stem sibs (8):");
            for (j, s) in sibs256.iter().enumerate() {
                println!("  [{}] 0x{}", j, hex(s));
            }
            println!("path sibs ({}):", path.len());
            for (j, s) in path.iter().enumerate() {
                println!("  [{}] 0x{}", j, hex(s));
            }
        }
    }

    // -------- 2) UPDATE: add another 10,000 pairs
    let mut added = Vec::with_capacity(1_000);
    for _ in 0..1_000 {
        let mut rkey = [0u8; 32];
        let mut rval = [0u8; 32];
        rng.fill_bytes(&mut rkey);
        rng.fill_bytes(&mut rval);
        let key = sha256_32(&rkey);
        let val = sha256_32(&rval);
        added.push((key, val));
    }

    let t1 = Instant::now();
    tree.insert_many_incremental(&added);
    let update_ms = t1.elapsed().as_millis();
    let root_after = tree.state_root();
    println!("\nSTATE ROOT (after update, 20k total): 0x{}", hex(root_after));
    println!("Update(+10k) time: {} ms", update_ms);

    // Show 3 proofs from the *newly inserted* set
    for i in 0..3 {
        let (k, _) = &added[(i * 101 + 7) % added.len()];
        if let Some((v, sibs256, path)) = tree.prove_for_key(k) {
            println!("\n=== UPDATED PROOF {} ===", i + 1);
            println!("key:          0x{}", hex(k));
            println!("value:        0x{}", hex(v));
            println!("subindex:     {}", k[31] as usize);
            println!("stem sibs (8):");
            for (j, s) in sibs256.iter().enumerate() {
                println!("  [{}] 0x{}", j, hex(s));
            }
            println!("path sibs ({}):", path.len());
            for (j, s) in path.iter().enumerate() {
                println!("  [{}] 0x{}", j, hex(s));
            }
        }
    }
}

use crate::crypto::hash::hash_pair;

pub fn compute_merkle_root(leaves: Vec<String>) -> String {
    if leaves.is_empty() {
        return crate::crypto::hash::hash_str("empty");
    }

    let mut layer = leaves;

    while layer.len() > 1 {
        if layer.len() % 2 != 0 {
            let last = layer.last().unwrap().clone();
            layer.push(last);
        }

        let mut next_layer: Vec<String> = Vec::new();
        let mut i = 0;
        while i < layer.len() {
            let parent = hash_pair(&layer[i], &layer[i + 1]);
            next_layer.push(parent);
            i += 2;
        }
        layer = next_layer;
    }

    layer.into_iter().next().unwrap_or_default()
}

pub fn build_merkle_tree(leaves: Vec<String>) -> Vec<Vec<String>> {
    if leaves.is_empty() {
        return vec![];
    }

    let mut tree: Vec<Vec<String>> = vec![leaves.clone()];
    let mut layer = leaves;

    while layer.len() > 1 {
        if layer.len() % 2 != 0 {
            let last = layer.last().unwrap().clone();
            layer.push(last);
        }
        let mut next_layer: Vec<String> = Vec::new();
        let mut i = 0;
        while i < layer.len() {
            let parent = hash_pair(&layer[i], &layer[i + 1]);
            next_layer.push(parent);
            i += 2;
        }
        layer = next_layer.clone();
        tree.push(next_layer);
    }

    tree
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::hash::hash_str;

    #[test]
    fn test_single_leaf() {
        let h = hash_str("a");
        let root = compute_merkle_root(vec![h.clone()]);
        assert_eq!(root, h);
    }

    #[test]
    fn test_two_leaves() {
        let h1 = hash_str("a");
        let h2 = hash_str("b");
        let root = compute_merkle_root(vec![h1.clone(), h2.clone()]);
        assert_eq!(root, hash_pair(&h1, &h2));
    }

    #[test]
    fn test_deterministic() {
        let leaves: Vec<String> = vec!["x", "y", "z"]
            .iter()
            .map(|s| hash_str(s))
            .collect();
        let r1 = compute_merkle_root(leaves.clone());
        let r2 = compute_merkle_root(leaves);
        assert_eq!(r1, r2);
    }

    #[test]
    fn test_order_matters() {
        let h1 = hash_str("a");
        let h2 = hash_str("b");
        let r1 = compute_merkle_root(vec![h1.clone(), h2.clone()]);
        let r2 = compute_merkle_root(vec![h2, h1]);
        assert_ne!(r1, r2);
    }
}

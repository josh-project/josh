use super::paths::{dst_path, src_path};
use crate::filter::Filter;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

#[derive(Default)]
struct PathTrie {
    children: HashMap<std::ffi::OsString, PathTrie>,
    indices: Vec<usize>,
}

impl PathTrie {
    fn insert(&mut self, path: &std::path::Path, index: usize) {
        let mut node = self;
        for comp in path.components() {
            let key = comp.as_os_str().to_owned();
            node = node.children.entry(key).or_default();
        }
        node.indices.push(index);
    }

    fn find_overlapping(&self, path: &std::path::Path) -> Vec<usize> {
        let mut result = Vec::new();
        let mut node = self;

        result.extend(&node.indices);
        for comp in path.components() {
            match node.children.get(comp.as_os_str()) {
                Some(child) => {
                    node = child;
                    result.extend(&node.indices);
                }
                None => return result,
            }
        }

        node.collect_descendants(&mut result);
        result
    }

    fn collect_descendants(&self, result: &mut Vec<usize>) {
        for child in self.children.values() {
            result.extend(&child.indices);
            child.collect_descendants(result);
        }
    }
}

type PrefixSortEdges = Vec<smallvec::SmallVec<[usize; 32]>>;

pub(super) fn prefix_sort(filters: &[Filter]) -> Vec<Filter> {
    if filters.len() <= 1 {
        return filters.to_vec();
    }

    let n = filters.len();
    let mut outgoing: PrefixSortEdges = vec![Default::default(); n];

    let mut src_trie = PathTrie::default();
    let mut dst_trie = PathTrie::default();

    let mut maybe_push_outgoing = |j: usize, i: usize| {
        // `i` only can increase in the loop below,
        // so it's enough to just check the last element
        if Some(i) != outgoing[j].last().cloned() {
            outgoing[j].push(i);
        }
    };

    for (i, filter) in filters.iter().enumerate() {
        let src = src_path(*filter);
        let dst = dst_path(*filter);

        for j in src_trie.find_overlapping(&src) {
            maybe_push_outgoing(j, i);
        }

        for j in dst_trie.find_overlapping(&dst) {
            maybe_push_outgoing(j, i);
        }

        src_trie.insert(&src, i);
        dst_trie.insert(&dst, i);
    }

    topo_sort_with_tiebreak(&outgoing, filters)
}

fn topo_sort_with_tiebreak(outgoing: &PrefixSortEdges, filters: &[Filter]) -> Vec<Filter> {
    let mut indegree: Vec<usize> = vec![0; filters.len()];
    for neighbors in outgoing {
        for &j in neighbors {
            indegree[j] += 1;
        }
    }

    // Use a BinaryHeap with a wrapper for custom ordering
    #[derive(Eq, PartialEq)]
    struct SortKey(usize, std::path::PathBuf, std::path::PathBuf); // (index, src, dst)

    impl Ord for SortKey {
        fn cmp(&self, other: &Self) -> Ordering {
            match other.1.cmp(&self.1) {
                Ordering::Equal => other.2.cmp(&self.2),
                ord => ord,
            }
        }
    }

    impl PartialOrd for SortKey {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }

    let make_key = |i: usize| -> SortKey { SortKey(i, src_path(filters[i]), dst_path(filters[i])) };

    let mut heap: BinaryHeap<SortKey> = indegree
        .iter()
        .enumerate()
        .filter(|(_, deg)| **deg == 0)
        .map(|(i, _)| make_key(i))
        .collect();

    let mut result = Vec::with_capacity(filters.len());

    while let Some(SortKey(i, _, _)) = heap.pop() {
        result.push(filters[i]);

        for &j in outgoing[i].iter() {
            indegree[j] -= 1;
            if indegree[j] == 0 {
                heap.push(make_key(j));
            }
        }
    }

    result
}

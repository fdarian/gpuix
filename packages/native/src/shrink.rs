//! Gives memory back after a bulk removal. A hash table keeps the capacity of its largest moment,
//! and an entry of a few hundred bytes makes that large: a page of thousands of elements, left
//! for a small one, would hold its high-water mark for the life of the process.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasher, Hash};

/// Tables with fewer slots than this are never shrunk, so a small page does not churn.
const FLOOR: usize = 256;
/// A table shrinks once its capacity is more than this many times its length.
const SPARSE: usize = 4;
/// And to this many times its length.
const KEEP: usize = 2;

pub(crate) trait Shrink {
    fn len(&self) -> usize;
    fn capacity(&self) -> usize;
    fn shrink_to(&mut self, capacity: usize);
}

impl<K: Eq + Hash, V, S: BuildHasher> Shrink for HashMap<K, V, S> {
    fn len(&self) -> usize {
        HashMap::len(self)
    }

    fn capacity(&self) -> usize {
        HashMap::capacity(self)
    }

    fn shrink_to(&mut self, capacity: usize) {
        HashMap::shrink_to(self, capacity)
    }
}

impl<T: Eq + Hash, S: BuildHasher> Shrink for HashSet<T, S> {
    fn len(&self) -> usize {
        HashSet::len(self)
    }

    fn capacity(&self) -> usize {
        HashSet::capacity(self)
    }

    fn shrink_to(&mut self, capacity: usize) {
        HashSet::shrink_to(self, capacity)
    }
}

impl<T> Shrink for Vec<T> {
    fn len(&self) -> usize {
        Vec::len(self)
    }

    fn capacity(&self) -> usize {
        Vec::capacity(self)
    }

    fn shrink_to(&mut self, capacity: usize) {
        Vec::shrink_to(self, capacity)
    }
}

/// Reallocates `table` to twice its length when it holds more than four times that and is above
/// the floor. O(1) when it does nothing.
pub(crate) fn shrink_if_sparse(table: &mut impl Shrink) {
    let capacity = table.capacity();
    if capacity > FLOOR && capacity > table.len() * SPARSE {
        table.shrink_to(table.len() * KEEP);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use rustc_hash::{FxHashMap, FxHashSet};

    const BIG: u64 = 5_000;

    #[test]
    fn a_map_that_lost_most_of_its_entries_gives_the_slots_back() {
        let mut map: FxHashMap<u64, [u8; 64]> = FxHashMap::default();
        for id in 0..BIG {
            map.insert(id, [0; 64]);
        }
        let grown = map.capacity();
        map.retain(|id, _| id % 100 == 0);
        assert_eq!(map.capacity(), grown, "removing frees no slots by itself");
        shrink_if_sparse(&mut map);
        assert!(map.capacity() < grown / 10, "capacity {} of {grown}", map.capacity());
        assert_eq!(map.len(), (BIG / 100) as usize);
        assert!((0..BIG).step_by(100).all(|id| map.contains_key(&id)));
    }

    #[test]
    fn a_set_that_lost_most_of_its_entries_gives_the_slots_back() {
        let mut set: FxHashSet<u64> = (0..BIG).collect();
        let grown = set.capacity();
        set.retain(|id| id % 100 == 0);
        shrink_if_sparse(&mut set);
        assert!(set.capacity() < grown / 10);
        assert!((0..BIG).step_by(100).all(|id| set.contains(&id)));
    }

    #[test]
    fn a_table_that_keeps_its_size_is_not_reallocated() {
        let mut map: HashMap<u64, u64> = (0..BIG).map(|id| (id, id)).collect();
        map.retain(|id, _| id % 2 == 0);
        let capacity = map.capacity();
        for _ in 0..3 {
            shrink_if_sparse(&mut map);
        }
        assert_eq!(map.capacity(), capacity);
    }

    #[test]
    fn a_small_table_is_never_shrunk() {
        let mut set: HashSet<u64> = (0..200).collect();
        assert!(set.capacity() <= FLOOR);
        set.retain(|id| *id == 0);
        let capacity = set.capacity();
        shrink_if_sparse(&mut set);
        assert_eq!(set.capacity(), capacity);
    }

    #[test]
    fn a_table_emptied_by_a_bulk_removal_is_freed() {
        let mut map: HashMap<u64, u64> = (0..BIG).map(|id| (id, id)).collect();
        map.clear();
        shrink_if_sparse(&mut map);
        assert_eq!(map.capacity(), 0);
    }

    #[test]
    fn a_vec_shrinks_like_a_table() {
        let mut list: Vec<u64> = (0..BIG).collect();
        list.truncate(10);
        shrink_if_sparse(&mut list);
        assert!(list.capacity() < 100);
        assert_eq!(list.len(), 10);
    }
}

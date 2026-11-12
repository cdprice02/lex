//! The memo behind the Endgame and the Rollout: a value per Candidate set, by the guesses the
//! position allows.

use std::sync::Arc;

use dashmap::DashMap;

use crate::allowed::Allowed;

/// Values by candidate set, kept apart by allowed-guess list, because the same set costs more when
/// fewer guesses are permitted. Only an [`Allowed::List`] (hard mode) varies, so it keys an inner
/// map by the list itself; `Whole` and `Candidates` are fixed by the mode and the set and share
/// one plain map. Both levels look up by a borrowed `&[u32]`, so a lookup allocates nothing, and
/// every method takes `&self`, so threads can share one memo.
#[derive(Debug)]
pub struct Memo<V> {
    /// Values for every allowed set that is implied by the mode, by candidate set.
    plain: DashMap<Vec<u32>, V>,
    /// Values by allowed list, then by candidate set.
    by_list: DashMap<Vec<u32>, Arc<DashMap<Vec<u32>, V>>>,
}

impl<V> Default for Memo<V> {
    fn default() -> Self {
        Self {
            plain: DashMap::new(),
            by_list: DashMap::new(),
        }
    }
}

impl<V: Copy> Memo<V> {
    /// The value stored for `set` under `allowed`, if any.
    pub(crate) fn get(&self, allowed: &Allowed, set: &[u32]) -> Option<V> {
        match allowed {
            Allowed::List(list) => self
                .by_list
                .get(list.as_slice())
                .and_then(|inner| inner.get(set).map(|v| *v)),
            Allowed::Whole | Allowed::Candidates => self.plain.get(set).map(|v| *v),
        }
    }

    /// Stores `value` for `set` under `allowed`, keeping an earlier value for the same key.
    pub(crate) fn insert(&self, allowed: &Allowed, set: &[u32], value: V) {
        match allowed {
            Allowed::List(list) => {
                // the inner map is cloned out so no shard guard is held across the insert
                let existing = self.by_list.get(list.as_slice()).map(|r| Arc::clone(&r));
                let inner = existing
                    .unwrap_or_else(|| Arc::clone(&self.by_list.entry(list.clone()).or_default()));
                inner.entry(set.to_vec()).or_insert(value);
            }
            Allowed::Whole | Allowed::Candidates => {
                self.plain.entry(set.to_vec()).or_insert(value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stored_value_is_read_back_under_the_same_allowed() {
        let memo = Memo::default();
        let list = Allowed::List(vec![1, 2, 3]);
        memo.insert(&list, &[1, 2], 4.5);
        memo.insert(&Allowed::Whole, &[1, 2], 2.5);
        assert_eq!(memo.get(&list, &[1, 2]), Some(4.5));
        assert_eq!(memo.get(&Allowed::Whole, &[1, 2]), Some(2.5));
    }

    #[test]
    fn different_allowed_lists_do_not_share_values() {
        let memo = Memo::default();
        memo.insert(&Allowed::List(vec![1, 2]), &[1], 1.0);
        assert_eq!(memo.get(&Allowed::List(vec![1, 2, 3]), &[1]), None);
        assert_eq!(memo.get(&Allowed::List(vec![1, 2]), &[2]), None);
        assert_eq!(memo.get(&Allowed::Whole, &[1]), None);
    }

    #[test]
    fn whole_and_candidates_share_the_plain_map() {
        let memo = Memo::default();
        memo.insert(&Allowed::Candidates, &[5, 6], 3_u32);
        assert_eq!(memo.get(&Allowed::Candidates, &[5, 6]), Some(3));
        assert_eq!(memo.get(&Allowed::Whole, &[5, 6]), Some(3));
    }

    #[test]
    fn the_first_value_for_a_key_is_kept() {
        let memo = Memo::default();
        let list = Allowed::List(vec![9]);
        memo.insert(&list, &[9], 1_u32);
        memo.insert(&list, &[9], 2_u32);
        assert_eq!(memo.get(&list, &[9]), Some(1));
    }

    #[test]
    fn threads_fill_one_memo() {
        let memo = Memo::default();
        let list = Allowed::List(vec![0, 1, 2, 3]);
        std::thread::scope(|s| {
            for _ in 0..4 {
                s.spawn(|| {
                    for i in 0..100_u32 {
                        memo.insert(&list, &[i], i);
                    }
                });
            }
        });
        for i in 0..100_u32 {
            assert_eq!(memo.get(&list, &[i]), Some(i));
        }
    }
}

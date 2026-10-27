/// A partition of the guess pool into classes of words that split every candidate set the same
/// way. Two words in one group give identical feedback against every candidate, so their entropy
/// is bit-identical and the lowest pool index (the group's representative) is the one the
/// solver's tie-break would pick; only representatives need scoring.
///
/// Groups only coarsen as the candidate set shrinks (words that agree on a set agree on every
/// subset), so a child position inherits its parent's groups. The root starts from singletons,
/// stored implicitly so hard mode and the root cost nothing.
#[derive(Debug, Clone, Default)]
pub(crate) struct Groups {
    merged: Option<Merged>,
}

/// Flat member list: group `g` is `members[offsets[g]..offsets[g + 1]]`, representative first,
/// groups in ascending representative order.
#[derive(Debug, Clone)]
struct Merged {
    members: Vec<u32>,
    offsets: Vec<u32>,
}

impl Groups {
    /// Every pool word in a group of its own.
    pub(crate) fn singletons() -> Self {
        Self { merged: None }
    }

    pub(crate) fn len(&self, pool_len: usize) -> usize {
        self.merged
            .as_ref()
            .map_or(pool_len, |m| m.offsets.len() - 1)
    }

    /// Group `g` as `(representative, the other members)`.
    pub(crate) fn group(&self, g: usize) -> (u32, &[u32]) {
        match &self.merged {
            None => (g as u32, &[]),
            Some(m) => {
                let (start, end) = (m.offsets[g] as usize, m.offsets[g + 1] as usize);
                (m.members[start], &m.members[start + 1..end])
            }
        }
    }

    /// Groups in ascending representative order.
    pub(crate) fn iter(&self, pool_len: usize) -> impl Iterator<Item = (u32, &[u32])> + '_ {
        (0..self.len(pool_len)).map(|g| self.group(g))
    }

    /// These groups with each `(kept, absorbed)` pair of group indices (`kept < absorbed`) joined
    /// into the kept group, which keeps the lower representative.
    pub(crate) fn coarsened(&self, pool_len: usize, merges: &mut [(usize, usize)]) -> Self {
        // each kept group takes its absorbed groups in ascending order
        merges.sort_unstable();
        let count = self.len(pool_len);
        let mut absorbed = vec![false; count];
        for &(kept, gone) in merges.iter() {
            debug_assert!(kept < gone);
            absorbed[gone] = true;
        }
        let mut members = Vec::with_capacity(pool_len);
        let mut offsets = Vec::with_capacity(count - merges.len() + 1);
        let mut pending = merges.iter().peekable();
        for g in (0..count).filter(|&g| !absorbed[g]) {
            offsets.push(members.len() as u32);
            let (rep, extra) = self.group(g);
            members.push(rep);
            members.extend_from_slice(extra);
            while let Some(&(_, gone)) = pending.next_if(|&&(kept, _)| kept == g) {
                let (rep, extra) = self.group(gone);
                members.push(rep);
                members.extend_from_slice(extra);
            }
        }
        offsets.push(members.len() as u32);
        Self {
            merged: Some(Merged { members, offsets }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flatten(groups: &Groups, pool_len: usize) -> Vec<(u32, Vec<u32>)> {
        groups
            .iter(pool_len)
            .map(|(r, e)| (r, e.to_vec()))
            .collect()
    }

    #[test]
    fn singletons_are_one_group_per_word() {
        let g = Groups::singletons();
        assert_eq!(g.len(4), 4);
        assert_eq!(g.group(2), (2, &[][..]));
    }

    #[test]
    fn coarsening_joins_groups_under_the_lower_representative() {
        let merged = Groups::singletons().coarsened(6, &mut [(1, 4), (1, 2), (0, 5)]);
        assert_eq!(
            flatten(&merged, 6),
            [(0, vec![5]), (1, vec![2, 4]), (3, vec![])]
        );
        // coarsening again works from the merged form
        let again = merged.coarsened(6, &mut [(0, 2)]);
        assert_eq!(flatten(&again, 6), [(0, vec![5, 3]), (1, vec![2, 4])]);
    }
}

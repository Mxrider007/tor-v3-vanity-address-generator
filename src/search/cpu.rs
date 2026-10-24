//! CPU worker.
//!
//! Keys are enumerated in symmetric groups (see [`crate::curve::walk`]),
//! costing about 5.5 field multiplications per key. Only the y coordinate is
//! needed to test a start-anchored pattern, so x and the checksum are computed
//! just for the rare candidates that pass the bit-mask filter.

use std::sync::atomic::Ordering;

use crate::curve::point::ExtendedPoint;
use crate::curve::walk::{Center, GroupWalker, group_offset};
use crate::onion::{address_chars, pubkey_chars};
use crate::pattern::{MatchResult, PrefixFilter};

use super::{Shared, Submitted, Walk};

/// Half-size of a key group: each group covers `2 * GROUP + 1` keys and costs
/// one field inversion. 256 keeps the per-thread working set (~80 KiB) in L2
/// cache; larger groups gain only a few percent.
pub(crate) const GROUP: usize = 256;

/// Keys walked from one random start before re-seeding (roughly a minute of
/// work for one core).
const WALK_KEYS: u64 = 1 << 30;

/// The patterns a worker is still looking for, refreshed when a pattern
/// completes.
struct Active {
    generation: u64,
    wanted: Vec<bool>,
    filters: Vec<PrefixFilter>,
}

impl Active {
    fn new(shared: &Shared) -> Active {
        let mut active = Active {
            generation: u64::MAX,
            wanted: Vec::new(),
            filters: Vec::new(),
        };
        active.refresh(shared);
        active
    }

    fn refresh(&mut self, shared: &Shared) {
        let generation = shared.generation.load(Ordering::Acquire);
        if generation == self.generation {
            return;
        }
        self.generation = generation;
        self.wanted = (0..shared.filters.len()).map(|i| shared.wants(i)).collect();
        self.filters = shared
            .filters
            .iter()
            .zip(&self.wanted)
            .filter(|(_, wanted)| **wanted)
            .map(|(f, _)| *f)
            .collect();
    }
}

pub(super) fn run(shared: &Shared) {
    let anywhere = shared.matcher.anywhere();
    let group = shared.table.group() as u64;
    let keys_per_group = shared.table.keys_per_group() as u64;
    let mut active = Active::new(shared);

    'walk: while !shared.should_stop() {
        let walk = match Walk::random() {
            Ok(walk) => walk,
            Err(message) => return shared.fail(message),
        };
        // The first group is centered on key number GROUP of the walk, so the
        // walk covers keys 0, 1, 2, … without gaps.
        let mut center = group;
        let start = ExtendedPoint::mul_base(&walk.scalar(center));
        let mut walker = GroupWalker::new(&shared.table, Center::from_extended(&start));

        while center + group < WALK_KEYS {
            if shared.should_stop() {
                return;
            }
            active.refresh(shared);
            walker.step();

            for (index, y) in walker.ys.iter().enumerate() {
                if anywhere {
                    // Most keys are ruled out by y alone, before x is computed.
                    if !shared
                        .matcher
                        .may_match_anywhere(&pubkey_chars(y), false, |i| active.wanted[i])
                    {
                        continue;
                    }
                } else if !active.filters.iter().any(|f| f.test(y)) {
                    continue;
                }
                let pubkey = walker.pubkey(index);
                if anywhere
                    && !shared
                        .matcher
                        .may_match_anywhere(&pubkey_chars(&pubkey), true, |i| active.wanted[i])
                {
                    continue;
                }
                match shared
                    .matcher
                    .check(&address_chars(&pubkey), |i| shared.wants(i))
                {
                    MatchResult::Match(_) => {}
                    MatchResult::MissingContains => {
                        shared.add_near_miss();
                        continue;
                    }
                    MatchResult::NoMatch => continue,
                }

                let key = center
                    .checked_add_signed(group_offset(index))
                    .expect("in range");
                match shared.submit(&walk.scalar(key), walk.start.hash_prefix()) {
                    Submitted::Rejected => {}
                    Submitted::Saved => {
                        shared.add_keys(index as u64 + 1);
                        continue 'walk;
                    }
                    Submitted::Stop => return,
                }
            }
            shared.add_keys(keys_per_group);
            center += keys_per_group;
        }
    }
}

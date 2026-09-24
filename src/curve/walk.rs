//! Enumerating consecutive keys in symmetric groups.
//!
//! Keys are visited in groups of `2G + 1` around an affine "center" point
//! `P = c·B`: the group contains `P` and `P ± Q_i` for the precomputed points
//! `Q_i = i·(8B)`, `i = 1..=G`, i.e. the keys with scalars `c + 8j` for
//! `j = -G..=G`. On the twisted Edwards curve `-x² + y² = 1 + d·x²·y²`:
//!
//! ```text
//! y(P + Q) = (y1·y2 + x1·x2) / (1 - k)      x(P + Q) = (x1·y2 + y1·x2) / (1 + k)
//! y(P - Q) = (y1·y2 - x1·x2) / (1 + k)      x(P - Q) = (x1·y2 - y1·x2) / (1 - k)
//! where k = d·x1·x2·y1·y2 = (x1·y1)·(d·x2·y2)
//! ```
//!
//! so each ± pair shares three multiplications, and all the denominators of
//! a group (plus the two needed to move to the next center) are inverted
//! together with a single field inversion. A start-anchored pattern only
//! needs y, so the whole group costs about 5.5 multiplications per key; x is
//! computed only for candidates. (This is the Edwards-curve version of the
//! grouping trick popularised by VanitySearch.)

use super::field::{Fe, FeLoose, batch_invert};
use super::point::{ExtendedPoint, compress_affine, curve_d};

/// An affine point with the precomputed product `e = d·x·y`.
#[derive(Clone, Copy)]
pub(crate) struct TableEntry {
    pub(crate) x: Fe,
    pub(crate) y: Fe,
    pub(crate) e: Fe,
}

/// `Q_i = i·8B` for `i = 1..=G` and the step `R = (2G + 1)·8B` from one
/// center to the next.
pub(crate) struct WalkTable {
    pub(crate) entries: Vec<TableEntry>,
    pub(crate) advance: TableEntry,
}

impl WalkTable {
    pub(crate) fn new(group: usize) -> WalkTable {
        assert!(group > 0);
        let step = ExtendedPoint::eight_basepoint_niels();
        let mut points = Vec::with_capacity(2 * group + 1);
        let mut p = ExtendedPoint::basepoint().double().double().double();
        for _ in 0..2 * group + 1 {
            points.push(p);
            p = p.add_niels(&step);
        }
        // points[i - 1] = i·8B
        let mut zs: Vec<Fe> = points.iter().map(|p| p.z).collect();
        let mut scratch = vec![Fe::ZERO; zs.len()];
        batch_invert(&mut zs, &mut scratch);
        let d = curve_d();
        let entry = |i: usize| {
            let x = points[i].x.mul(zs[i]);
            let y = points[i].y.mul(zs[i]);
            TableEntry {
                x,
                y,
                e: x.mul(y).mul(d),
            }
        };
        WalkTable {
            entries: (0..group).map(entry).collect(),
            advance: entry(2 * group),
        }
    }

    /// `G`.
    pub(crate) fn group(&self) -> usize {
        self.entries.len()
    }

    /// Keys per group, `2G + 1`.
    pub(crate) fn keys_per_group(&self) -> usize {
        2 * self.entries.len() + 1
    }
}

/// An affine point with `t = x·y`.
#[derive(Clone, Copy)]
pub(crate) struct Center {
    pub(crate) x: Fe,
    pub(crate) y: Fe,
    pub(crate) t: Fe,
}

impl Center {
    pub(crate) fn from_extended(p: &ExtendedPoint) -> Center {
        let zinv = p.z.invert();
        Center::from_affine(p.x.mul(zinv), p.y.mul(zinv))
    }

    pub(crate) fn from_affine(x: Fe, y: Fe) -> Center {
        Center { x, y, t: x.mul(y) }
    }
}

/// Position of key `index` of a group relative to its center, in steps of 8B:
/// index 0 is the center, `2i - 1` is `+i` and `2i` is `-i`.
pub(crate) fn group_offset(index: usize) -> i64 {
    if index == 0 {
        0
    } else {
        let i = index.div_ceil(2) as i64;
        if index % 2 == 1 { i } else { -i }
    }
}

/// Walks groups of keys, producing the canonical encoding of every key's y
/// coordinate (the sign bit of x is not included).
pub(crate) struct GroupWalker<'t> {
    table: &'t WalkTable,
    /// Center of the next group.
    center: Center,
    /// Center of the group whose results are in `ys`.
    current: Center,
    /// Denominators, inverted in place: `[1 - k_1, 1 + k_1, …, 1 - k_R, 1 + k_R]`.
    den: Vec<Fe>,
    num: Vec<FeLoose>,
    scratch: Vec<Fe>,
    /// `y` of each key of the last group, indexed as in [`group_offset`].
    pub(crate) ys: Vec<[u8; 32]>,
}

impl<'t> GroupWalker<'t> {
    pub(crate) fn new(table: &'t WalkTable, first_center: Center) -> GroupWalker<'t> {
        let g = table.group();
        GroupWalker {
            table,
            center: first_center,
            current: first_center,
            den: vec![Fe::ZERO; 2 * g + 2],
            num: vec![Fe::ZERO.relax(); 2 * g],
            scratch: vec![Fe::ZERO; 2 * g + 2],
            ys: vec![[0; 32]; 2 * g + 1],
        }
    }

    /// Process the group around the current center and move to the next one.
    pub(crate) fn step(&mut self) {
        let c = self.center;
        let g = self.table.group();
        for (i, q) in self.table.entries.iter().enumerate() {
            let xx = c.x.mul(q.x);
            let yy = c.y.mul(q.y);
            let k = c.t.mul(q.e);
            self.den[2 * i] = Fe::ONE.sub(k).carry();
            self.den[2 * i + 1] = Fe::ONE.add(k).carry();
            self.num[2 * i] = yy.add(xx);
            self.num[2 * i + 1] = yy.sub(xx);
        }
        let r = &self.table.advance;
        let k = c.t.mul(r.e);
        self.den[2 * g] = Fe::ONE.sub(k).carry();
        self.den[2 * g + 1] = Fe::ONE.add(k).carry();

        batch_invert(&mut self.den, &mut self.scratch);

        self.ys[0] = c.y.to_bytes();
        for j in 0..2 * g {
            self.ys[j + 1] = self.num[j].mul(self.den[j].relax()).to_bytes();
        }

        let y = c.y.mul(r.y).add(c.x.mul(r.x)).mul(self.den[2 * g].relax());
        let x =
            c.x.mul(r.y)
                .add(c.y.mul(r.x))
                .mul(self.den[2 * g + 1].relax());
        self.current = c;
        self.center = Center::from_affine(x, y);
    }

    /// Affine x and y (canonical bytes) of the next group's center.
    #[cfg(test)]
    pub(crate) fn next_center_bytes(&self) -> ([u8; 32], [u8; 32]) {
        (self.center.x.to_bytes(), self.center.y.to_bytes())
    }

    /// The full compressed public key of key `index` of the last group.
    pub(crate) fn pubkey(&self, index: usize) -> [u8; 32] {
        let c = self.current;
        if index == 0 {
            return compress_affine(c.x, c.y);
        }
        let i = (index - 1) / 2;
        let q = &self.table.entries[i];
        let (a, b) = (c.x.mul(q.y), c.y.mul(q.x));
        let x = if index % 2 == 1 {
            a.add(b).mul(self.den[2 * i + 1].relax())
        } else {
            a.sub(b).mul(self.den[2 * i].relax())
        };
        let mut bytes = self.ys[index];
        bytes[31] |= u8::from(x.is_negative()) << 7;
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use curve25519_dalek::{EdwardsPoint, Scalar};

    fn scalar_plus(base: &[u8; 32], steps: i64) -> Scalar {
        let s = Scalar::from_bytes_mod_order(*base);
        let delta = Scalar::from(8 * steps.unsigned_abs());
        if steps >= 0 { s + delta } else { s - delta }
    }

    #[test]
    fn offsets() {
        let offsets: Vec<i64> = (0..7).map(group_offset).collect();
        assert_eq!(offsets, [0, 1, -1, 2, -2, 3, -3]);
    }

    #[test]
    fn groups_match_dalek() {
        for g in [1, 3, 16] {
            let table = WalkTable::new(g);
            let mut base = [0u8; 32];
            getrandom::fill(&mut base).unwrap();
            base[31] &= 0x3f;
            let start = ExtendedPoint::mul_base(&base);
            let mut walker = GroupWalker::new(&table, Center::from_extended(&start));
            let mut center = 0i64;
            for _ in 0..3 {
                walker.step();
                for index in 0..table.keys_per_group() {
                    let expected =
                        EdwardsPoint::mul_base(&scalar_plus(&base, center + group_offset(index)))
                            .compress()
                            .to_bytes();
                    assert_eq!(walker.pubkey(index), expected, "g={g} index={index}");
                    let mut y = expected;
                    y[31] &= 0x7f;
                    assert_eq!(walker.ys[index], y);
                }
                center += table.keys_per_group() as i64;
            }
        }
    }
}

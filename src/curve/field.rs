//! Arithmetic in GF(2^255 - 19).
//!
//! This is a thin, allocation-free wrapper around the formally verified field
//! arithmetic from [`fiat_crypto`]. Values come in two flavours, mirroring
//! fiat's bounds discipline:
//!
//! * [`Fe`] — "tight" limbs, the output of every multiplication/carry and the
//!   only valid input for addition, subtraction and serialization.
//! * [`FeLoose`] — "loose" limbs, the output of addition/subtraction. They may
//!   be fed straight into a multiplication (which is where they are used) but
//!   must be carried back to [`Fe`] before any further addition.
//!
//! Keeping the two apart in the type system means an out-of-bounds limb can
//! never reach a fiat routine whose correctness proof assumes tighter bounds.

use fiat_crypto::curve25519_64::{
    fiat_25519_add, fiat_25519_carry, fiat_25519_carry_mul, fiat_25519_carry_square,
    fiat_25519_from_bytes, fiat_25519_loose_field_element as Loose, fiat_25519_relax,
    fiat_25519_selectznz, fiat_25519_sub, fiat_25519_tight_field_element as Tight,
    fiat_25519_to_bytes,
};

/// A field element with tight limb bounds.
#[derive(Clone, Copy)]
pub(crate) struct Fe(Tight);

/// A field element with loose limb bounds (result of an addition/subtraction).
#[derive(Clone, Copy)]
pub(crate) struct FeLoose(Loose);

impl Fe {
    pub(crate) const ZERO: Fe = Fe(Tight([0; 5]));
    pub(crate) const ONE: Fe = Fe(Tight([1, 0, 0, 0, 0]));

    /// Decode a little-endian field element. The top bit is ignored, exactly
    /// like Ed25519 point decoding does.
    pub(crate) fn from_bytes(bytes: &[u8; 32]) -> Fe {
        let mut b = *bytes;
        b[31] &= 0x7f;
        let mut out = Tight([0; 5]);
        fiat_25519_from_bytes(&mut out, &b);
        Fe(out)
    }

    /// Canonical (fully reduced) little-endian encoding.
    pub(crate) fn to_bytes(self) -> [u8; 32] {
        let mut out = [0u8; 32];
        fiat_25519_to_bytes(&mut out, &self.0);
        out
    }

    pub(crate) fn relax(self) -> FeLoose {
        let mut out = Loose([0; 5]);
        fiat_25519_relax(&mut out, &self.0);
        FeLoose(out)
    }

    pub(crate) fn add(self, rhs: Fe) -> FeLoose {
        let mut out = Loose([0; 5]);
        fiat_25519_add(&mut out, &self.0, &rhs.0);
        FeLoose(out)
    }

    pub(crate) fn sub(self, rhs: Fe) -> FeLoose {
        let mut out = Loose([0; 5]);
        fiat_25519_sub(&mut out, &self.0, &rhs.0);
        FeLoose(out)
    }

    pub(crate) fn mul(self, rhs: Fe) -> Fe {
        self.relax().mul(rhs.relax())
    }

    pub(crate) fn square(self) -> Fe {
        self.relax().square()
    }

    /// `self^(2^k)`.
    fn pow2k(self, k: u32) -> Fe {
        let mut r = self;
        for _ in 0..k {
            r = r.square();
        }
        r
    }

    /// Multiplicative inverse (`self^(p-2)`); the inverse of zero is zero.
    ///
    /// Uses the standard addition chain from the Ed25519 reference code
    /// (254 squarings, 11 multiplications). Runs in constant time.
    pub(crate) fn invert(self) -> Fe {
        let z2 = self.square(); // 2
        let z9 = z2.pow2k(2).mul(self); // 9
        let z11 = z9.mul(z2); // 11
        let z2_5_0 = z11.square().mul(z9); // 2^5 - 2^0
        let z2_10_0 = z2_5_0.pow2k(5).mul(z2_5_0); // 2^10 - 2^0
        let z2_20_0 = z2_10_0.pow2k(10).mul(z2_10_0); // 2^20 - 2^0
        let z2_40_0 = z2_20_0.pow2k(20).mul(z2_20_0); // 2^40 - 2^0
        let z2_50_0 = z2_40_0.pow2k(10).mul(z2_10_0); // 2^50 - 2^0
        let z2_100_0 = z2_50_0.pow2k(50).mul(z2_50_0); // 2^100 - 2^0
        let z2_200_0 = z2_100_0.pow2k(100).mul(z2_100_0); // 2^200 - 2^0
        let z2_250_0 = z2_200_0.pow2k(50).mul(z2_50_0); // 2^250 - 2^0
        z2_250_0.pow2k(5).mul(z11) // 2^255 - 21 = p - 2
    }

    /// Whether the canonical encoding is odd (the Ed25519 "sign" of x).
    pub(crate) fn is_negative(self) -> bool {
        self.to_bytes()[0] & 1 == 1
    }

    /// Constant-time select: `b` if `choice == 1`, otherwise `a`.
    #[cfg(test)]
    pub(crate) fn select(a: Fe, b: Fe, choice: u8) -> Fe {
        let mut out = [0u64; 5];
        fiat_25519_selectznz(&mut out, choice & 1, &a.0.0, &b.0.0);
        Fe(Tight(out))
    }

    #[cfg(test)]
    pub(crate) fn ct_eq(self, rhs: Fe) -> bool {
        self.to_bytes() == rhs.to_bytes()
    }
}

impl FeLoose {
    pub(crate) fn mul(self, rhs: FeLoose) -> Fe {
        let mut out = Tight([0; 5]);
        fiat_25519_carry_mul(&mut out, &self.0, &rhs.0);
        Fe(out)
    }

    pub(crate) fn square(self) -> Fe {
        let mut out = Tight([0; 5]);
        fiat_25519_carry_square(&mut out, &self.0);
        Fe(out)
    }

    pub(crate) fn carry(self) -> Fe {
        let mut out = Tight([0; 5]);
        fiat_25519_carry(&mut out, &self.0);
        Fe(out)
    }

    /// Constant-time select: `b` if `choice == 1`, otherwise `a`.
    pub(crate) fn select(a: FeLoose, b: FeLoose, choice: u8) -> FeLoose {
        let mut out = [0u64; 5];
        fiat_25519_selectznz(&mut out, choice & 1, &a.0.0, &b.0.0);
        FeLoose(Loose(out))
    }
}

/// Invert every element of `zs` in place using Montgomery's trick: one field
/// inversion plus three multiplications per element. `scratch` must be at
/// least as long as `zs`. No element may be zero (projective Z coordinates of
/// valid points never are).
pub(crate) fn batch_invert(zs: &mut [Fe], scratch: &mut [Fe]) {
    let scratch = &mut scratch[..zs.len()];
    let mut acc = Fe::ONE;
    for (z, s) in zs.iter().zip(scratch.iter_mut()) {
        *s = acc;
        acc = acc.mul(*z);
    }
    let mut inv = acc.invert();
    for (z, s) in zs.iter_mut().zip(scratch.iter()).rev() {
        let next = inv.mul(*z);
        *z = inv.mul(*s);
        inv = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P_MINUS_1: [u8; 32] = {
        let mut b = [0xffu8; 32];
        b[0] = 0xec;
        b[31] = 0x7f;
        b
    };

    fn fe(n: u64) -> Fe {
        let mut b = [0u8; 32];
        b[..8].copy_from_slice(&n.to_le_bytes());
        Fe::from_bytes(&b)
    }

    fn random_fe() -> Fe {
        let mut b = [0u8; 32];
        getrandom::fill(&mut b).unwrap();
        Fe::from_bytes(&b)
    }

    #[test]
    fn canonical_encoding_round_trips() {
        for _ in 0..100 {
            let a = random_fe();
            assert!(Fe::from_bytes(&a.to_bytes()).ct_eq(a));
        }
        // p - 1 is canonical; p itself reduces to zero.
        assert_eq!(Fe::from_bytes(&P_MINUS_1).to_bytes(), P_MINUS_1);
        let mut p = P_MINUS_1;
        p[0] = 0xed;
        assert_eq!(Fe::from_bytes(&p).to_bytes(), [0u8; 32]);
    }

    #[test]
    fn arithmetic_identities() {
        for _ in 0..100 {
            let (a, b, c) = (random_fe(), random_fe(), random_fe());
            // Distributivity and commutativity across the loose/tight boundary.
            let lhs = a.add(b).mul(c.relax());
            let rhs = a.mul(c).add(b.mul(c)).carry();
            assert!(lhs.ct_eq(rhs));
            assert!(a.mul(b).ct_eq(b.mul(a)));
            assert!(a.square().ct_eq(a.mul(a)));
            assert!(a.sub(b).carry().add(b).carry().ct_eq(a));
        }
        // -1 * -1 = 1
        let minus_one = Fe::from_bytes(&P_MINUS_1);
        assert!(minus_one.square().ct_eq(Fe::ONE));
        assert!(fe(7).mul(fe(6)).ct_eq(fe(42)));
    }

    #[test]
    fn inversion() {
        for _ in 0..50 {
            let a = random_fe();
            assert!(a.mul(a.invert()).ct_eq(Fe::ONE));
        }
        assert!(Fe::ZERO.invert().ct_eq(Fe::ZERO));
    }

    #[test]
    fn batch_inversion_matches_single() {
        let mut zs: Vec<Fe> = (0..37).map(|_| random_fe()).collect();
        let expected: Vec<Fe> = zs.iter().map(|z| z.invert()).collect();
        let mut scratch = vec![Fe::ZERO; 64];
        batch_invert(&mut zs, &mut scratch);
        for (z, e) in zs.iter().zip(&expected) {
            assert!(z.ct_eq(*e));
        }
    }

    #[test]
    fn select_is_correct() {
        let (a, b) = (fe(1), fe(2));
        assert!(Fe::select(a, b, 0).ct_eq(a));
        assert!(Fe::select(a, b, 1).ct_eq(b));
    }
}

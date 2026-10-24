//! Twisted Edwards (Ed25519) point arithmetic used by the search loop.
//!
//! The formulas are the same ones curve25519-dalek uses (extended
//! coordinates, "Niels" precomputed addends) and are cross-checked against
//! curve25519-dalek in the tests below. Only what the vanity search needs is
//! implemented: doubling, mixed addition with a precomputed affine point,
//! constant-time base-point multiplication and (batch) compression.

use super::field::{Fe, FeLoose};

/// d = -121665/121666
const D: [u8; 32] = [
    0xa3, 0x78, 0x59, 0x13, 0xca, 0x4d, 0xeb, 0x75, 0xab, 0xd8, 0x41, 0x41, 0x4d, 0x0a, 0x70, 0x00,
    0x98, 0xe8, 0x79, 0x77, 0x79, 0x40, 0xc7, 0x8c, 0x73, 0xfe, 0x6f, 0x2b, 0xee, 0x6c, 0x03, 0x52,
];
/// 2d
const D2: [u8; 32] = [
    0x59, 0xf1, 0xb2, 0x26, 0x94, 0x9b, 0xd6, 0xeb, 0x56, 0xb1, 0x83, 0x82, 0x9a, 0x14, 0xe0, 0x00,
    0x30, 0xd1, 0xf3, 0xee, 0xf2, 0x80, 0x8e, 0x19, 0xe7, 0xfc, 0xdf, 0x56, 0xdc, 0xd9, 0x06, 0x24,
];
/// Base point x coordinate.
const BASE_X: [u8; 32] = [
    0x1a, 0xd5, 0x25, 0x8f, 0x60, 0x2d, 0x56, 0xc9, 0xb2, 0xa7, 0x25, 0x95, 0x60, 0xc7, 0x2c, 0x69,
    0x5c, 0xdc, 0xd6, 0xfd, 0x31, 0xe2, 0xa4, 0xc0, 0xfe, 0x53, 0x6e, 0xcd, 0xd3, 0x36, 0x69, 0x21,
];
/// Base point y coordinate (4/5).
const BASE_Y: [u8; 32] = [
    0x58, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
    0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
];

/// A point in extended coordinates: x = X/Z, y = Y/Z, T = XY/Z.
#[derive(Clone, Copy)]
pub(crate) struct ExtendedPoint {
    pub(crate) x: Fe,
    pub(crate) y: Fe,
    pub(crate) z: Fe,
    pub(crate) t: Fe,
}

/// A precomputed affine addend: (y + x, y - x, 2dxy).
#[derive(Clone, Copy)]
pub(crate) struct AffineNielsPoint {
    y_plus_x: FeLoose,
    y_minus_x: FeLoose,
    xy2d: FeLoose,
}

impl AffineNielsPoint {
    fn identity() -> Self {
        Self {
            y_plus_x: Fe::ONE.relax(),
            y_minus_x: Fe::ONE.relax(),
            xy2d: Fe::ZERO.relax(),
        }
    }

    fn select(a: &Self, b: &Self, choice: u8) -> Self {
        Self {
            y_plus_x: FeLoose::select(a.y_plus_x, b.y_plus_x, choice),
            y_minus_x: FeLoose::select(a.y_minus_x, b.y_minus_x, choice),
            xy2d: FeLoose::select(a.xy2d, b.xy2d, choice),
        }
    }
}

impl ExtendedPoint {
    pub(crate) fn identity() -> Self {
        Self {
            x: Fe::ZERO,
            y: Fe::ONE,
            z: Fe::ONE,
            t: Fe::ZERO,
        }
    }

    pub(crate) fn basepoint() -> Self {
        let x = Fe::from_bytes(&BASE_X);
        let y = Fe::from_bytes(&BASE_Y);
        Self {
            x,
            y,
            z: Fe::ONE,
            t: x.mul(y),
        }
    }

    /// `self + q` (7 field multiplications).
    #[inline(always)]
    pub(crate) fn add_niels(&self, q: &AffineNielsPoint) -> Self {
        let pp = self.y.add(self.x).mul(q.y_plus_x);
        let mm = self.y.sub(self.x).mul(q.y_minus_x);
        let txy2d = self.t.relax().mul(q.xy2d);
        let z2 = self.z.add(self.z).carry();
        // Completed point: x = cx/cz, y = cy/ct.
        let cx = pp.sub(mm);
        let cy = pp.add(mm);
        let cz = z2.add(txy2d);
        let ct = z2.sub(txy2d);
        Self {
            x: cx.mul(ct),
            y: cy.mul(cz),
            z: cz.mul(ct),
            t: cx.mul(cy),
        }
    }

    /// `2 * self`.
    pub(crate) fn double(&self) -> Self {
        let xx = self.x.square();
        let yy = self.y.square();
        let zz = self.z.square();
        let zz2 = zz.add(zz).carry();
        let x_plus_y_sq = self.x.add(self.y).square();
        let yy_plus_xx = yy.add(xx).carry();
        let yy_minus_xx = yy.sub(xx).carry();
        let cx = x_plus_y_sq.sub(yy_plus_xx);
        let cy = yy_plus_xx.relax();
        let cz = yy_minus_xx.relax();
        let ct = zz2.sub(yy_minus_xx);
        Self {
            x: cx.mul(ct),
            y: cy.mul(cz),
            z: cz.mul(ct),
            t: cx.mul(cy),
        }
    }

    pub(crate) fn to_affine_niels(self) -> AffineNielsPoint {
        let recip = self.z.invert();
        let x = self.x.mul(recip);
        let y = self.y.mul(recip);
        let xy2d = x.mul(y).mul(Fe::from_bytes(&D2));
        AffineNielsPoint {
            y_plus_x: y.add(x),
            y_minus_x: y.sub(x),
            xy2d: xy2d.relax(),
        }
    }

    /// Standard 32-byte Ed25519 encoding (y with the sign of x in bit 255).
    #[cfg(test)]
    pub(crate) fn compress(&self) -> [u8; 32] {
        let recip = self.z.invert();
        compress_affine(self.x.mul(recip), self.y.mul(recip))
    }

    /// Multiply the base point by a 256-bit little-endian integer (not
    /// reduced mod the group order). Constant time with respect to `scalar`.
    pub(crate) fn mul_base(scalar: &[u8; 32]) -> Self {
        let base = Self::basepoint().to_affine_niels();
        let identity = AffineNielsPoint::identity();
        let mut acc = Self::identity();
        for i in (0..256).rev() {
            acc = acc.double();
            let bit = (scalar[i >> 3] >> (i & 7)) & 1;
            acc = acc.add_niels(&AffineNielsPoint::select(&identity, &base, bit));
        }
        acc
    }

    /// `8 * B` as a precomputed addend: the step between consecutive keys.
    pub(crate) fn eight_basepoint_niels() -> AffineNielsPoint {
        Self::basepoint()
            .double()
            .double()
            .double()
            .to_affine_niels()
    }
}

/// Encode an affine point given its coordinates.
pub(crate) fn compress_affine(x: Fe, y: Fe) -> [u8; 32] {
    let mut bytes = y.to_bytes();
    bytes[31] |= (x.is_negative() as u8) << 7;
    bytes
}

/// The curve constant `d`.
pub(crate) fn curve_d() -> Fe {
    Fe::from_bytes(&D)
}

#[cfg(test)]
mod tests {
    use super::*;
    use curve25519_dalek::{EdwardsPoint, Scalar, constants::ED25519_BASEPOINT_COMPRESSED};

    fn random_bytes() -> [u8; 32] {
        let mut b = [0u8; 32];
        getrandom::fill(&mut b).unwrap();
        b
    }

    #[test]
    fn constants_are_consistent() {
        // d * 121666 + 121665 == 0 (mod p)
        let mut b = [0u8; 32];
        b[..8].copy_from_slice(&121666u64.to_le_bytes());
        let n121666 = Fe::from_bytes(&b);
        b[..8].copy_from_slice(&121665u64.to_le_bytes());
        let n121665 = Fe::from_bytes(&b);
        let d = curve_d();
        assert_eq!(d.mul(n121666).add(n121665).carry().to_bytes(), [0u8; 32]);
        // 2d == d + d
        assert_eq!(d.add(d).carry().to_bytes(), D2);

        // The base point is on the curve: -x^2 + y^2 = 1 + d x^2 y^2.
        let (x, y) = (Fe::from_bytes(&BASE_X), Fe::from_bytes(&BASE_Y));
        let (xx, yy) = (x.square(), y.square());
        let lhs = yy.sub(xx).carry();
        let rhs = Fe::ONE.add(d.mul(xx).mul(yy)).carry();
        assert!(lhs.ct_eq(rhs));
        assert_eq!(
            ExtendedPoint::basepoint().compress(),
            *ED25519_BASEPOINT_COMPRESSED.as_bytes()
        );
    }

    #[test]
    fn mul_base_matches_dalek() {
        for _ in 0..40 {
            let mut k = random_bytes();
            k[31] &= 0x7f;
            let ours = ExtendedPoint::mul_base(&k).compress();
            let theirs = EdwardsPoint::mul_base(&Scalar::from_bytes_mod_order(k)).compress();
            assert_eq!(ours, theirs.to_bytes());
        }
        // Edge cases: 0, 1, and a full 256-bit value.
        assert_eq!(
            ExtendedPoint::mul_base(&[0u8; 32]).compress(),
            EdwardsPoint::mul_base(&Scalar::ZERO).compress().to_bytes()
        );
        let mut one = [0u8; 32];
        one[0] = 1;
        assert_eq!(
            ExtendedPoint::mul_base(&one).compress(),
            *ED25519_BASEPOINT_COMPRESSED.as_bytes()
        );
        let all = [0xffu8; 32];
        assert_eq!(
            ExtendedPoint::mul_base(&all).compress(),
            EdwardsPoint::mul_base(&Scalar::from_bytes_mod_order(all))
                .compress()
                .to_bytes()
        );
    }

    #[test]
    fn stepping_by_eight_b_matches_dalek() {
        let mut k = random_bytes();
        k[31] &= 0x7f;
        let step = ExtendedPoint::eight_basepoint_niels();
        let mut p = ExtendedPoint::mul_base(&k);
        let mut expected = EdwardsPoint::mul_base(&Scalar::from_bytes_mod_order(k));
        let eight_b = EdwardsPoint::mul_base(&Scalar::from(8u8));
        for _ in 0..200 {
            assert_eq!(p.compress(), expected.compress().to_bytes());
            p = p.add_niels(&step);
            expected += eight_b;
        }
    }

    #[test]
    fn doubling_matches_addition() {
        let p = ExtendedPoint::mul_base(&random_bytes());
        let doubled = p.double().compress();
        let added = p.add_niels(&p.to_affine_niels()).compress();
        assert_eq!(doubled, added);
    }
}

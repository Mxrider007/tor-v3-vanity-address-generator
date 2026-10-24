//! Generates the WGSL source: the hand-written kernel plus fully unrolled
//! field arithmetic (GPUs need straight-line code with constant indices to
//! keep the limbs in registers).
//!
//! Limb bounds (see kernel.wgsl): inputs to `fe_mul`/`fe_sq` have limbs
//! below 8256, so a product is below 2^26.02 and a column of at most twenty
//! products stays below 2^31; the reduction folds column `20 + i` into column
//! `i` with the factor 608 (2^260 = 2^5 · 2^255 ≡ 32 · 19 mod p).

use std::fmt::Write;

const LIMBS: usize = 20;
const BITS: usize = 13;

/// Full shader source for groups of `2 * group + 1` keys.
pub(super) fn source(group: usize) -> String {
    let mut s = String::new();
    writeln!(s, "const G: u32 = {group}u;").unwrap();
    s.push_str(include_str!("kernel.wgsl"));
    s.push_str(&carry());
    s.push_str(&mul());
    s.push_str(&square());
    s.push_str(&pack());
    s.push_str(&unpack());
    s.push_str(&table_entry());
    s
}

/// Fold and carry: limbs 0..=18 below 2^13 (limb 1 may exceed it by a few
/// units), limb 19 below 2^8. Input limbs must be below 2^31.
fn carry() -> String {
    let mut s = String::from("\nfn fe_carry(a: Fe) -> Fe {\n    var r = a;\n");
    for i in 0..LIMBS - 1 {
        writeln!(s, "    r[{}] += r[{i}] >> 13u; r[{i}] &= M;", i + 1).unwrap();
    }
    s.push_str("    let hi = r[19] >> 8u;\n    r[19] &= 0xffu;\n    r[0] += 19u * hi;\n");
    s.push_str("    r[1] += r[0] >> 13u; r[0] &= M;\n    return r;\n}\n");
    s
}

/// Reduce the 39 column sums `c0..c38` (already declared) into a field
/// element: carry through all columns, then fold the high half.
fn reduce_columns() -> String {
    let mut s = String::new();
    for k in 0..2 * LIMBS - 2 {
        writeln!(s, "    c{} += c{k} >> 13u; c{k} &= M;", k + 1).unwrap();
    }
    s.push_str("    let c39 = c38 >> 13u; c38 &= M;\n    var r: Fe;\n");
    for i in 0..LIMBS {
        writeln!(s, "    r[{i}] = c{i} + 608u * c{};", i + LIMBS).unwrap();
    }
    s.push_str("    return fe_carry(r);\n}\n");
    s
}

fn mul() -> String {
    let mut s = String::from("\nfn fe_mul(a: Fe, b: Fe) -> Fe {\n");
    for k in 0..2 * LIMBS - 1 {
        let terms: Vec<String> = (k.saturating_sub(LIMBS - 1)..=k.min(LIMBS - 1))
            .map(|i| format!("a[{i}] * b[{}]", k - i))
            .collect();
        writeln!(s, "    var c{k}: u32 = {};", terms.join(" + ")).unwrap();
    }
    s.push_str(&reduce_columns());
    s
}

fn square() -> String {
    let mut s = String::from("\nfn fe_sq(a: Fe) -> Fe {\n    var d: Fe;\n");
    for i in 0..LIMBS {
        writeln!(s, "    d[{i}] = a[{i}] << 1u;").unwrap();
    }
    for k in 0..2 * LIMBS - 1 {
        let mut terms: Vec<String> = (k.saturating_sub(LIMBS - 1)..=k.min(LIMBS - 1))
            .filter(|&i| i < k - i)
            .map(|i| format!("d[{i}] * a[{}]", k - i))
            .collect();
        if k % 2 == 0 {
            terms.push(format!("a[{}] * a[{}]", k / 2, k / 2));
        }
        writeln!(s, "    var c{k}: u32 = {};", terms.join(" + ")).unwrap();
    }
    s.push_str(&reduce_columns());
    s
}

/// Pack strictly carried limbs (limbs 0..=18 < 2^13, limb 19 < 2^9) into
/// eight little-endian 32-bit words.
fn pack() -> String {
    let mut s = String::from("\nfn fe_pack(a: Fe) -> Packed {\n    var w: Packed;\n");
    for i in 0..LIMBS {
        let bit = i * BITS;
        let (word, shift) = (bit / 32, bit % 32);
        writeln!(s, "    w[{word}] |= a[{i}] << {shift}u;").unwrap();
        if shift + BITS > 32 && word + 1 < 8 {
            writeln!(s, "    w[{}] |= a[{i}] >> {}u;", word + 1, 32 - shift).unwrap();
        }
    }
    s.push_str("    return w;\n}\n");
    s
}

fn unpack() -> String {
    let mut s = String::from("\nfn fe_unpack(w: Packed) -> Fe {\n    var a: Fe;\n");
    for i in 0..LIMBS {
        let bit = i * BITS;
        let (word, shift) = (bit / 32, bit % 32);
        if shift + BITS > 32 && word + 1 < 8 {
            writeln!(
                s,
                "    a[{i}] = ((w[{word}] >> {shift}u) | (w[{}] << {}u)) & M;",
                word + 1,
                32 - shift
            )
            .unwrap();
        } else {
            writeln!(s, "    a[{i}] = (w[{word}] >> {shift}u) & M;").unwrap();
        }
    }
    s.push_str("    return a;\n}\n");
    s
}

/// `table_entry(i, which)`: coordinate `which` (0 = x, 1 = y, 2 = e) of
/// table entry `i`.
fn table_entry() -> String {
    let mut s = String::from(
        "\nfn table_entry(i: u32, which: u32) -> Fe {\n    let b = (i * 3u + which) * 20u;\n    return Fe(",
    );
    let loads: Vec<String> = (0..LIMBS).map(|k| format!("table[b + {k}u]")).collect();
    s.push_str(&loads.join(", "));
    s.push_str(");\n}\n");
    s
}

/// Split a canonical little-endian field element into 20 limbs of 13 bits.
pub(super) fn limbs(bytes: &[u8; 32]) -> [u32; LIMBS] {
    let mut out = [0u32; LIMBS];
    for (i, limb) in out.iter_mut().enumerate() {
        for b in 0..BITS {
            let bit = i * BITS + b;
            if bit < 256 && bytes[bit / 8] >> (bit % 8) & 1 == 1 {
                *limb |= 1 << b;
            }
        }
    }
    out
}

/// Eight little-endian words of a 32-byte little-endian value.
pub(super) fn words(bytes: &[u8; 32]) -> [u32; 8] {
    let mut out = [0u32; 8];
    for (w, chunk) in out.iter_mut().zip(bytes.as_chunks::<4>().0) {
        *w = u32::from_le_bytes(*chunk);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_bounds_fit_in_u32() {
        // Worst case for fe_sq: ten doubled cross terms plus one square.
        let limb: u64 = 8255;
        let mul_column = 20 * limb * limb;
        let sq_column = 10 * (2 * limb) * limb + limb * limb;
        assert!(mul_column < 1 << 31);
        assert!(sq_column < 1 << 31);
    }

    #[test]
    fn limbs_and_words() {
        let mut bytes = [0u8; 32];
        bytes[0] = 0xff;
        bytes[1] = 0x3f; // bits 0..13 set
        let l = limbs(&bytes);
        assert_eq!(l[0], 0x1fff);
        assert_eq!(l[1], 1);
        assert_eq!(words(&bytes)[0], 0x3fff);
    }

    #[test]
    fn generated_source_is_complete() {
        let src = source(4);
        for f in [
            "fn fe_mul",
            "fn fe_sq",
            "fn fe_carry",
            "fn fe_pack",
            "fn fe_unpack",
            "fn table_entry",
            "fn search",
        ] {
            assert!(src.contains(f), "missing {f}");
        }
    }
}

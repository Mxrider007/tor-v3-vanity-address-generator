//! Search patterns: what counts as a "vanity" address.
//!
//! A pattern is a string of base32 characters (`a`–`z`, `2`–`7`, any case)
//! where `?` matches any single character. By default a pattern must match at
//! the start of the address; in "anywhere" mode it may match at any position.
//! Optional "contains" patterns must additionally appear somewhere in the
//! address.

use thiserror::Error;

use crate::onion::{ADDRESS_LEN, ALPHABET, PUBKEY_CHARS};

/// The single-character wildcard.
pub const WILDCARD: char = '?';

/// A validated, lowercase search pattern.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Pattern {
    text: String,
}

/// Why a pattern was rejected. The messages are written for end users.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PatternError {
    #[error("the pattern is empty")]
    Empty,
    #[error("'{pattern}' is {len} characters long, but an onion address only has 56")]
    TooLong { pattern: String, len: usize },
    #[error("'{pattern}' contains '{ch}', which never appears in onion addresses ({hint})")]
    InvalidChar {
        pattern: String,
        ch: char,
        hint: &'static str,
    },
    #[error("'{0}' is only wildcards, so every address would match it")]
    OnlyWildcards(String),
    #[error("'{pattern}' can never match at the start of an address: {reason}")]
    Impossible {
        pattern: String,
        reason: &'static str,
    },
    #[error("no patterns were given")]
    NoPatterns,
}

impl Pattern {
    /// Parse and validate a single pattern (surrounding whitespace is ignored).
    pub fn parse(input: &str) -> Result<Pattern, PatternError> {
        let text = input.trim().to_ascii_lowercase();
        if text.is_empty() {
            return Err(PatternError::Empty);
        }
        if let Some(ch) = text
            .chars()
            .find(|&c| c != WILDCARD && !ALPHABET.contains(&(c as u8)) || !c.is_ascii())
        {
            return Err(PatternError::InvalidChar {
                pattern: input.trim().to_string(),
                ch,
                hint: invalid_char_hint(ch),
            });
        }
        let len = text.len();
        if len > ADDRESS_LEN {
            return Err(PatternError::TooLong { pattern: text, len });
        }
        if text.chars().all(|c| c == WILDCARD) {
            return Err(PatternError::OnlyWildcards(text));
        }
        Ok(Pattern { text })
    }

    /// Parse a comma- and/or whitespace-separated list, dropping duplicates.
    pub fn parse_list(input: &str) -> Result<Vec<Pattern>, PatternError> {
        let mut out: Vec<Pattern> = Vec::new();
        for part in input.split(|c: char| c == ',' || c.is_whitespace()) {
            if part.is_empty() {
                continue;
            }
            let pattern = Pattern::parse(part)?;
            if !out.contains(&pattern) {
                out.push(pattern);
            }
        }
        Ok(out)
    }

    /// The normalized (lowercase) pattern text.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Number of characters, including wildcards.
    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// Always false: empty patterns are rejected by [`Pattern::parse`].
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Number of non-wildcard characters.
    pub fn fixed_chars(&self) -> usize {
        self.text.bytes().filter(|&b| b != WILDCARD as u8).count()
    }

    fn bytes(&self) -> &[u8] {
        self.text.as_bytes()
    }

    /// Whether the pattern matches `address` (base32 chars) at `pos`.
    pub fn matches_at(&self, address: &[u8], pos: usize) -> bool {
        let p = self.bytes();
        pos + p.len() <= address.len()
            && p.iter()
                .zip(&address[pos..])
                .all(|(&pc, &ac)| pc == WILDCARD as u8 || pc == ac)
    }

    /// Whether the pattern matches at the start of `address`.
    pub fn matches_start(&self, address: &[u8]) -> bool {
        self.matches_at(address, 0)
    }

    /// Whether the pattern matches anywhere in `address`.
    pub fn matches_anywhere(&self, address: &[u8]) -> bool {
        address.len() >= self.len()
            && (0..=address.len() - self.len()).any(|pos| self.matches_at(address, pos))
    }

    /// Probability that a random address matches this pattern at `pos`.
    fn probability_at(&self, pos: usize) -> f64 {
        let mut p = 1.0;
        for (i, &c) in self.bytes().iter().enumerate() {
            if c == WILDCARD as u8 {
                continue;
            }
            p *= match pos + i {
                // Two random checksum bits followed by three zero version bits.
                54 if b"aiqy".contains(&c) => 0.25,
                54 => 0.0,
                // The last five bits of the version byte (3) are always 'd'.
                55 if c == b'd' => 1.0,
                55 => 0.0,
                _ => 1.0 / 32.0,
            };
        }
        p
    }

    /// Probability that a random address matches this pattern (a union bound
    /// over positions in anywhere mode; accurate for realistic patterns).
    pub fn probability(&self, anywhere: bool) -> f64 {
        if !anywhere {
            return self.probability_at(0);
        }
        let positions = ADDRESS_LEN - self.len() + 1;
        (0..positions)
            .map(|pos| self.probability_at(pos))
            .sum::<f64>()
            .min(1.0)
    }

    /// Bit mask/value over the raw public key for the start-anchored case.
    pub(crate) fn prefix_filter(&self) -> PrefixFilter {
        PrefixFilter::new(self)
    }
}

impl std::fmt::Display for Pattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}

impl std::str::FromStr for Pattern {
    type Err = PatternError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Pattern::parse(s)
    }
}

fn invalid_char_hint(ch: char) -> &'static str {
    match ch {
        '0' => "the digits 0, 1, 8 and 9 are not used; try 'o' instead of 0",
        '1' => "the digits 0, 1, 8 and 9 are not used; try 'l' or 'i' instead of 1",
        '8' => "the digits 0, 1, 8 and 9 are not used; try 'b' instead of 8",
        '9' => "the digits 0, 1, 8 and 9 are not used; try 'g' instead of 9",
        '*' => "use ? to match any single character",
        '.' => "leave out the \".onion\" suffix",
        '-' | '_' => "addresses contain only letters a-z and digits 2-7",
        _ => "only a-z, 2-7 and ? are allowed",
    }
}

/// Result of checking an address against a [`Matcher`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchResult {
    /// The address satisfies pattern number `.0` and every "contains" pattern.
    Match(usize),
    /// A pattern matched, but a required "contains" pattern did not.
    MissingContains,
    /// No wanted pattern matched.
    NoMatch,
}

/// A complete search specification: the patterns, the matching mode and any
/// additional "contains" requirements.
#[derive(Debug, Clone)]
pub struct Matcher {
    patterns: Vec<Pattern>,
    anywhere: bool,
    contains: Vec<Pattern>,
    /// Anywhere mode: for each pattern, the start positions it can occupy
    /// given the fixed address tail.
    feasible: Vec<Vec<usize>>,
}

impl Matcher {
    /// Create a matcher. Patterns that can never match are rejected here.
    pub fn new(
        patterns: Vec<Pattern>,
        anywhere: bool,
        contains: Vec<Pattern>,
    ) -> Result<Matcher, PatternError> {
        if patterns.is_empty() {
            return Err(PatternError::NoPatterns);
        }
        for p in patterns.iter().chain(&contains) {
            let anywhere = anywhere || contains.contains(p);
            if p.probability(anywhere) == 0.0 {
                return Err(PatternError::Impossible {
                    pattern: p.to_string(),
                    reason: "the last two characters of every v3 address are one of a/i/q/y followed by 'd'",
                });
            }
        }
        let feasible = patterns
            .iter()
            .map(|p| {
                (0..=ADDRESS_LEN - p.len())
                    .filter(|&pos| p.probability_at(pos) > 0.0)
                    .collect()
            })
            .collect();
        Ok(Matcher {
            patterns,
            anywhere,
            contains,
            feasible,
        })
    }

    /// The patterns being searched for, in priority order.
    pub fn patterns(&self) -> &[Pattern] {
        &self.patterns
    }

    /// Whether patterns may match anywhere rather than only at the start.
    pub fn anywhere(&self) -> bool {
        self.anywhere
    }

    /// Patterns that must additionally appear somewhere in the address.
    pub fn contains(&self) -> &[Pattern] {
        &self.contains
    }

    /// Whether pattern `i` matches `address` (ignoring "contains").
    pub fn pattern_matches(&self, i: usize, address: &[u8]) -> bool {
        if self.anywhere {
            self.patterns[i].matches_anywhere(address)
        } else {
            self.patterns[i].matches_start(address)
        }
    }

    /// Check a full 56-character address. Only patterns for which `wanted`
    /// returns true are considered; the first one that matches wins.
    pub fn check(
        &self,
        address: &[u8; ADDRESS_LEN],
        wanted: impl Fn(usize) -> bool,
    ) -> MatchResult {
        let Some(i) =
            (0..self.patterns.len()).find(|&i| wanted(i) && self.pattern_matches(i, address))
        else {
            return MatchResult::NoMatch;
        };
        if self.contains.iter().all(|c| c.matches_anywhere(address)) {
            MatchResult::Match(i)
        } else {
            MatchResult::MissingContains
        }
    }

    /// Probability that a random address matches pattern `i`, including the
    /// "contains" requirements (treated as independent).
    pub fn probability(&self, i: usize) -> f64 {
        let contains: f64 = self.contains.iter().map(|c| c.probability(true)).product();
        self.patterns[i].probability(self.anywhere) * contains
    }

    /// For anywhere mode: could any wanted pattern match an address whose
    /// first [`PUBKEY_CHARS`] characters are `head`? `false` is definitive;
    /// `true` means the full address must be checked.
    pub(crate) fn may_match_anywhere(
        &self,
        head: &[u8; PUBKEY_CHARS],
        wanted: impl Fn(usize) -> bool,
    ) -> bool {
        self.patterns.iter().enumerate().any(|(i, p)| {
            if !wanted(i) {
                return false;
            }
            let bytes = p.bytes();
            self.feasible[i].iter().any(|&pos| {
                // Compare only the characters that fall inside `head`.
                bytes
                    .iter()
                    .enumerate()
                    .take_while(|(k, _)| pos + k < PUBKEY_CHARS)
                    .all(|(k, &c)| c == WILDCARD as u8 || c == head[pos + k])
            })
        })
    }
}

/// A necessary condition for a start-anchored pattern, as a bit mask over the
/// big-endian 256-bit public key. Because the first 51 address characters are
/// just the public key's bits, `(key & mask) == value` is an exact test for
/// every fixed character that lies entirely inside the key — except the bit
/// holding the sign of x, which the fast search does not compute and which is
/// therefore excluded from the mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PrefixFilter {
    pub(crate) mask: [u64; 4],
    pub(crate) value: [u64; 4],
    /// Number of leading words with a non-zero mask.
    pub(crate) words: usize,
}

impl PrefixFilter {
    fn new(pattern: &Pattern) -> PrefixFilter {
        let mut mask = [0u64; 4];
        let mut value = [0u64; 4];
        for (pos, &c) in pattern.bytes().iter().enumerate() {
            if c == WILDCARD as u8 || pos >= PUBKEY_CHARS {
                continue;
            }
            let digit = ALPHABET.iter().position(|&a| a == c).expect("validated") as u64;
            for bit in 0..5 {
                let stream_bit = pos * 5 + bit; // 0 = most significant bit of byte 0
                if stream_bit >= 256 {
                    continue;
                }
                let (word, shift) = (stream_bit / 64, 63 - stream_bit % 64);
                mask[word] |= 1 << shift;
                value[word] |= ((digit >> (4 - bit)) & 1) << shift;
            }
        }
        // Stream bit 248 is the sign of x (top bit of the last key byte).
        mask[3] &= !(1 << (63 - 248 % 64));
        value[3] &= mask[3];
        let words = mask.iter().rposition(|&m| m != 0).map_or(0, |w| w + 1);
        PrefixFilter { mask, value, words }
    }

    /// Test the canonical little-endian encoding of y (sign bit clear).
    #[inline(always)]
    pub(crate) fn test(&self, key: &[u8; 32]) -> bool {
        (0..self.words).all(|w| {
            let word = u64::from_be_bytes(key[w * 8..w * 8 + 8].try_into().expect("8 bytes"));
            word & self.mask[w] == self.value[w]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::onion::{SecretKey, address_chars};

    fn p(s: &str) -> Pattern {
        Pattern::parse(s).unwrap()
    }

    fn random_pubkey() -> [u8; 32] {
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed).unwrap();
        SecretKey::from_seed(&seed).public_key()
    }

    #[test]
    fn parsing_and_normalization() {
        assert_eq!(p(" KoRe ").as_str(), "kore");
        assert_eq!(p("k?re").fixed_chars(), 3);
        assert_eq!(Pattern::parse(""), Err(PatternError::Empty));
        assert!(matches!(
            Pattern::parse("kore0"),
            Err(PatternError::InvalidChar { ch: '0', .. })
        ));
        assert!(matches!(
            Pattern::parse("k*"),
            Err(PatternError::InvalidChar { ch: '*', .. })
        ));
        assert!(matches!(
            Pattern::parse("kore.onion"),
            Err(PatternError::InvalidChar { ch: '.', .. })
        ));
        assert!(matches!(
            Pattern::parse("ké"),
            Err(PatternError::InvalidChar { ch: 'é', .. })
        ));
        assert_eq!(
            Pattern::parse("???"),
            Err(PatternError::OnlyWildcards("???".into()))
        );
        assert!(matches!(
            Pattern::parse(&"a".repeat(57)),
            Err(PatternError::TooLong { len: 57, .. })
        ));
        let list = Pattern::parse_list("kore, test  kore,,abc").unwrap();
        assert_eq!(
            list.iter().map(Pattern::as_str).collect::<Vec<_>>(),
            ["kore", "test", "abc"]
        );
    }

    #[test]
    fn matching() {
        let addr = b"koretestxyz";
        assert!(p("kore").matches_start(addr));
        assert!(p("k?re").matches_start(addr));
        assert!(!p("test").matches_start(addr));
        assert!(p("test").matches_anywhere(addr));
        assert!(p("t?st").matches_anywhere(addr));
        assert!(p("????test").matches_start(addr));
        assert!(!p("xyzq").matches_anywhere(addr));
        assert!(!p("koretestxyzab").matches_anywhere(addr));
    }

    #[test]
    fn matcher_priority_and_contains() {
        let m = Matcher::new(vec![p("ab"), p("a")], false, vec![p("zz")]).unwrap();
        let mut addr = [b'a'; ADDRESS_LEN];
        assert_eq!(m.check(&addr, |_| true), MatchResult::MissingContains);
        addr[..4].copy_from_slice(b"abzz");
        assert_eq!(m.check(&addr, |_| true), MatchResult::Match(0));
        assert_eq!(m.check(&addr, |i| i != 0), MatchResult::Match(1));
        assert_eq!(m.check(&addr, |_| false), MatchResult::NoMatch);
    }

    #[test]
    fn impossible_patterns_are_rejected() {
        let tail_ok = format!("{}ad", "?".repeat(54));
        assert!(Matcher::new(vec![p(&tail_ok)], false, vec![]).is_ok());
        let tail_bad = format!("{}bd", "?".repeat(54));
        assert!(matches!(
            Matcher::new(vec![p(&tail_bad)], false, vec![]),
            Err(PatternError::Impossible { .. })
        ));
        // Anywhere mode can still place it elsewhere.
        assert!(Matcher::new(vec![p("bd")], true, vec![]).is_ok());
        assert_eq!(
            Matcher::new(vec![], false, vec![]).unwrap_err(),
            PatternError::NoPatterns
        );
    }

    #[test]
    fn probabilities() {
        assert_eq!(p("kore").probability(false), 1.0 / 32f64.powi(4));
        assert_eq!(p("k?re").probability(false), 1.0 / 32f64.powi(3));
        let anywhere = p("kore").probability(true);
        assert!(anywhere > 50.0 / 32f64.powi(4) && anywhere < 53.0 / 32f64.powi(4));
        assert_eq!(p(&format!("{}ad", "?".repeat(54))).probability(false), 0.25);
    }

    #[test]
    fn prefix_filter_is_exact_for_leading_chars() {
        // Build patterns from real addresses and check the filter agrees with
        // real matching on random keys.
        for _ in 0..200 {
            let pk = random_pubkey();
            let chars = address_chars(&pk);
            let pattern = p(std::str::from_utf8(&chars[..6]).unwrap());
            let filter = pattern.prefix_filter();
            let mut y = pk;
            y[31] &= 0x7f;
            assert!(filter.test(&y));
            assert_eq!(filter.words, 1);
        }
        // Short patterns match often, so both outcomes get exercised.
        let short = [p("a"), p("q?"), p("?7"), p("zz")];
        let mut hits = 0;
        for _ in 0..3000 {
            let pk = random_pubkey();
            let mut y = pk;
            y[31] &= 0x7f;
            let chars = address_chars(&pk);
            for pattern in &short {
                let expected = pattern.matches_start(&chars);
                assert_eq!(pattern.prefix_filter().test(&y), expected);
                hits += usize::from(expected);
            }
        }
        assert!(hits > 100);
        // Long patterns use more words; the sign bit is ignored.
        let pk = random_pubkey();
        let chars = address_chars(&pk);
        let pattern = p(std::str::from_utf8(&chars[..51]).unwrap());
        let filter = pattern.prefix_filter();
        assert_eq!(filter.words, 4);
        let mut y = pk;
        y[31] &= 0x7f;
        assert!(filter.test(&y));
    }

    #[test]
    fn may_match_anywhere_never_misses() {
        let m = Matcher::new(vec![p("ab"), p("q?d")], true, vec![]).unwrap();
        for _ in 0..2000 {
            let pk = random_pubkey();
            let chars = address_chars(&pk);
            let head: [u8; PUBKEY_CHARS] = chars[..PUBKEY_CHARS].try_into().unwrap();
            if m.check(&chars, |_| true) != MatchResult::NoMatch {
                assert!(m.may_match_anywhere(&head, |_| true));
            }
        }
    }
}

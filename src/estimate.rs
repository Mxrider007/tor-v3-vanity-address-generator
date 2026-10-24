//! Difficulty and time estimates.
//!
//! Every key is an independent trial, so the number of keys needed to find a
//! pattern with per-key probability `p` is geometric: on average `1/p`, with
//! a 50% chance within `ln(2)/p` and a 90% chance within `ln(10)/p`. There is
//! no such thing as "percent complete" — only the chance of having found it.
//!
//! A search for several patterns is done when the last of them has been
//! found, which takes longer than finding just the rarest one: three equally
//! rare patterns take about 1.8 times as long as one.

use std::f64::consts::SQRT_2;

use crate::pattern::Matcher;

/// How hard a search is.
#[derive(Debug, Clone, PartialEq)]
pub struct Estimate {
    /// Per-key match probability for each pattern (including "contains").
    per_pattern: Vec<f64>,
    /// Addresses wanted per pattern (`None` = unlimited).
    count: Option<u64>,
    /// Cached [`Estimate::expected_keys`].
    expected: f64,
}

impl Estimate {
    pub fn new(matcher: &Matcher, count: Option<u64>) -> Estimate {
        let mut estimate = Estimate {
            per_pattern: (0..matcher.patterns().len())
                .map(|i| matcher.probability(i))
                .collect(),
            count,
            expected: 0.0,
        };
        estimate.expected = estimate.compute_expected_keys();
        estimate
    }

    /// Addresses wanted per pattern (`None` = unlimited).
    pub fn count(&self) -> Option<u64> {
        self.count
    }

    /// Number of patterns.
    pub fn patterns(&self) -> usize {
        self.per_pattern.len()
    }

    /// Total number of addresses the search will find (`None` = unlimited).
    pub fn total(&self) -> Option<u64> {
        self.count
            .map(|n| n.saturating_mul(self.per_pattern.len() as u64))
    }

    /// Match probability of the rarest pattern.
    pub fn hardest(&self) -> f64 {
        self.per_pattern.iter().copied().fold(1.0, f64::min)
    }

    /// Expected number of keys until the search is done. With an unlimited
    /// count this is the expected number of keys per address found.
    pub fn expected_keys(&self) -> f64 {
        self.expected
    }

    /// Chance that the search is done after checking `keys` keys: every
    /// pattern found as often as wanted (with an unlimited count: any address
    /// found).
    pub fn chance_after(&self, keys: f64) -> f64 {
        if keys <= 0.0 {
            return 0.0;
        }
        match self.count {
            None => {
                let miss: f64 = self.per_pattern.iter().map(|&p| ln_miss(p)).sum();
                -(keys * miss).exp_m1()
            }
            Some(n) => self
                .per_pattern
                .iter()
                .map(|&p| found_at_least(n, p, keys))
                .product(),
        }
    }

    /// Keys needed for the given chance (e.g. `0.5` or `0.9`) that the search
    /// is done; see [`Estimate::chance_after`].
    pub fn keys_for_chance(&self, chance: f64) -> f64 {
        if chance <= 0.0 {
            return 0.0;
        }
        if chance >= 1.0 {
            return f64::INFINITY;
        }
        let single = |ln_miss: f64| {
            if ln_miss == f64::NEG_INFINITY {
                1.0
            } else {
                (-chance).ln_1p() / ln_miss
            }
        };
        match (self.count, self.per_pattern.as_slice()) {
            (None, patterns) => single(patterns.iter().map(|&p| ln_miss(p)).sum()),
            (Some(1), [p]) => single(ln_miss(*p)),
            _ => {
                // The chance grows with the number of keys: bisect.
                let mut hi = self.count.unwrap_or(1) as f64 / self.hardest();
                while self.chance_after(hi) < chance {
                    hi *= 2.0;
                    if !hi.is_finite() {
                        return f64::INFINITY;
                    }
                }
                let mut lo = 0.0;
                for _ in 0..200 {
                    let mid = 0.5 * (lo + hi);
                    if self.chance_after(mid) < chance {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                    if hi - lo <= hi * 1e-12 {
                        break;
                    }
                }
                hi
            }
        }
    }

    fn compute_expected_keys(&self) -> f64 {
        match (self.count, self.per_pattern.as_slice()) {
            (None, patterns) => 1.0 / patterns.iter().sum::<f64>(),
            (Some(n), [p]) => n as f64 / p,
            _ => {
                // E[keys] = ∫ P(not done after k keys) dk (Simpson's rule).
                let end = self.keys_for_chance(1.0 - 1e-9);
                if !end.is_finite() {
                    return f64::INFINITY;
                }
                const STEPS: usize = 2000;
                let h = end / STEPS as f64;
                let not_done = |i: usize| 1.0 - self.chance_after(i as f64 * h);
                let inner: f64 = (1..STEPS)
                    .map(|i| not_done(i) * if i % 2 == 1 { 4.0 } else { 2.0 })
                    .sum();
                h / 3.0 * (not_done(0) + inner + not_done(STEPS))
            }
        }
    }
}

/// `ln(1 - p)`: the log of the chance that one key misses a pattern.
fn ln_miss(p: f64) -> f64 {
    (-p.min(1.0)).ln_1p()
}

/// Chance that a pattern with per-key probability `p` was found at least `n`
/// times among `keys` keys.
fn found_at_least(n: u64, p: f64, keys: f64) -> f64 {
    if n == 1 {
        -(keys * ln_miss(p)).exp_m1()
    } else {
        poisson_at_least(n, keys * p)
    }
}

/// `P(X >= n)` for `X ~ Poisson(lambda)`.
fn poisson_at_least(n: u64, lambda: f64) -> f64 {
    if n == 0 || lambda == f64::INFINITY {
        return 1.0;
    }
    if lambda <= 0.0 || lambda.is_nan() {
        return 0.0;
    }
    if n > 1000 {
        // Normal approximation, with continuity correction.
        let z = (n as f64 - 0.5 - lambda) / lambda.sqrt();
        return 0.5 * erfc(z / SQRT_2);
    }
    // P(X < n) = Σ e^-λ λ^j / j! for j < n, summed in log space.
    let ln_lambda = lambda.ln();
    let (mut ln_term, mut max, mut sum) = (-lambda, f64::NEG_INFINITY, 0.0);
    for j in 0..n {
        if j > 0 {
            ln_term += ln_lambda - (j as f64).ln();
        }
        if ln_term > max {
            sum = sum * (max - ln_term).exp() + 1.0;
            max = ln_term;
        } else {
            sum += (ln_term - max).exp();
        }
    }
    (1.0 - (max + sum.ln()).exp()).clamp(0.0, 1.0)
}

/// Complementary error function (Numerical Recipes `erfcc`, relative error
/// below 1.2e-7).
fn erfc(x: f64) -> f64 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.5 * z);
    let poly = -1.265_512_23
        + t * (1.000_023_68
            + t * (0.374_091_96
                + t * (0.096_784_18
                    + t * (-0.186_288_06
                        + t * (0.278_868_07
                            + t * (-1.135_203_98
                                + t * (1.488_515_87 + t * (-0.822_152_23 + t * 0.170_872_77))))))));
    let r = t * (-z * z + poly).exp();
    if x >= 0.0 { r } else { 2.0 - r }
}

/// Human-friendly duration such as `"42 seconds"`, `"3 h 20 min"` or
/// `"1.2K years"`.
pub fn format_duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "unknown".to_string();
    }
    const MIN: f64 = 60.0;
    const HOUR: f64 = 3600.0;
    const DAY: f64 = 86_400.0;
    const YEAR: f64 = 365.25 * DAY;
    let s = seconds;
    if s < 1.0 {
        "under a second".to_string()
    } else if s < MIN {
        plural(s.floor(), "second")
    } else if s < HOUR {
        let m = (s / MIN).floor();
        let rest = (s - m * MIN).floor();
        if m < 10.0 && rest > 0.0 {
            format!("{m} min {rest} s")
        } else {
            format!("{m} min")
        }
    } else if s < DAY {
        let h = (s / HOUR).floor();
        let m = ((s - h * HOUR) / MIN).floor();
        format!("{h} h {m} min")
    } else if s < 60.0 * DAY {
        let d = (s / DAY).floor();
        let h = ((s - d * DAY) / HOUR).floor();
        if d < 10.0 && h > 0.0 {
            format!("{} {h} h", plural(d, "day"))
        } else {
            plural(d, "day")
        }
    } else if s < 2.0 * YEAR {
        plural((s / (30.44 * DAY)).round(), "month")
    } else {
        let years = s / YEAR;
        if years < 1000.0 {
            plural(years.floor(), "year")
        } else {
            format!("{} years", format_count(years))
        }
    }
}

fn plural(n: f64, unit: &str) -> String {
    if n == 1.0 {
        format!("1 {unit}")
    } else {
        format!("{n} {unit}s")
    }
}

/// Compact number: `950`, `12.3K`, `4.5M`, `6.7B`, `8.9T`, `1.2Q`, then
/// scientific notation.
pub fn format_count(n: f64) -> String {
    if n.is_nan() {
        return "unknown".to_string();
    }
    if !n.is_finite() {
        return "∞".to_string();
    }
    if n < 999.5 {
        return format!("{n:.0}");
    }
    // Pick the unit after rounding, so 999,999 is "1.0M" and not "1000K".
    const UNITS: [(f64, &str); 5] = [(1e3, "K"), (1e6, "M"), (1e9, "B"), (1e12, "T"), (1e15, "Q")];
    for (scale, suffix) in UNITS {
        let v = n / scale;
        if v < 99.95 {
            return format!("{v:.1}{suffix}");
        }
        if v < 999.5 {
            return format!("{v:.0}{suffix}");
        }
    }
    format!("{n:.1e}")
}

/// Key rate such as `"5.3M keys/s"`.
pub fn format_rate(keys_per_second: f64) -> String {
    format!("{} keys/s", format_count(keys_per_second))
}

/// Odds such as `"1 in 1.0M"`.
pub fn format_odds(p: f64) -> String {
    if p.is_nan() {
        "unknown".to_string()
    } else if p <= 0.0 {
        "impossible".to_string()
    } else if p >= 1.0 {
        "certain".to_string()
    } else {
        format!("1 in {}", format_count(1.0 / p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern::Pattern;

    fn matcher(patterns: &[&str], contains: &[&str]) -> Matcher {
        let parse = |v: &[&str]| v.iter().map(|s| Pattern::parse(s).unwrap()).collect();
        Matcher::new(parse(patterns), false, parse(contains)).unwrap()
    }

    #[test]
    fn expected_keys() {
        let e = Estimate::new(&matcher(&["shop"], &[]), Some(1));
        assert_eq!(e.expected_keys(), 32f64.powi(4));
        // The rarest pattern dominates.
        let e = Estimate::new(&matcher(&["ab", "abcde"], &[]), Some(2));
        assert!((e.expected_keys() / (2.0 * 32f64.powi(5)) - 1.0).abs() < 1e-6);
        // Unlimited: expected keys per address found across all patterns.
        let e = Estimate::new(&matcher(&["a", "b"], &[]), None);
        assert_eq!(e.expected_keys(), 16.0);
        // "contains" makes it rarer.
        let plain = Estimate::new(&matcher(&["ab"], &[]), Some(1)).expected_keys();
        let with = Estimate::new(&matcher(&["ab"], &["xyz"]), Some(1)).expected_keys();
        assert!(with > plain * 500.0);
        // Several equally rare patterns: the search ends with the last one,
        // after (1 + 1/2 + 1/3) / p keys on average.
        let e = Estimate::new(&matcher(&["shop", "blog", "news"], &[]), Some(1));
        let want = (1.0 + 0.5 + 1.0 / 3.0) * 32f64.powi(4);
        assert!(
            (e.expected_keys() / want - 1.0).abs() < 0.002,
            "{}",
            e.expected_keys()
        );
        // A much easier extra pattern hardly matters.
        let e = Estimate::new(&matcher(&["shop", "a"], &[]), Some(1));
        assert!((e.expected_keys() / 32f64.powi(4) - 1.0).abs() < 0.002);
        // Two of each: the mean of the maximum of two Gamma(2) variables is
        // 2.75 / p.
        let e = Estimate::new(&matcher(&["shop", "blog"], &[]), Some(2));
        assert!((e.expected_keys() / (2.75 * 32f64.powi(4)) - 1.0).abs() < 0.002);
    }

    #[test]
    fn chances() {
        for (patterns, count) in [
            (&["shop"][..], Some(1)),
            (&["shop"][..], Some(3)),
            (&["shop", "blog", "news"][..], Some(1)),
            (&["shop", "ab"][..], Some(5)),
            (&["shop", "ab"][..], None),
        ] {
            let e = Estimate::new(&matcher(patterns, &[]), count);
            for chance in [0.1, 0.5, 0.9, 0.99] {
                let keys = e.keys_for_chance(chance);
                assert!(
                    (e.chance_after(keys) - chance).abs() < 1e-6,
                    "{patterns:?} {count:?} {chance}"
                );
            }
            assert_eq!(e.chance_after(0.0), 0.0);
        }
        // Three patterns, each needed once: (1 - e^-pK)^3 = 0.9.
        let e = Estimate::new(&matcher(&["shop", "blog", "news"], &[]), Some(1));
        let p = 32f64.powi(-4);
        let want = -(1.0 - 0.9f64.powf(1.0 / 3.0)).ln() / p;
        assert!((e.keys_for_chance(0.9) / want - 1.0).abs() < 1e-3);
        // Poisson tail, exact and approximated.
        assert!((poisson_at_least(2, 1.0) - (1.0 - 2.0 / 1f64.exp())).abs() < 1e-12);
        assert!((poisson_at_least(5000, 5000.0) - 0.5038).abs() < 0.001);
        assert!(poisson_at_least(1000, 1e5) > 0.999_999);
        assert!(poisson_at_least(1000, 10.0) < 1e-12);
    }

    #[test]
    fn formatting() {
        assert_eq!(format_duration(0.2), "under a second");
        assert_eq!(format_duration(1.0), "1 second");
        assert_eq!(format_duration(42.7), "42 seconds");
        assert_eq!(format_duration(252.0), "4 min 12 s");
        assert_eq!(format_duration(3700.0), "1 h 1 min");
        assert_eq!(format_duration(90_000.0), "1 day 1 h");
        assert_eq!(format_duration(20.0 * 86_400.0), "20 days");
        assert_eq!(format_duration(100.0 * 86_400.0), "3 months");
        assert_eq!(format_duration(59.9 * 86_400.0), "59 days");
        assert_eq!(format_duration(60.0 * 86_400.0), "2 months");
        assert_eq!(format_duration(5.0 * 365.25 * 86_400.0), "5 years");
        assert_eq!(format_duration(2e6 * 365.25 * 86_400.0), "2.0M years");
        assert_eq!(format_duration(f64::INFINITY), "unknown");
        assert_eq!(format_count(950.0), "950");
        assert_eq!(format_count(12_345.0), "12.3K");
        assert_eq!(format_count(1_048_576.0), "1.0M");
        assert_eq!(format_count(345e9), "345B");
        assert_eq!(format_count(999.6), "1.0K");
        assert_eq!(format_count(99_960.0), "100K");
        assert_eq!(format_count(999_600.0), "1.0M");
        assert_eq!(format_count(999_999_999.0), "1.0B");
        assert_eq!(format_count(2e18), "2.0e18");
        assert_eq!(format_count(f64::NAN), "unknown");
        assert_eq!(format_rate(5.3e6), "5.3M keys/s");
        assert_eq!(format_odds(1.0 / 1_048_576.0), "1 in 1.0M");
    }
}

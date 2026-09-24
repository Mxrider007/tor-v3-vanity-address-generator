//! Difficulty and time estimates.
//!
//! Every key is an independent trial, so the number of keys needed to find a
//! pattern with per-key probability `p` is geometric: on average `1/p`, with
//! a 50% chance within `ln(2)/p` and a 90% chance within `ln(10)/p`. There is
//! no such thing as "percent complete" — only the chance of having found it.

use crate::pattern::Matcher;

/// How hard a search is.
#[derive(Debug, Clone, PartialEq)]
pub struct Estimate {
    /// Per-key match probability for each pattern (including "contains").
    pub per_pattern: Vec<f64>,
    /// Addresses wanted per pattern (`None` = unlimited).
    pub count: Option<u64>,
}

impl Estimate {
    pub fn new(matcher: &Matcher, count: Option<u64>) -> Estimate {
        Estimate {
            per_pattern: (0..matcher.patterns().len())
                .map(|i| matcher.probability(i))
                .collect(),
            count,
        }
    }

    /// Match probability of the rarest pattern (the one that decides how long
    /// the whole search takes).
    pub fn hardest(&self) -> f64 {
        self.per_pattern.iter().copied().fold(1.0, f64::min)
    }

    /// Expected number of keys until the search is done. With an unlimited
    /// count this is the expected number of keys per address found.
    pub fn expected_keys(&self) -> f64 {
        match self.count {
            Some(n) => n as f64 / self.hardest(),
            None => 1.0 / self.per_pattern.iter().sum::<f64>(),
        }
    }

    /// Keys needed to find the rarest pattern once with the given chance
    /// (e.g. `0.5` or `0.9`).
    pub fn keys_for_chance(&self, chance: f64) -> f64 {
        let p = self.hardest();
        if p >= 1.0 {
            return 1.0;
        }
        (-chance).ln_1p() / (-p).ln_1p()
    }

    /// Chance that the rarest pattern has been found at least once after
    /// checking `keys` keys.
    pub fn chance_after(&self, keys: f64) -> f64 {
        let p = self.hardest();
        if p >= 1.0 {
            return 1.0;
        }
        -(keys * (-p).ln_1p()).exp_m1()
    }
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
        plural((s / (30.44 * DAY)).floor(), "month")
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
    if !n.is_finite() {
        return "∞".to_string();
    }
    const UNITS: [(f64, &str); 5] = [(1e15, "Q"), (1e12, "T"), (1e9, "B"), (1e6, "M"), (1e3, "K")];
    if n >= 1e18 {
        return format!("{n:.1e}");
    }
    for (scale, suffix) in UNITS {
        if n >= scale {
            let v = n / scale;
            return if v >= 100.0 {
                format!("{v:.0}{suffix}")
            } else {
                format!("{v:.1}{suffix}")
            };
        }
    }
    format!("{n:.0}")
}

/// Key rate such as `"5.3M keys/s"`.
pub fn format_rate(keys_per_second: f64) -> String {
    format!("{} keys/s", format_count(keys_per_second))
}

/// Odds such as `"1 in 1.0M"`.
pub fn format_odds(p: f64) -> String {
    if p <= 0.0 {
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
        let e = Estimate::new(&matcher(&["kore"], &[]), Some(1));
        assert_eq!(e.expected_keys(), 32f64.powi(4));
        // The rarest pattern dominates.
        let e = Estimate::new(&matcher(&["ab", "abcde"], &[]), Some(2));
        assert_eq!(e.expected_keys(), 2.0 * 32f64.powi(5));
        // Unlimited: expected keys per address found across all patterns.
        let e = Estimate::new(&matcher(&["a", "b"], &[]), None);
        assert_eq!(e.expected_keys(), 16.0);
        // "contains" makes it rarer.
        let plain = Estimate::new(&matcher(&["ab"], &[]), Some(1)).expected_keys();
        let with = Estimate::new(&matcher(&["ab"], &["xyz"]), Some(1)).expected_keys();
        assert!(with > plain * 500.0);
    }

    #[test]
    fn chances() {
        let e = Estimate::new(&matcher(&["kore"], &[]), Some(1));
        let median = e.keys_for_chance(0.5);
        assert!((e.chance_after(median) - 0.5).abs() < 1e-9);
        assert!((e.chance_after(e.keys_for_chance(0.9)) - 0.9).abs() < 1e-9);
        assert_eq!(e.chance_after(0.0), 0.0);
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
        assert_eq!(format_duration(5.0 * 365.25 * 86_400.0), "5 years");
        assert_eq!(format_duration(2e6 * 365.25 * 86_400.0), "2.0M years");
        assert_eq!(format_duration(f64::INFINITY), "unknown");
        assert_eq!(format_count(950.0), "950");
        assert_eq!(format_count(12_345.0), "12.3K");
        assert_eq!(format_count(1_048_576.0), "1.0M");
        assert_eq!(format_count(345e9), "345B");
        assert_eq!(format_rate(5.3e6), "5.3M keys/s");
        assert_eq!(format_odds(1.0 / 1_048_576.0), "1 in 1.0M");
    }
}

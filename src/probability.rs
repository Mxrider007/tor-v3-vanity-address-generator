//! Probability calculations for vanity address generation
//!
//! This module provides functions to calculate the expected time and probability
//! of finding a vanity address with specific prefix and contains requirements.

/// Onion v3 address length (without .onion suffix)
pub const ONION_ADDRESS_LENGTH: usize = 56;

/// Base32 alphabet size
pub const BASE32_SIZE: f64 = 32.0;

/// Statistics for a vanity search
#[derive(Debug, Clone)]
pub struct SearchStats {
    /// Probability of finding a match per key
    pub probability_per_key: f64,
    /// Expected number of keys to check
    pub expected_keys: f64,
    /// Estimated time in seconds at given speed
    pub estimated_seconds: f64,
    /// Human-readable time estimate
    pub time_estimate: String,
    /// Breakdown of individual probabilities
    pub breakdown: ProbabilityBreakdown,
}

/// Breakdown of probability components
#[derive(Debug, Clone)]
pub struct ProbabilityBreakdown {
    /// Probability of matching the prefix
    pub prefix_probability: f64,
    /// Probability of matching all contains words (given prefix matches)
    pub contains_probability: f64,
    /// Number of possible positions for contains matches
    pub contains_positions: Vec<usize>,
}

/// Calculate search statistics for given parameters
/// prefix_len is the number of fixed (non-wildcard) characters in the longest prefix pattern
pub fn calculate_stats(
    prefix_len: usize,
    contains_words: &[String],
    keys_per_second: f64,
) -> SearchStats {
    let breakdown = calculate_breakdown(prefix_len, contains_words);

    // A probability is at most 1.0; the contains approximation (union bound) can
    // otherwise overshoot for very short words and make expected_keys < 1.
    let probability_per_key =
        (breakdown.prefix_probability * breakdown.contains_probability).min(1.0);
    let expected_keys = if probability_per_key > 0.0 {
        1.0 / probability_per_key
    } else {
        f64::INFINITY
    };

    let estimated_seconds = if keys_per_second > 0.0 {
        expected_keys / keys_per_second
    } else {
        f64::INFINITY
    };

    let time_estimate = format_duration_human(estimated_seconds);

    SearchStats {
        probability_per_key,
        expected_keys,
        estimated_seconds,
        time_estimate,
        breakdown,
    }
}

/// Calculate effective prefix length (non-wildcard characters only)
pub fn effective_prefix_len(prefixes: &[String]) -> usize {
    prefixes.iter().map(|p| count_fixed_chars(p)).max().unwrap_or(0)
}

/// Count non-wildcard characters in a pattern (wildcards don't affect probability)
fn count_fixed_chars(pattern: &str) -> usize {
    pattern.chars().filter(|c| *c != '?').count()
}

/// Calculate probability breakdown
fn calculate_breakdown(prefix_len: usize, contains_words: &[String]) -> ProbabilityBreakdown {
    // Prefix probability: 1/32^prefix_len
    // Note: this takes the full pattern length, wildcards should be accounted for by caller
    let prefix_probability = if prefix_len > 0 {
        1.0 / BASE32_SIZE.powi(prefix_len as i32)
    } else {
        1.0
    };

    // Contains probability: for each word, ~(positions) / 32^(non-wildcard chars)
    // Multiple words: multiply probabilities (assuming independence)
    let mut contains_probability = 1.0;
    let mut contains_positions = Vec::new();

    for word in contains_words {
        let word_len = word.len();
        let fixed_chars = count_fixed_chars(word);
        if word_len > 0 && word_len <= ONION_ADDRESS_LENGTH {
            // Number of positions where the word can appear
            let positions = ONION_ADDRESS_LENGTH - word_len + 1;
            contains_positions.push(positions);

            // Probability word appears at any position (approximate, ignoring overlaps)
            // Only fixed (non-wildcard) characters affect probability. Clamp to 1.0
            // since the union-bound estimate can exceed 1 for short words.
            let word_probability =
                ((positions as f64) / BASE32_SIZE.powi(fixed_chars as i32)).min(1.0);
            contains_probability *= word_probability;
        }
    }

    ProbabilityBreakdown {
        prefix_probability,
        contains_probability,
        contains_positions,
    }
}

/// Calculate live progress statistics
pub fn calculate_progress(
    keys_checked: u64,
    keys_per_second: f64,
    prefix_len: usize,
    contains_words: &[String],
) -> LiveStats {
    let stats = calculate_stats(prefix_len, contains_words, keys_per_second);

    // Progress percentage (capped at 99.9% since it's probabilistic)
    let progress_percent = if stats.expected_keys > 0.0 {
        ((keys_checked as f64 / stats.expected_keys) * 100.0).min(99.9)
    } else {
        0.0
    };

    // Cumulative probability of having found a match by now
    // P(found) = 1 - (1 - p)^n ≈ 1 - e^(-n*p) for small p
    let cumulative_probability = if stats.probability_per_key > 0.0 {
        1.0 - (-(keys_checked as f64) * stats.probability_per_key).exp()
    } else {
        0.0
    };

    // Estimated time remaining
    let remaining_keys = if stats.expected_keys > keys_checked as f64 {
        stats.expected_keys - keys_checked as f64
    } else {
        stats.expected_keys // Still might not have found it
    };

    let remaining_seconds = if keys_per_second > 0.0 {
        remaining_keys / keys_per_second
    } else {
        f64::INFINITY
    };

    LiveStats {
        progress_percent,
        cumulative_probability,
        remaining_seconds,
        remaining_estimate: format_duration_human(remaining_seconds),
        expected_keys: stats.expected_keys,
        probability_per_key: stats.probability_per_key,
    }
}

/// Live statistics during mining
#[derive(Debug, Clone)]
pub struct LiveStats {
    /// Progress percentage (0-100)
    pub progress_percent: f64,
    /// Cumulative probability of having found a match
    pub cumulative_probability: f64,
    /// Estimated seconds remaining
    pub remaining_seconds: f64,
    /// Human-readable remaining time
    pub remaining_estimate: String,
    /// Expected total keys to check
    pub expected_keys: f64,
    /// Probability per key
    pub probability_per_key: f64,
}

/// Format seconds as human-readable duration
pub fn format_duration_human(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "unknown".to_string();
    }

    let seconds = seconds as u64;

    if seconds < 60 {
        return format!("{} sec", seconds);
    }

    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{} min", minutes);
    }

    let hours = minutes / 60;
    if hours < 24 {
        let mins = minutes % 60;
        return format!("{}h {}m", hours, mins);
    }

    let days = hours / 24;
    if days < 365 {
        let hrs = hours % 24;
        return format!("{}d {}h", days, hrs);
    }

    let years = days / 365;
    let remaining_days = days % 365;

    if years >= 100 {
        return format!("{} years", years);
    }

    format!("{}y {}d", years, remaining_days)
}

/// Format a probability as a human-readable string
pub fn format_probability(p: f64) -> String {
    if p >= 0.01 {
        format!("{:.1}%", p * 100.0)
    } else if p >= 0.0001 {
        format!("{:.2}%", p * 100.0)
    } else if p > 0.0 {
        let inverse = 1.0 / p;
        if inverse >= 1e15 {
            format!("1 in {:.1}Q", inverse / 1e15)
        } else if inverse >= 1e12 {
            format!("1 in {:.1}T", inverse / 1e12)
        } else if inverse >= 1e9 {
            format!("1 in {:.1}B", inverse / 1e9)
        } else if inverse >= 1e6 {
            format!("1 in {:.1}M", inverse / 1e6)
        } else if inverse >= 1e3 {
            format!("1 in {:.1}K", inverse / 1e3)
        } else {
            format!("1 in {:.0}", inverse)
        }
    } else {
        "0%".to_string()
    }
}

/// Format large numbers for display
pub fn format_number(n: f64) -> String {
    if n >= 1e18 {
        format!("{:.1}E", n / 1e18)
    } else if n >= 1e15 {
        format!("{:.1}Q", n / 1e15)
    } else if n >= 1e12 {
        format!("{:.1}T", n / 1e12)
    } else if n >= 1e9 {
        format!("{:.1}B", n / 1e9)
    } else if n >= 1e6 {
        format!("{:.1}M", n / 1e6)
    } else if n >= 1e3 {
        format!("{:.1}K", n / 1e3)
    } else {
        format!("{:.0}", n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prefix_probability() {
        // 4-char prefix: 1/32^4 = 1/1,048,576
        let stats = calculate_stats(4, &[], 1_000_000.0);
        assert!((stats.breakdown.prefix_probability - 1.0/1_048_576.0).abs() < 1e-10);
    }

    #[test]
    fn test_contains_probability() {
        // 5-char word in 56-char address: 52 positions, probability ≈ 52/32^5
        let stats = calculate_stats(0, &["block".to_string()], 1_000_000.0);
        let expected = 52.0 / 32.0_f64.powi(5);
        assert!((stats.breakdown.contains_probability - expected).abs() < 1e-10);
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration_human(30.0), "30 sec");
        assert_eq!(format_duration_human(90.0), "1 min");
        assert_eq!(format_duration_human(3700.0), "1h 1m");
        assert_eq!(format_duration_human(90000.0), "1d 1h");
    }
}

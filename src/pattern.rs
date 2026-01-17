//! Pattern matching with wildcard support
//!
//! Supports `?` as a single-character wildcard.
//! Example patterns:
//! - `kore` - matches "kore" at position 0
//! - `????kore` - matches "kore" at position 4
//! - `kore????test` - matches "kore" at start and "test" at position 8

/// Check if text matches pattern at the beginning (for prefix matching)
/// The `?` character matches any single character.
pub fn matches_prefix(text: &str, pattern: &str) -> bool {
    let text_chars: Vec<char> = text.to_lowercase().chars().collect();
    let pattern_chars: Vec<char> = pattern.to_lowercase().chars().collect();

    if text_chars.len() < pattern_chars.len() {
        return false;
    }

    for (i, p_char) in pattern_chars.iter().enumerate() {
        if *p_char != '?' && text_chars[i] != *p_char {
            return false;
        }
    }

    true
}

/// Check if pattern appears anywhere in text (for contains matching)
/// The `?` character matches any single character.
pub fn contains_pattern(text: &str, pattern: &str) -> bool {
    let text_lower = text.to_lowercase();
    let pattern_lower = pattern.to_lowercase();

    // If pattern has no wildcards, use simple contains
    if !pattern_lower.contains('?') {
        return text_lower.contains(&pattern_lower);
    }

    let text_chars: Vec<char> = text_lower.chars().collect();
    let pattern_chars: Vec<char> = pattern_lower.chars().collect();

    if pattern_chars.is_empty() {
        return true;
    }

    if text_chars.len() < pattern_chars.len() {
        return false;
    }

    // Try matching pattern at each position
    for start in 0..=(text_chars.len() - pattern_chars.len()) {
        let mut matches = true;
        for (i, p_char) in pattern_chars.iter().enumerate() {
            if *p_char != '?' && text_chars[start + i] != *p_char {
                matches = false;
                break;
            }
        }
        if matches {
            return true;
        }
    }

    false
}

/// Check if a pattern is valid for base32 onion addresses
/// Allows a-z, 2-7, and ? as wildcard
pub fn is_valid_pattern(pattern: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }

    for c in pattern.to_lowercase().chars() {
        match c {
            'a'..='z' | '2'..='7' | '?' => continue,
            _ => return false,
        }
    }

    true
}

/// Extract the non-wildcard prefix from a pattern (for CUDA tool)
/// e.g., "????kore" -> "????kore" (passed as-is since CUDA supports it)
/// e.g., "kore" -> "kore"
pub fn get_cuda_pattern(pattern: &str) -> String {
    pattern.to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_prefix() {
        assert!(matches_prefix("koreabcdef", "kore"));
        assert!(matches_prefix("abcdkore", "????kore"));
        assert!(matches_prefix("xyzzkoretest", "????kore"));
        assert!(!matches_prefix("koreabc", "????kore")); // kore at pos 0, not 4
        assert!(matches_prefix("anything", "????????"));
        assert!(matches_prefix("KORE", "kore")); // case insensitive
    }

    #[test]
    fn test_contains_pattern() {
        assert!(contains_pattern("abckoredef", "kore"));
        assert!(contains_pattern("testkoremore", "kore"));
        assert!(contains_pattern("xxkoreyy", "??kore"));
        assert!(contains_pattern("abcXYZdef", "???xyz"));
        assert!(!contains_pattern("abc", "abcd"));
    }

    #[test]
    fn test_is_valid_pattern() {
        assert!(is_valid_pattern("kore"));
        assert!(is_valid_pattern("????kore"));
        assert!(is_valid_pattern("abc234567"));
        assert!(!is_valid_pattern("kore!")); // invalid char
        assert!(!is_valid_pattern("kore8")); // 8 not in base32
        assert!(!is_valid_pattern("")); // empty
    }
}

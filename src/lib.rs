//! Kore Vanity Address Generator Library
//!
//! This library provides functionality to generate vanity onion addresses
//! with both CUDA GPU acceleration and CPU fallback support.

pub mod backend;
pub mod onion;
pub mod pattern;
pub mod probability;

pub use backend::{
    select_backend, select_backend_with_mode, select_backend_with_config,
    Backend, BackendInfo, BackendMode, GeneratorError, FoundKey, Progress, SearchFilter, format_speed,
};
pub use onion::pubkey_to_onion;
pub use probability::{
    calculate_stats, calculate_progress, format_duration_human, format_probability, format_number,
    effective_prefix_len, SearchStats, LiveStats, ProbabilityBreakdown,
};
pub use pattern::{matches_prefix, contains_pattern, is_valid_pattern};

/// File prefix for Tor ed25519 secret key files
pub const FILE_PREFIX: &[u8] = b"== ed25519v1-secret: type0 ==\0\0\0";

/// File prefix for Tor ed25519 public key files
pub const PUBKEY_PREFIX: &[u8] = b"== ed25519v1-public: type0 ==\0\0\0";

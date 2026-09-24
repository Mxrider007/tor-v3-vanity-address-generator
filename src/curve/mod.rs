//! Minimal Ed25519 arithmetic for the key search hot loop.
//!
//! Field operations come from fiat-crypto (formally verified); the point
//! formulas mirror curve25519-dalek and are tested against it. Anything that
//! ends up on disk is re-derived with curve25519-dalek before it is written,
//! so a bug here could at worst make the search miss a match — never produce
//! a wrong key.

pub(crate) mod field;
pub(crate) mod point;
pub(crate) mod walk;

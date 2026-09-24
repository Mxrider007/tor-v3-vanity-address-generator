//! # kore-vanity
//!
//! Fast generator for Tor v3 `.onion` vanity addresses.
//!
//! ```no_run
//! use kore_vanity::pattern::{Matcher, Pattern};
//! use kore_vanity::search::{Event, GpuMode, Search, SearchConfig};
//! use std::time::Duration;
//!
//! let matcher = Matcher::new(vec![Pattern::parse("kore")?], false, vec![])?;
//! let search = Search::start(SearchConfig {
//!     matcher,
//!     count: Some(1),
//!     output_dir: "keys".into(),
//!     cpu_threads: kore_vanity::search::available_threads(),
//!     gpu: GpuMode::Auto,
//! })?;
//! loop {
//!     match search.next_event(Duration::from_secs(1)) {
//!         Some(Event::Found(found)) => println!("{} -> {}", found.address, found.path.display()),
//!         Some(Event::Finished) => break,
//!         _ => {}
//!     }
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Modules:
//! * [`pattern`] — parsing and matching of search patterns;
//! * [`search`] — the multi-threaded CPU/GPU search engine;
//! * [`onion`] — onion addresses and Tor key files;
//! * [`estimate`] — probability and time estimates.

#![forbid(unsafe_code)]

mod curve;
pub mod estimate;
pub mod onion;
pub mod pattern;
pub mod search;

/// The crate version, e.g. `"1.0.0"`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

//! Kore Vanity Address Generator CLI
//!
//! A high-performance vanity address generator with GPU acceleration and CPU fallback.

use clap::{Parser, ValueEnum};
use crossbeam_channel::unbounded;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use kore_vanity::backend::{select_backend_with_mode, BackendMode, Progress, SearchFilter};
use kore_vanity::probability::{calculate_stats, calculate_progress, format_probability, format_number};

#[derive(Parser)]
#[command(name = "kore-vanity")]
#[command(about = "Vanity onion address generator with GPU acceleration")]
#[command(version)]
struct Cli {
    /// Desired prefixes (comma-separated)
    #[arg(required = true, value_delimiter = ',')]
    prefixes: Vec<String>,

    /// Output directory for generated keys
    #[arg(short, long, default_value = ".")]
    dst: PathBuf,

    /// Backend mode
    #[arg(short, long, value_enum, default_value = "auto")]
    mode: Mode,

    /// Number of CPU threads (only used in cpu and hybrid modes)
    #[arg(short = 't', long, default_value_t = num_cpus::get())]
    threads: usize,

    /// Words that must also appear in the address (in addition to prefix)
    /// Can be specified multiple times: --contains block --contains chain
    #[arg(short = 'c', long = "contains", value_name = "WORD")]
    contains: Vec<String>,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum Mode {
    /// Automatically select best backend (Hybrid > CUDA > CPU)
    Auto,
    /// CPU only
    Cpu,
    /// CUDA GPU only
    #[cfg(feature = "cuda")]
    Cuda,
    /// Hybrid CPU + GPU (maximum speed)
    #[cfg(feature = "cuda")]
    Hybrid,
}

impl From<Mode> for BackendMode {
    fn from(mode: Mode) -> Self {
        match mode {
            Mode::Auto => BackendMode::Auto,
            Mode::Cpu => BackendMode::Cpu,
            #[cfg(feature = "cuda")]
            Mode::Cuda => BackendMode::Cuda,
            #[cfg(feature = "cuda")]
            Mode::Hybrid => BackendMode::Hybrid,
        }
    }
}

fn main() {
    let cli = Cli::parse();

    // Validate output directory
    if !cli.dst.is_dir() {
        eprintln!("Error: '{}' is not a directory", cli.dst.display());
        std::process::exit(1);
    }

    // Validate prefixes
    let max_len = cli.prefixes.iter().map(|p| p.len()).max().unwrap_or(0);
    for prefix in &cli.prefixes {
        if prefix.is_empty() {
            eprintln!("Error: Empty prefix not allowed");
            std::process::exit(1);
        }
        // Check if valid base32
        if base32::decode(
            base32::Alphabet::Rfc4648Lower { padding: false },
            &format!("{}aa", prefix),
        )
        .is_none()
        {
            eprintln!("Error: '{}' is not a valid base32 prefix", prefix);
            std::process::exit(1);
        }
    }

    println!("=== Kore Vanity Generator ===");
    println!("Prefixes: {:?}", cli.prefixes);
    if !cli.contains.is_empty() {
        println!("Must contain: {:?}", cli.contains);
    }
    println!("Output: {}", cli.dst.display());
    println!("CPU threads: {}", cli.threads);
    println!();

    // Select backend
    let backend = select_backend_with_mode(cli.mode.into());
    let info = backend.info();

    // Show initial probability estimate
    println!("--- Probability Estimate ---");
    let initial_stats = calculate_stats(max_len, &cli.contains, info.estimated_speed as f64);
    println!("Prefix length: {} chars", max_len);
    if !cli.contains.is_empty() {
        println!("Contains words: {:?}", cli.contains);
    }
    println!("Probability per key: {}", format_probability(initial_stats.probability_per_key));
    println!("Expected keys to check: {}", format_number(initial_stats.expected_keys));
    println!("Estimated time @ {:.1}M/s: {}",
        info.estimated_speed as f64 / 1_000_000.0,
        initial_stats.time_estimate
    );
    println!("----------------------------");
    println!();
    println!("Starting generation...");
    println!();

    // Set up channels
    let (progress_tx, progress_rx) = unbounded::<Progress>();
    let (result_tx, result_rx) = unbounded();
    let (stop_tx, stop_rx) = crossbeam_channel::bounded(1);

    // Handle Ctrl+C
    let stop_tx_clone = stop_tx.clone();
    ctrlc::set_handler(move || {
        eprintln!("\nStopping...");
        let _ = stop_tx_clone.send(());
    })
    .ok();

    // Clone values for threads
    let prefixes = cli.prefixes.clone();
    let dst = cli.dst.clone();
    let filter = SearchFilter {
        contains: cli.contains.clone(),
    };
    let contains_for_stats = cli.contains.clone();

    // Spawn generation thread
    let gen_handle = std::thread::spawn(move || {
        backend.generate_with_filter(prefixes, dst, progress_tx, result_tx, stop_rx, filter)
    });

    // Progress display
    let start_time = Instant::now();
    let mut last_log = Instant::now();
    let mut found_count = 0;
    let total_prefixes = cli.prefixes.len();

    loop {
        // Check for results
        while let Ok(result) = result_rx.try_recv() {
            found_count += 1;
            println!();
            println!(
                "*** FOUND [{}/{}]: {} -> {} ***",
                found_count, total_prefixes, result.prefix, result.onion_address
            );
            println!("    Saved to: {}", result.key_path.display());
            println!();
        }

        // Check for progress
        if let Ok(progress) = progress_rx.try_recv() {
            if last_log.elapsed() > Duration::from_secs(10) {
                // Calculate live statistics
                let live_stats = calculate_progress(
                    progress.keys_checked,
                    progress.keys_per_sec,
                    max_len,
                    &contains_for_stats,
                );

                let elapsed_secs = start_time.elapsed().as_secs();
                let elapsed_str = format_elapsed(elapsed_secs);

                println!("┌─────────────────────────────────────────────────┐");
                println!("│ Keys checked: {:>15} ({:>6.2}%)       │",
                    format_number(progress.keys_checked as f64),
                    live_stats.progress_percent
                );
                println!("│ Speed: {:>22.2} M/sec          │", progress.keys_per_sec / 1_000_000.0);
                println!("│ Elapsed: {:>20}              │", elapsed_str);
                println!("│ Est. remaining: {:>14}              │", live_stats.remaining_estimate);
                println!("│ Cumulative probability: {:>10}         │",
                    format!("{:.1}%", live_stats.cumulative_probability * 100.0)
                );
                if progress.prefix_matches > 0 {
                    println!("│ Prefix matches: {:>14} (rejected: {:<5})│",
                        progress.prefix_matches,
                        progress.contains_rejects
                    );
                }
                println!("│ Found: {}/{}                                     │", found_count, total_prefixes);
                println!("└─────────────────────────────────────────────────┘");
                println!();

                last_log = Instant::now();
            }
        }

        // Check if generation is done
        if found_count >= total_prefixes {
            break;
        }

        // Small sleep to prevent busy loop
        std::thread::sleep(Duration::from_millis(50));

        // Check if generator thread has finished
        if gen_handle.is_finished() {
            break;
        }
    }

    // Wait for generator
    match gen_handle.join() {
        Ok(Ok(())) => {
            let elapsed = start_time.elapsed().as_secs();
            println!();
            println!("=== Complete! ===");
            println!("Found all {} prefixes in {}", total_prefixes, format_elapsed(elapsed));
        }
        Ok(Err(e)) => {
            eprintln!();
            eprintln!("Generation stopped: {}", e);
        }
        Err(_) => {
            eprintln!();
            eprintln!("Generation thread panicked");
        }
    }
}

fn format_elapsed(seconds: u64) -> String {
    if seconds < 60 {
        format!("{}s", seconds)
    } else if seconds < 3600 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else if seconds < 86400 {
        format!("{}h {}m", seconds / 3600, (seconds % 3600) / 60)
    } else {
        format!("{}d {}h", seconds / 86400, (seconds % 86400) / 3600)
    }
}

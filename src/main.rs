//! `torv3-vanity` — command-line Tor v3 vanity address generator.

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use clap::{Parser, ValueEnum};

use torv3_vanity::estimate::{Estimate, format_count, format_duration, format_odds, format_rate};
use torv3_vanity::onion::verify_hidden_service_dir;
use torv3_vanity::pattern::{Matcher, Pattern, PatternError};
use torv3_vanity::search::{
    Event, GpuMode, RateMeter, Search, SearchConfig, available_threads, list_gpus,
};

/// `eprintln!` that ignores errors: a closed stderr must not crash a search.
macro_rules! say {
    ($($arg:tt)*) => {{
        let _ = writeln!(std::io::stderr(), $($arg)*);
    }};
}

const EXAMPLES: &str = "\
Examples:
  torv3-vanity shop                   address starting with \"shop\"
  torv3-vanity shop -o keys           ...saved in the folder \"keys\"
  torv3-vanity shop blog news         several patterns at once
  torv3-vanity \"sh?p\"                 ? matches any single character
  torv3-vanity shop -n 5              find 5 addresses to choose from
  torv3-vanity shop --anywhere        \"shop\" anywhere in the address
  torv3-vanity shop -c blog           starts with \"shop\" and contains \"blog\"
  torv3-vanity shop --estimate        how long will it take?
  torv3-vanity --verify keys/shop...  check a saved key folder

Each extra character makes the search 32 times longer. Keep the
hs_ed25519_secret_key file private: anyone who has it controls your address.";

#[derive(Parser)]
#[command(
    name = "torv3-vanity",
    version,
    about = "Generate Tor v3 .onion vanity addresses on the CPU and GPU.",
    after_help = EXAMPLES
)]
struct Cli {
    /// Pattern(s) to search for: letters a-z and digits 2-7, with ? matching any
    /// character. Separate several patterns with spaces or commas.
    #[arg(value_name = "PATTERN", required_unless_present_any = ["verify", "list_gpus", "benchmark"])]
    patterns: Vec<String>,

    /// Folder to save the keys in (created if needed).
    #[arg(
        short,
        long,
        value_name = "DIR",
        default_value = ".",
        visible_alias = "dst",
        short_alias = 'd'
    )]
    output: PathBuf,

    /// Number of addresses to find for each pattern (0 = keep going until stopped).
    #[arg(short = 'n', long, value_name = "N", default_value_t = 1)]
    count: u64,

    /// Match patterns anywhere in the address instead of only at the start.
    #[arg(short, long)]
    anywhere: bool,

    /// Also require this text somewhere in the address (can be repeated).
    #[arg(short, long = "contains", value_name = "TEXT")]
    contains: Vec<String>,

    /// Number of CPU threads to use [default: all cores].
    #[arg(short, long, value_name = "N")]
    threads: Option<usize>,

    /// Whether to use the graphics card.
    #[arg(short, long, value_enum, default_value_t = GpuArg::Auto)]
    gpu: GpuArg,

    /// Show how long the search would take, then exit.
    #[arg(long)]
    estimate: bool,

    /// Measure the key rate for SECONDS (default 10), then exit.
    #[arg(long, value_name = "SECONDS", num_args = 0..=1, default_missing_value = "10")]
    benchmark: Option<u64>,

    /// List the graphics cards that can be used, then exit.
    #[arg(long)]
    list_gpus: bool,

    /// Check a saved key folder (the one containing hs_ed25519_secret_key).
    #[arg(long, value_name = "DIR", conflicts_with_all = ["patterns", "benchmark", "estimate"])]
    verify: Option<PathBuf>,

    /// Only print the addresses found, one per line.
    #[arg(short, long)]
    quiet: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum GpuArg {
    /// Use a dedicated graphics card (or Apple Silicon) if there is one.
    Auto,
    /// Use any available GPU, including integrated graphics.
    On,
    /// Use only the CPU.
    Off,
}

impl From<GpuArg> for GpuMode {
    fn from(g: GpuArg) -> GpuMode {
        match g {
            GpuArg::Auto => GpuMode::Auto,
            GpuArg::On => GpuMode::On,
            GpuArg::Off => GpuMode::Off,
        }
    }
}

fn main() -> ExitCode {
    if std::env::args_os().len() == 1 && console::own_window() {
        println!("torv3-vanity is a command-line program.");
        println!("For the graphical version, open torv3-vanity-gui.exe instead.\n");
        println!("To use it, open a terminal (PowerShell) in this folder and run for example:\n");
        println!("    .\\torv3-vanity.exe shop\n");
        println!("Run  .\\torv3-vanity.exe --help  to see all options.");
        console::pause();
        return ExitCode::SUCCESS;
    }

    let cli = Cli::parse();
    let code = run(&cli).unwrap_or_else(|message| {
        say!("error: {message}");
        ExitCode::FAILURE
    });
    if !cli.quiet && console::own_window() {
        console::pause();
    }
    code
}

fn run(cli: &Cli) -> Result<ExitCode, String> {
    if let Some(dir) = &cli.verify {
        return verify(dir);
    }
    if cli.list_gpus {
        return Ok(print_gpus());
    }

    let threads = cli.threads.unwrap_or_else(available_threads);
    if let Some(seconds) = cli.benchmark {
        return benchmark(cli, threads, Duration::from_secs(seconds.max(1)));
    }

    let matcher = build_matcher(cli).map_err(|e| e.to_string())?;
    let count = (cli.count > 0).then_some(cli.count);
    let estimate = Estimate::new(&matcher, count);

    if !cli.quiet {
        print_header(cli, &matcher, &estimate);
    }
    if cli.estimate {
        let rate = measure_rate(
            threads,
            cli.gpu.into(),
            matcher.anywhere(),
            Duration::from_secs(5),
        )?;
        print_time_estimate(&estimate, rate);
        return Ok(ExitCode::SUCCESS);
    }

    let search = Search::start(SearchConfig {
        matcher,
        count,
        output_dir: cli.output.clone(),
        cpu_threads: threads,
        gpu: cli.gpu.into(),
    })
    .map_err(|e| e.to_string())?;
    install_ctrlc(&search);

    let outcome = monitor(&search, cli, &estimate, threads);
    let stats = search.stats();
    let rate = stats.keys_checked as f64 / stats.elapsed.as_secs_f64().max(1e-9);
    if !cli.quiet {
        say!();
        let summary = format!(
            "{} keys in {} ({})",
            format_count(stats.keys_checked as f64),
            format_duration(stats.elapsed.as_secs_f64()),
            format_rate(rate)
        );
        match outcome {
            Outcome::Complete => say!("Done: found {} address(es), {summary}.", stats.found),
            Outcome::Interrupted | Outcome::OutputClosed => {
                say!("Stopped: found {} address(es), {summary}.", stats.found)
            }
            Outcome::Failed => say!("The search stopped because of an error ({summary})."),
        }
        if stats.found > 0 {
            say!(
                "Keep each folder's hs_ed25519_secret_key private: anyone who has it controls that address."
            );
        }
    }
    Ok(match outcome {
        Outcome::Complete | Outcome::OutputClosed => ExitCode::SUCCESS,
        Outcome::Interrupted => ExitCode::from(130),
        Outcome::Failed => ExitCode::FAILURE,
    })
}

fn build_matcher(cli: &Cli) -> Result<Matcher, PatternError> {
    let mut patterns: Vec<Pattern> = Vec::new();
    for arg in &cli.patterns {
        for p in Pattern::parse_list(arg)? {
            if !patterns.contains(&p) {
                patterns.push(p);
            }
        }
    }
    let mut contains = Vec::new();
    for arg in &cli.contains {
        contains.extend(Pattern::parse_list(arg)?);
    }
    Matcher::new(patterns, cli.anywhere, contains)
}

fn print_header(cli: &Cli, matcher: &Matcher, estimate: &Estimate) {
    say!("torv3-vanity {}", torv3_vanity::VERSION);
    let place = if matcher.anywhere() {
        "anywhere in the address"
    } else {
        "at the start of the address"
    };
    let list: Vec<&str> = matcher.patterns().iter().map(Pattern::as_str).collect();
    say!("Searching for: {} ({place})", list.join(", "));
    if !matcher.contains().is_empty() {
        let words: Vec<&str> = matcher.contains().iter().map(Pattern::as_str).collect();
        say!("Also containing: {}", words.join(", "));
    }
    match estimate.count() {
        Some(1) => {}
        Some(n) => say!("Addresses per pattern: {n}"),
        None => say!("Addresses per pattern: unlimited (press Ctrl+C to stop)"),
    }
    let per = if estimate.count().is_some() {
        "keys expected"
    } else {
        "keys per address"
    };
    say!(
        "Difficulty: {} per key, about {} {per}",
        format_odds(estimate.hardest()),
        format_count(estimate.expected_keys())
    );
    if !cli.estimate {
        let output = std::path::absolute(&cli.output).unwrap_or_else(|_| cli.output.clone());
        say!("Saving to: {}", output.display());
    }
}

fn print_time_estimate(estimate: &Estimate, rate: f64) {
    say!("Measured speed: {}", format_rate(rate));
    let time = |keys: f64| format_duration(keys / rate);
    if estimate.count().is_some() {
        say!("Expected time:  {}", time(estimate.expected_keys()));
        say!("50% chance within {}", time(estimate.keys_for_chance(0.5)));
        say!("90% chance within {}", time(estimate.keys_for_chance(0.9)));
        say!("99% chance within {}", time(estimate.keys_for_chance(0.99)));
    } else {
        say!(
            "Expected time per address: {}",
            time(estimate.expected_keys())
        );
    }
    say!("Each extra character multiplies these times by 32.");
}

enum Outcome {
    Complete,
    Interrupted,
    Failed,
    /// Whoever read the addresses (for example `head`) went away.
    OutputClosed,
}

/// Print a found address on stdout.
fn print_found(found: &torv3_vanity::search::Found, quiet: bool) -> std::io::Result<()> {
    let mut out = std::io::stdout().lock();
    if quiet {
        writeln!(out, "{}", found.address)?;
    } else {
        writeln!(
            out,
            "Found {}  (pattern \"{}\")",
            found.address, found.pattern
        )?;
        writeln!(out, "      saved in {}", found.path.display())?;
    }
    out.flush()
}

fn monitor(search: &Search, cli: &Cli, estimate: &Estimate, threads: usize) -> Outcome {
    let tty = std::io::stderr().is_terminal();
    let draw_every = if tty {
        Duration::from_secs(1)
    } else {
        Duration::from_secs(30)
    };
    let mut progress = ProgressLine::new(tty && !cli.quiet);
    let mut meter = RateMeter::new(Duration::from_secs(10));
    let mut last_draw = Instant::now();
    let mut failed = false;
    let mut output_closed = false;
    if !cli.quiet {
        say!("CPU threads: {threads}");
    }

    loop {
        let event = search.next_event(Duration::from_millis(250));
        match event {
            Some(Event::Found(found)) => {
                progress.clear();
                if !output_closed && print_found(&found, cli.quiet).is_err() {
                    // Nobody is reading any more: finish the keys being
                    // saved, then stop.
                    output_closed = true;
                    search.stop();
                }
            }
            Some(Event::Info(message)) => {
                progress.clear();
                if !cli.quiet {
                    say!("{message}");
                }
            }
            Some(Event::Warning(message)) => {
                progress.clear();
                say!("warning: {message}");
            }
            Some(Event::Error(message)) => {
                progress.clear();
                say!("error: {message}");
                failed = true;
            }
            Some(Event::Finished) => break,
            None => {}
        }

        if last_draw.elapsed() >= draw_every && !cli.quiet {
            last_draw = Instant::now();
            let stats = search.stats();
            let rate = meter.update(stats.keys_checked);
            progress.draw(&progress_line(&stats, rate, estimate));
        }
    }
    progress.clear();

    if failed {
        Outcome::Failed
    } else if output_closed {
        Outcome::OutputClosed
    } else if search.is_complete() {
        Outcome::Complete
    } else {
        Outcome::Interrupted
    }
}

/// The status line, kept within 79 columns so it can be redrawn in place:
/// less important parts are left out when it would not fit.
fn progress_line(stats: &torv3_vanity::search::Stats, rate: f64, estimate: &Estimate) -> String {
    const WIDTH: usize = 79;
    let mut line = format!(
        "[{}] {} keys, {}",
        clock(stats.elapsed),
        format_count(stats.keys_checked as f64),
        format_rate(rate)
    );
    let mut optional = Vec::new();
    if stats.gpu_keys_checked > 0 {
        let share = 100.0 * stats.gpu_keys_checked as f64 / stats.keys_checked.max(1) as f64;
        optional.push(format!(" ({share:.0}% GPU)"));
    }
    match estimate.total() {
        Some(_) if stats.found == 0 => {
            let chance = estimate.chance_after(stats.keys_checked as f64);
            line += &format!(", {:.0}% chance by now", chance * 100.0);
        }
        Some(total) => line += &format!(", found {}/{total}", stats.found),
        None => line += &format!(", found {}", stats.found),
    }
    if rate > 0.0 {
        let per = if estimate.count().is_some() {
            "expected"
        } else {
            "per address"
        };
        line += &format!(", {} {per}", about(estimate.expected_keys() / rate));
    }
    if stats.near_misses > 0 {
        optional.push(format!(
            ", {} near misses",
            format_count(stats.near_misses as f64)
        ));
    }
    // The GPU share goes right after the rate; near misses at the end.
    let mut gpu = String::new();
    for part in optional {
        if line.chars().count() + gpu.chars().count() + part.chars().count() > WIDTH {
            continue;
        }
        if part.starts_with(" (") {
            gpu = part;
        } else {
            line += &part;
        }
    }
    if !gpu.is_empty() {
        let at = line
            .find(" keys/s")
            .map_or(line.len(), |i| i + " keys/s".len());
        line.insert_str(at, &gpu);
    }
    line
}

/// A status line that is redrawn in place on a terminal and printed as
/// separate lines otherwise.
struct ProgressLine {
    in_place: bool,
    width: usize,
}

impl ProgressLine {
    fn new(in_place: bool) -> ProgressLine {
        ProgressLine { in_place, width: 0 }
    }

    fn draw(&mut self, line: &str) {
        let mut err = std::io::stderr().lock();
        if self.in_place {
            let pad = self.width.saturating_sub(line.chars().count());
            let _ = write!(err, "\r{line}{}", " ".repeat(pad));
            self.width = line.chars().count();
        } else {
            let _ = writeln!(err, "{line}");
        }
        let _ = err.flush();
    }

    fn clear(&mut self) {
        if self.in_place && self.width > 0 {
            let _ = write!(std::io::stderr(), "\r{}\r", " ".repeat(self.width));
            self.width = 0;
        }
    }
}

/// An approximate duration: "~5 min", but "under a second".
fn about(seconds: f64) -> String {
    let text = format_duration(seconds);
    if text.starts_with("under") || text == "unknown" {
        text
    } else {
        format!("~{text}")
    }
}

fn clock(d: Duration) -> String {
    let s = d.as_secs();
    format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

fn install_ctrlc(search: &Search) {
    static PRESSES: AtomicU32 = AtomicU32::new(0);
    let stop = search.stop_handle();
    let _ = ctrlc::set_handler(move || {
        if PRESSES.fetch_add(1, Ordering::SeqCst) == 0 {
            say!("\nStopping... (press Ctrl+C again to quit immediately)");
            stop.stop();
        } else {
            std::process::exit(130);
        }
    });
}

/// Run a search for an unreachable pattern for `duration` and return the
/// average rate. The measurement starts once the GPU (if any) is running.
fn measure_rate(
    threads: usize,
    gpu: GpuMode,
    anywhere: bool,
    duration: Duration,
) -> Result<f64, String> {
    let matcher = Matcher::new(
        vec![Pattern::parse(&"z".repeat(40)).expect("valid")],
        anywhere,
        vec![],
    )
    .expect("valid");
    let search = Search::start(SearchConfig {
        matcher,
        count: Some(1),
        output_dir: std::env::temp_dir(),
        cpu_threads: threads,
        gpu,
    })
    .map_err(|e| e.to_string())?;
    let report = |event: Option<Event>| -> Result<bool, String> {
        match event {
            Some(Event::Error(message)) => Err(message),
            Some(Event::Info(message)) => {
                say!("{message}");
                Ok(true)
            }
            Some(Event::Warning(message)) => {
                say!("warning: {message}");
                Ok(true)
            }
            _ => Ok(false),
        }
    };

    // Wait for the GPU to start (or to report that it will not be used):
    // compiling the GPU program can take several seconds.
    let start = Instant::now();
    if gpu != GpuMode::Off && torv3_vanity::search::gpu_support() {
        while start.elapsed() < Duration::from_secs(60) {
            let event = search.next_event(Duration::from_millis(100));
            let gpu_started =
                matches!(&event, Some(Event::Info(m)) if m.starts_with("Using the GPU"));
            let reported = report(event)?;
            if gpu_started {
                // Let it reach full speed.
                std::thread::sleep(Duration::from_secs(1));
                break;
            }
            if reported {
                break;
            }
        }
    } else {
        std::thread::sleep(Duration::from_millis(500));
    }

    let base = (Instant::now(), search.stats().keys_checked);
    while base.0.elapsed() < duration {
        report(search.next_event(Duration::from_millis(100)))?;
    }
    let keys = search.stats().keys_checked - base.1;
    Ok(keys as f64 / base.0.elapsed().as_secs_f64())
}

fn benchmark(cli: &Cli, threads: usize, duration: Duration) -> Result<ExitCode, String> {
    say!("torv3-vanity {} benchmark", torv3_vanity::VERSION);
    say!("CPU threads: {threads}");
    let rate = measure_rate(threads, cli.gpu.into(), cli.anywhere, duration)?;
    let _ = writeln!(std::io::stdout(), "{}", format_rate(rate));
    let place = if cli.anywhere {
        "anywhere in the address"
    } else {
        "at the start"
    };
    for chars in [5, 6, 7, 8] {
        let pattern = Pattern::parse(&"a".repeat(chars)).expect("valid");
        let keys = 1.0 / pattern.probability(cli.anywhere);
        say!(
            "  {chars} characters {place}: {} on average",
            about(keys / rate)
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn print_gpus() -> ExitCode {
    let gpus = list_gpus();
    if gpus.is_empty() {
        println!("No usable GPU found (or this build has no GPU support).");
    }
    for gpu in gpus {
        let kind = if gpu.software {
            "software, only used for testing"
        } else if gpu.discrete {
            "dedicated"
        } else if gpu.preferred {
            "integrated, used by default"
        } else {
            "integrated, used with --gpu on"
        };
        println!("{} ({}, {kind})", gpu.name, gpu.backend);
    }
    ExitCode::SUCCESS
}

fn verify(dir: &std::path::Path) -> Result<ExitCode, String> {
    match verify_hidden_service_dir(dir) {
        Ok(address) => {
            println!("OK: {address}");
            println!(
                "The keys in {} are valid and match this address.",
                dir.display()
            );
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => Err(format!("{}: {e}", dir.display())),
    }
}

/// Windows: detect being started by double-clicking in Explorer, where the
/// console window would otherwise close before anything can be read.
mod console {
    use std::io::IsTerminal;

    /// Whether this process has a console window of its own (nobody else is
    /// attached to it) and is talking to it directly, which is what a
    /// double-click in Explorer looks like. Programs started by scripts or
    /// other software usually have their output redirected.
    pub fn own_window() -> bool {
        alone_in_console()
            && std::io::stdin().is_terminal()
            && std::io::stdout().is_terminal()
            && std::io::stderr().is_terminal()
    }

    #[cfg(windows)]
    fn alone_in_console() -> bool {
        use windows_sys::Win32::System::Console::GetConsoleProcessList;
        let mut pids = [0u32; 2];
        // SAFETY: the buffer is valid for `pids.len()` elements.
        let n = unsafe { GetConsoleProcessList(pids.as_mut_ptr(), pids.len() as u32) };
        n == 1
    }

    #[cfg(not(windows))]
    fn alone_in_console() -> bool {
        false
    }

    pub fn pause() {
        eprintln!("\nPress Enter to close this window.");
        let _ = std::io::stdin().read_line(&mut String::new());
    }
}

//! Main application state and UI

use crossbeam_channel::{Receiver, Sender};
use eframe::egui;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

use kore_vanity::backend::{
    select_backend, select_backend_with_config, BackendInfo, BackendMode, FoundKey,
    Progress, SearchFilter,
};
use kore_vanity::probability::{calculate_stats, calculate_progress, format_probability, format_number, effective_prefix_len};
use kore_vanity::pattern::is_valid_pattern;

/// Application state
pub struct VanityApp {
    // Input fields
    prefix_input: String,
    contains_input: String,
    anywhere: bool,
    output_dir: String,

    // Backend selection
    selected_mode: BackendModeSelection,
    cpu_threads: usize,
    max_threads: usize,

    // Current backend info
    backend_info: Option<BackendInfo>,

    // Generation state
    state: AppState,
    worker_handle: Option<JoinHandle<()>>,
    stop_flag: Arc<AtomicBool>,

    // Channels
    progress_rx: Option<Receiver<Progress>>,
    result_rx: Option<Receiver<FoundKey>>,
    error_rx: Option<Receiver<String>>,

    // Progress display
    progress: Progress,
    results: Vec<FoundKey>,
    start_time: Option<Instant>,

    // Errors
    error_message: Option<String>,

    // Pending prefixes
    pending_prefixes: Vec<String>,

    // Active contains words (for stats calculation)
    active_contains: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackendModeSelection {
    Auto,
    Cpu,
    Cuda,
    Hybrid,
}

impl BackendModeSelection {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "Auto (best available)",
            Self::Cpu => "CPU only",
            Self::Cuda => "GPU only (CUDA)",
            Self::Hybrid => "Hybrid (CPU + GPU)",
        }
    }

    fn to_backend_mode(&self) -> BackendMode {
        match self {
            Self::Auto => BackendMode::Auto,
            Self::Cpu => BackendMode::Cpu,
            Self::Cuda => BackendMode::Cuda,
            Self::Hybrid => BackendMode::Hybrid,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppState {
    Idle,
    Running,
    Stopped,
    Finished,
}

impl VanityApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let max_threads = num_cpus::get();

        // Try to detect available backend
        let backend = select_backend();
        let backend_info = Some(backend.info());

        // Default output directory
        let output_dir = directories::UserDirs::new()
            .and_then(|dirs| dirs.document_dir().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."))
            .join("kore_vanity_keys")
            .display()
            .to_string();

        Self {
            prefix_input: String::new(),
            contains_input: String::new(),
            anywhere: false,
            output_dir,
            selected_mode: BackendModeSelection::Auto,
            cpu_threads: max_threads,
            max_threads,
            backend_info,
            state: AppState::Idle,
            worker_handle: None,
            stop_flag: Arc::new(AtomicBool::new(false)),
            progress_rx: None,
            result_rx: None,
            error_rx: None,
            progress: Progress::default(),
            results: Vec::new(),
            start_time: None,
            error_message: None,
            pending_prefixes: Vec::new(),
            active_contains: Vec::new(),
        }
    }

    fn start_generation(&mut self) {
        // Parse and validate prefixes
        let prefixes: Vec<String> = self
            .prefix_input
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();

        if prefixes.is_empty() {
            self.error_message = Some("Please enter at least one pattern".to_string());
            return;
        }

        // Validate each prefix pattern (allows a-z, 2-7, ? as wildcard)
        for prefix in &prefixes {
            if !is_valid_pattern(prefix) {
                self.error_message = Some(format!("Invalid pattern: '{}' (use a-z, 2-7, ? for wildcard)", prefix));
                return;
            }
        }

        // Validate output directory
        let output_dir = PathBuf::from(&self.output_dir);
        if !output_dir.exists() {
            if let Err(e) = std::fs::create_dir_all(&output_dir) {
                self.error_message = Some(format!("Failed to create output directory: {}", e));
                return;
            }
        }

        // Clear state
        self.error_message = None;
        self.results.clear();
        self.progress = Progress::default();
        self.pending_prefixes = prefixes.clone();
        self.stop_flag.store(false, Ordering::SeqCst);
        self.start_time = Some(Instant::now());

        // Create channels
        let (progress_tx, progress_rx) = crossbeam_channel::unbounded();
        let (result_tx, result_rx) = crossbeam_channel::unbounded();
        let (stop_tx, stop_rx) = crossbeam_channel::bounded(1);

        self.progress_rx = Some(progress_rx);
        self.result_rx = Some(result_rx);

        // Parse contains filter
        let contains: Vec<String> = self
            .contains_input
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();

        // Validate contains patterns
        for pattern in &contains {
            if !is_valid_pattern(pattern) {
                self.error_message = Some(format!("Invalid contains pattern: '{}' (use a-z, 2-7, ? for wildcard)", pattern));
                return;
            }
        }

        // Store for stats calculation
        self.active_contains = contains.clone();

        let filter = SearchFilter {
            contains,
            prefix_anywhere: self.anywhere,
        };

        // Spawn worker thread
        let backend_mode = self.selected_mode.to_backend_mode();
        let cpu_threads = self.cpu_threads;
        let stop_flag = self.stop_flag.clone();

        // Channel to report backend errors back to GUI
        let (error_tx, error_rx) = crossbeam_channel::bounded(1);
        self.error_rx = Some(error_rx);

        let handle = std::thread::spawn(move || {
            let backend = select_backend_with_config(backend_mode, cpu_threads);

            // Monitor stop flag
            let monitor_stop_flag = stop_flag.clone();
            std::thread::spawn(move || {
                while !monitor_stop_flag.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                let _ = stop_tx.send(());
            });

            if let Err(e) = backend.generate_with_filter(prefixes, output_dir, progress_tx, result_tx, stop_rx, filter) {
                let _ = error_tx.send(format!("Generation error: {}", e));
            }
        });

        self.worker_handle = Some(handle);
        self.state = AppState::Running;
    }

    fn stop_generation(&mut self) {
        self.stop_flag.store(true, Ordering::SeqCst);
        self.state = AppState::Stopped;
    }

    fn update_from_channels(&mut self) {
        // Update progress
        if let Some(rx) = &self.progress_rx {
            while let Ok(progress) = rx.try_recv() {
                self.progress = progress;
            }
        }

        // Update results
        if let Some(rx) = &self.result_rx {
            while let Ok(result) = rx.try_recv() {
                // Remove from pending
                self.pending_prefixes.retain(|p| p != &result.prefix);
                self.results.push(result);
            }
        }

        // Check for errors from worker
        if let Some(rx) = &self.error_rx {
            if let Ok(error) = rx.try_recv() {
                self.error_message = Some(error);
                self.state = AppState::Stopped;
            }
        }

        // Check if finished
        if self.state == AppState::Running && self.pending_prefixes.is_empty() {
            self.state = AppState::Finished;
        }

        // Check if worker thread finished
        if let Some(handle) = &self.worker_handle {
            if handle.is_finished() && self.state == AppState::Running {
                self.state = AppState::Stopped;
            }
        }
    }
}

impl eframe::App for VanityApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process channel updates
        self.update_from_channels();

        // Request repaint while running
        if self.state == AppState::Running {
            ctx.request_repaint();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Kore Vanity Generator");
            ui.separator();

            // Backend info
            ui.group(|ui| {
                ui.label("Backend Status:");
                if let Some(info) = &self.backend_info {
                    ui.label(format!("  {}", info.name));
                    ui.label(format!(
                        "  Est. speed: ~{} keys/sec",
                        format_speed(info.estimated_speed)
                    ));
                } else {
                    ui.label("  Detecting...");
                }
            });

            ui.add_space(10.0);

            // Backend mode selection
            ui.horizontal(|ui| {
                ui.label("Mode:");
                egui::ComboBox::from_id_salt("backend_mode")
                    .selected_text(self.selected_mode.as_str())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.selected_mode,
                            BackendModeSelection::Auto,
                            BackendModeSelection::Auto.as_str(),
                        );
                        ui.selectable_value(
                            &mut self.selected_mode,
                            BackendModeSelection::Cpu,
                            BackendModeSelection::Cpu.as_str(),
                        );
                        ui.selectable_value(
                            &mut self.selected_mode,
                            BackendModeSelection::Cuda,
                            BackendModeSelection::Cuda.as_str(),
                        );
                        ui.selectable_value(
                            &mut self.selected_mode,
                            BackendModeSelection::Hybrid,
                            BackendModeSelection::Hybrid.as_str(),
                        );
                    });
            });

            // CPU threads slider (shown for CPU/Hybrid/Auto modes)
            let show_threads = matches!(
                self.selected_mode,
                BackendModeSelection::Cpu | BackendModeSelection::Auto | BackendModeSelection::Hybrid
            );
            if show_threads {
                ui.horizontal(|ui| {
                    ui.label("CPU Threads:");
                    ui.add(
                        egui::Slider::new(&mut self.cpu_threads, 1..=self.max_threads)
                            .clamp_to_range(true),
                    );
                });
            }

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(10.0);

            // Prefix input
            ui.label("Patterns (comma-separated):");
            ui.add(
                egui::TextEdit::singleline(&mut self.prefix_input)
                    .hint_text("kore, ????wink, test?me")
                    .desired_width(f32::INFINITY),
            );
            ui.small("Use a-z, 2-7. Use ? as wildcard for any character. 5-6 chars recommended.");

            ui.add_space(5.0);

            // Anywhere checkbox
            ui.checkbox(&mut self.anywhere, "Match patterns anywhere in address (not just at start)");

            ui.add_space(10.0);

            // Contains filter input
            ui.label("Must also contain (optional, comma-separated):");
            ui.add(
                egui::TextEdit::singleline(&mut self.contains_input)
                    .hint_text("block, ch??n")
                    .desired_width(f32::INFINITY),
            );
            ui.small("Patterns that must appear anywhere in the address. Use ? as wildcard.");

            ui.add_space(10.0);

            // Output directory
            ui.horizontal(|ui| {
                ui.label("Output:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.output_dir).desired_width(400.0),
                );
                #[cfg(target_os = "windows")]
                if ui.button("Browse...").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_directory(&self.output_dir)
                        .pick_folder()
                    {
                        self.output_dir = path.display().to_string();
                    }
                }
            });

            ui.add_space(15.0);

            // Start/Stop button
            ui.horizontal(|ui| {
                let button_enabled = match self.state {
                    AppState::Idle | AppState::Stopped | AppState::Finished => true,
                    AppState::Running => true,
                };

                let button_text = match self.state {
                    AppState::Idle | AppState::Stopped | AppState::Finished => "Start Generation",
                    AppState::Running => "Stop",
                };

                if ui
                    .add_enabled(button_enabled, egui::Button::new(button_text).min_size(egui::vec2(150.0, 30.0)))
                    .clicked()
                {
                    match self.state {
                        AppState::Idle | AppState::Stopped | AppState::Finished => {
                            self.start_generation();
                        }
                        AppState::Running => {
                            self.stop_generation();
                        }
                    }
                }

                // Status indicator
                let status_text = match self.state {
                    AppState::Idle => "Ready",
                    AppState::Running => "Running...",
                    AppState::Stopped => "Stopped",
                    AppState::Finished => "Complete!",
                };
                ui.label(status_text);
            });

            // Error message
            if let Some(error) = &self.error_message {
                ui.add_space(5.0);
                ui.colored_label(egui::Color32::RED, error);
            }

            ui.add_space(15.0);
            ui.separator();
            ui.add_space(10.0);

            // Progress section
            if self.state == AppState::Running || self.progress.keys_checked > 0 {
                ui.label("Progress:");

                // Calculate progress percentage based on effective prefix length (non-wildcard chars)
                let all_prefixes: Vec<String> = self
                    .pending_prefixes
                    .iter()
                    .chain(self.results.iter().map(|r: &FoundKey| &r.prefix))
                    .cloned()
                    .collect();
                let max_prefix_len = effective_prefix_len(&all_prefixes).max(1);

                // Calculate live statistics using probability module
                let live_stats = calculate_progress(
                    self.progress.keys_checked,
                    self.progress.keys_per_sec,
                    max_prefix_len,
                    &self.active_contains,
                );

                ui.add(
                    egui::ProgressBar::new((live_stats.progress_percent / 100.0) as f32)
                        .show_percentage()
                        .animate(self.state == AppState::Running),
                );

                // Stats grid
                egui::Grid::new("progress_grid")
                    .num_columns(2)
                    .spacing([20.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Keys checked:");
                        ui.label(format_number(self.progress.keys_checked as f64));
                        ui.end_row();

                        ui.label("Speed:");
                        ui.label(format!("{:.2} M/sec", self.progress.keys_per_sec / 1_000_000.0));
                        ui.end_row();

                        if let Some(start) = self.start_time {
                            ui.label("Elapsed:");
                            ui.label(format_duration(start.elapsed()));
                            ui.end_row();
                        }

                        ui.label("Est. remaining:");
                        ui.label(&live_stats.remaining_estimate);
                        ui.end_row();

                        ui.label("Probability so far:");
                        ui.label(format!("{:.1}%", live_stats.cumulative_probability * 100.0));
                        ui.end_row();

                        ui.label("Expected keys:");
                        ui.label(format_number(live_stats.expected_keys));
                        ui.end_row();

                        // Show prefix matches only when contains filter is active
                        if !self.active_contains.is_empty() && self.progress.prefix_matches > 0 {
                            ui.label("Prefix matches:");
                            ui.label(format!("{}", self.progress.prefix_matches));
                            ui.end_row();

                            ui.label("Rejected (missing words):");
                            ui.label(format!("{}", self.progress.contains_rejects));
                            ui.end_row();
                        }
                    });

                ui.add_space(10.0);
            }

            // Show initial probability estimate when idle with valid input
            if self.state == AppState::Idle && !self.prefix_input.trim().is_empty() {
                let prefixes: Vec<String> = self.prefix_input
                    .split(',')
                    .map(|s| s.trim().to_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect();

                let contains: Vec<String> = self.contains_input
                    .split(',')
                    .map(|s| s.trim().to_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect();

                let max_len = effective_prefix_len(&prefixes);
                if max_len > 0 {
                    if let Some(info) = &self.backend_info {
                        let stats = calculate_stats(max_len, &contains, info.estimated_speed as f64);

                        ui.separator();
                        ui.add_space(5.0);
                        ui.label("Probability Estimate:");

                        egui::Grid::new("estimate_grid")
                            .num_columns(2)
                            .spacing([20.0, 4.0])
                            .show(ui, |ui| {
                                ui.label("Probability per key:");
                                ui.label(format_probability(stats.probability_per_key));
                                ui.end_row();

                                ui.label("Expected keys:");
                                ui.label(format_number(stats.expected_keys));
                                ui.end_row();

                                ui.label("Estimated time:");
                                ui.colored_label(
                                    if stats.estimated_seconds > 86400.0 * 30.0 {
                                        egui::Color32::RED
                                    } else if stats.estimated_seconds > 86400.0 {
                                        egui::Color32::YELLOW
                                    } else {
                                        egui::Color32::GREEN
                                    },
                                    &stats.time_estimate
                                );
                                ui.end_row();
                            });

                        ui.add_space(10.0);
                    }
                }
            }

            // Results section
            if !self.results.is_empty() || !self.pending_prefixes.is_empty() {
                ui.separator();
                ui.add_space(10.0);
                ui.label("Results:");

                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| {
                        // Show found results
                        for result in &self.results {
                            ui.horizontal(|ui| {
                                ui.colored_label(egui::Color32::GREEN, "✓");
                                ui.label(&result.prefix);
                                ui.label("→");
                                ui.monospace(&result.onion_address[..30]);
                                ui.label("...");

                                if ui.button("Copy").clicked() {
                                    ui.output_mut(|o| {
                                        o.copied_text = result.onion_address.clone();
                                    });
                                }

                                if ui.button("Open folder").clicked() {
                                    if let Some(parent) = result.key_path.parent() {
                                        let _ = open::that(parent);
                                    }
                                }
                            });
                        }

                        // Show pending prefixes
                        for prefix in &self.pending_prefixes {
                            ui.horizontal(|ui| {
                                ui.colored_label(egui::Color32::YELLOW, "○");
                                ui.label(prefix);
                                ui.label("→ searching...");
                            });
                        }
                    });
            }
        });
    }
}

fn format_speed(speed: u64) -> String {
    if speed >= 1_000_000_000 {
        format!("{:.1}B", speed as f64 / 1_000_000_000.0)
    } else if speed >= 1_000_000 {
        format!("{:.1}M", speed as f64 / 1_000_000.0)
    } else if speed >= 1_000 {
        format!("{:.1}K", speed as f64 / 1_000.0)
    } else {
        format!("{}", speed)
    }
}

fn format_large_number(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.2}B", n as f64 / 1_000_000_000.0)
    } else if n >= 1_000_000 {
        format!("{:.2}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.2}K", n as f64 / 1_000.0)
    } else {
        format!("{}", n)
    }
}

fn format_duration(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let secs = secs % 60;

    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, mins, secs)
    } else {
        format!("{:02}:{:02}", mins, secs)
    }
}

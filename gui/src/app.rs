//! Application state and user interface.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText};
use torv3_vanity::estimate::{Estimate, format_count, format_duration, format_odds, format_rate};
use torv3_vanity::pattern::{Matcher, Pattern};
use torv3_vanity::search::{
    self, Event, Found, GpuInfo, GpuMode, RateMeter, Search, SearchConfig, Stats,
};

pub const REPO_URL: &str = "https://github.com/Mxrider007/tor-v3-vanity-address-generator";

/// Rough keys/second per CPU thread, used for estimates before the first run.
const GUESS_RATE_PER_THREAD: f64 = 5.0e6;

/// Results shown in the list; the rest are only in the keys folder.
const MAX_SHOWN: usize = 100;

const GREEN: Color32 = Color32::from_rgb(34, 160, 90);
const AMBER: Color32 = Color32::from_rgb(214, 140, 20);
const RED: Color32 = Color32::from_rgb(210, 60, 60);

/// Settings remembered between launches.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Settings {
    patterns: String,
    contains: String,
    anywhere: bool,
    count: u32,
    output_dir: PathBuf,
    threads: usize,
    /// Use the GPU; `None` means "if it is a dedicated card or Apple Silicon".
    use_gpu: Option<bool>,
    /// Speed measured during the last long-enough search.
    measured: Option<Measured>,
}

/// A measured search speed and the settings it was measured with.
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
struct Measured {
    rate: f64,
    threads: usize,
    gpu: bool,
    /// "Anywhere" searches are slower and never use the GPU.
    #[serde(default)]
    anywhere: bool,
}

impl Default for Settings {
    fn default() -> Self {
        // The home folder rather than Documents or the Desktop, which are
        // often uploaded to OneDrive or iCloud.
        let home = directories::BaseDirs::new()
            .map(|d| d.home_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        Settings {
            patterns: String::new(),
            contains: String::new(),
            anywhere: false,
            count: 1,
            output_dir: home.join("Tor Vanity Keys"),
            threads: search::available_threads(),
            use_gpu: None,
            measured: None,
        }
    }
}

/// A running (or finished) search.
struct Run {
    search: Search,
    threads: usize,
    anywhere: bool,
    estimate: Estimate,
    meter: RateMeter,
    rate: f64,
    stats: Stats,
    finished: bool,
    failed: bool,
}

enum Message {
    Info(String),
    Warning(String),
    Error(String),
}

pub struct App {
    settings: Settings,
    max_threads: usize,
    gpus: Option<Vec<GpuInfo>>,
    gpu_probe: Option<Receiver<Vec<GpuInfo>>>,
    run: Option<Run>,
    found: Vec<Found>,
    messages: Vec<Message>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> App {
        let mut settings: Settings = cc
            .storage
            .and_then(|s| eframe::get_value(s, eframe::APP_KEY))
            .unwrap_or_default();
        let max_threads = search::available_threads();
        settings.threads = settings.threads.clamp(1, max_threads);

        // Enumerating GPUs can take a moment; don't block the first frame.
        let (tx, rx) = mpsc::channel();
        let ctx = cc.egui_ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(search::list_gpus());
            ctx.request_repaint();
        });

        App {
            settings,
            max_threads,
            gpus: None,
            gpu_probe: Some(rx),
            run: None,
            found: Vec::new(),
            messages: Vec::new(),
        }
    }

    fn running(&self) -> bool {
        self.run.as_ref().is_some_and(|r| !r.finished)
    }

    /// The GPU that will be used, if any: a dedicated card or Apple Silicon
    /// first, otherwise integrated graphics (never software renderers).
    fn best_gpu(&self) -> Option<&GpuInfo> {
        let gpus = self.gpus.as_ref()?;
        gpus.iter()
            .find(|g| g.preferred)
            .or_else(|| gpus.iter().find(|g| !g.software))
            .or_else(|| gpus.iter().find(|_| search::software_gpu_allowed()))
    }

    /// Whether the GPU box is ticked (by default for dedicated cards and
    /// Apple Silicon).
    fn use_gpu(&self) -> bool {
        self.best_gpu()
            .is_some_and(|gpu| self.settings.use_gpu.unwrap_or(gpu.preferred))
    }

    fn gpu_mode(&self) -> GpuMode {
        if self.settings.anywhere {
            // The GPU only searches at the start of the address.
            GpuMode::Off
        } else if self.gpus.is_none() {
            // Still looking for graphics cards: let the search decide.
            GpuMode::Auto
        } else if self.use_gpu() {
            GpuMode::On
        } else {
            GpuMode::Off
        }
    }

    /// Why keys cannot be saved in the chosen folder, if they cannot.
    fn output_problem(&self) -> Option<&'static str> {
        let dir = &self.settings.output_dir;
        if dir.as_os_str().is_empty() {
            Some("Choose a folder to save the keys in.")
        } else if !dir.is_absolute() {
            Some("Enter the full path of the folder (or use Browse…).")
        } else {
            None
        }
    }

    fn build_matcher(&self) -> Result<Matcher, String> {
        let patterns = Pattern::parse_list(&self.settings.patterns).map_err(|e| e.to_string())?;
        if patterns.is_empty() {
            return Err(String::new());
        }
        let contains = Pattern::parse_list(&self.settings.contains).map_err(|e| e.to_string())?;
        Matcher::new(patterns, self.settings.anywhere, contains).map_err(|e| e.to_string())
    }

    fn count(&self) -> Option<u64> {
        (self.settings.count > 0).then_some(u64::from(self.settings.count))
    }

    /// Best available speed figure and whether it was measured.
    fn speed(&self) -> (f64, bool) {
        if let Some(run) = &self.run
            && !run.finished
            && run.stats.elapsed > Duration::from_secs(3)
            && run.rate > 0.0
        {
            return (run.rate, true);
        }
        let threads = self.settings.threads;
        let gpu = self.gpu_mode() != GpuMode::Off;
        let anywhere = self.settings.anywhere;
        match self.settings.measured {
            Some(m) if m.threads == threads && m.gpu == gpu && m.anywhere == anywhere => {
                (m.rate, true)
            }
            // CPU speed scales with the number of threads.
            Some(m) if !m.gpu && !gpu && m.anywhere == anywhere => {
                (m.rate * threads as f64 / m.threads as f64, false)
            }
            // "Anywhere" searches run at about 60% of the speed.
            _ if anywhere => (threads as f64 * GUESS_RATE_PER_THREAD * 0.6, false),
            _ => (threads as f64 * GUESS_RATE_PER_THREAD, false),
        }
    }

    fn start(&mut self) {
        let matcher = match self.build_matcher() {
            Ok(m) => m,
            Err(_) => return,
        };
        if self.output_problem().is_some() {
            return;
        }
        let count = self.count();
        let estimate = Estimate::new(&matcher, count);
        let gpu = self.gpu_mode();
        let config = SearchConfig {
            matcher,
            count,
            output_dir: self.settings.output_dir.clone(),
            cpu_threads: self.settings.threads,
            gpu,
        };
        self.messages.clear();
        match Search::start(config) {
            Ok(search) => {
                self.run = Some(Run {
                    search,
                    threads: self.settings.threads,
                    anywhere: self.settings.anywhere,
                    estimate,
                    meter: RateMeter::new(Duration::from_secs(8)),
                    rate: 0.0,
                    stats: Stats::default(),
                    finished: false,
                    failed: false,
                });
            }
            Err(e) => self.messages.push(Message::Error(e.to_string())),
        }
    }

    fn poll(&mut self) {
        if let Some(rx) = &self.gpu_probe {
            match rx.try_recv() {
                Ok(gpus) => self.gpus = Some(gpus),
                // The probe crashed (for example in a graphics driver).
                Err(TryRecvError::Disconnected) => self.gpus = Some(Vec::new()),
                Err(TryRecvError::Empty) => {}
            }
            if self.gpus.is_some() {
                self.gpu_probe = None;
            }
        }

        let Some(run) = &mut self.run else { return };
        if run.finished {
            return;
        }
        while let Some(event) = run.search.try_event() {
            match event {
                Event::Found(found) => self.found.push(found),
                Event::Info(m) => self.messages.push(Message::Info(m)),
                Event::Warning(m) => self.messages.push(Message::Warning(m)),
                Event::Error(m) => {
                    run.failed = true;
                    self.messages.push(Message::Error(m));
                }
                Event::Finished => run.finished = true,
            }
        }
        // After `Finished` this records the final totals once.
        run.stats = run.search.stats();
        run.rate = run.meter.update(run.stats.keys_checked);
        // Remember the recent speed once the GPU (if any) has warmed up,
        // unless saving many keys slowed the search down.
        if !run.finished
            && run.stats.elapsed > Duration::from_secs(10)
            && run.rate > 0.0
            && run.stats.found <= run.estimate.patterns() as u64
        {
            self.settings.measured = Some(Measured {
                rate: run.rate,
                threads: run.threads,
                gpu: run.stats.gpu_keys_checked > 0,
                anywhere: run.anywhere,
            });
        }
    }

    /// Open a file or folder in the system's file manager.
    fn open(&mut self, path: &Path) {
        if let Err(e) = open::that_detached(path) {
            self.messages.push(Message::Warning(format!(
                "could not open {}: {e}",
                path.display()
            )));
        }
    }
}

impl eframe::App for App {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.settings);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Give a key that is being saved the chance to finish.
        if let Some(run) = &self.run {
            run.search.stop();
            let deadline = Instant::now() + Duration::from_secs(5);
            while !run.search.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll();
        if self.running() {
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }

        egui::Panel::top("header").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading(RichText::new("Tor V3 Vanity Address Generator").strong());
                ui.label(RichText::new(format!("v{}", torv3_vanity::VERSION)).weak());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.hyperlink_to("Help & source code", REPO_URL);
                });
            });
            ui.label("Create a custom Tor .onion address that starts with the text you choose.");
            ui.add_space(6.0);
        });

        egui::Panel::bottom("footer").show(ui, |ui| {
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "🔒 Keep each folder's hs_ed25519_secret_key private — anyone who has it controls that address.",
                )
                .small(),
            );
            ui.add_space(4.0);
        });

        egui::CentralPanel::default_margins().show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    self.settings_section(ui);
                    ui.add_space(8.0);
                    self.action_section(ui);
                    ui.add_space(6.0);
                    self.messages_section(ui);
                    self.progress_section(ui);
                    self.results_section(ui);
                    ui.add_space(8.0);
                    help_section(ui);
                });
        });
    }
}

impl App {
    fn settings_section(&mut self, ui: &mut egui::Ui) {
        let enabled = !self.running();
        let matcher = self.build_matcher();

        ui.add_enabled_ui(enabled, |ui| {
            section(ui, "1. Your address", |ui| {
                let label = if self.settings.anywhere {
                    "Text to find (separate several with commas):"
                } else {
                    "Address should start with (separate several with commas):"
                };
                ui.label(label);
                ui.add(
                    egui::TextEdit::singleline(&mut self.settings.patterns)
                        .hint_text("e.g. shop")
                        .desired_width(f32::INFINITY)
                        .font(egui::TextStyle::Monospace),
                );
                ui.label(
                    RichText::new("Letters a–z and digits 2–7 only. Use ? for \"any character\". Not case sensitive.")
                        .small()
                        .weak(),
                );
                if let Err(e) = &matcher
                    && !e.is_empty()
                {
                    ui.colored_label(RED, e);
                }
                ui.checkbox(&mut self.settings.anywhere, "Allow the text anywhere in the address (not just at the start)");
                ui.horizontal(|ui| {
                    ui.label("Must also contain (optional):");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.settings.contains)
                            .hint_text("e.g. blog")
                            .desired_width(f32::INFINITY)
                            .font(egui::TextStyle::Monospace),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Addresses to find per pattern:");
                    ui.add(egui::DragValue::new(&mut self.settings.count).range(0..=10_000));
                    if self.settings.count == 0 {
                        ui.label(RichText::new("(unlimited — runs until you press Stop)").weak());
                    }
                });
            });

            section(ui, "2. Where to save the keys", |ui| {
                ui.horizontal(|ui| {
                    let mut text = self.settings.output_dir.display().to_string();
                    let width = (ui.available_width() - 190.0).max(120.0);
                    if ui
                        .add(egui::TextEdit::singleline(&mut text).desired_width(width))
                        .changed()
                    {
                        self.settings.output_dir = PathBuf::from(text);
                    }
                    if ui.button("📁 Browse…").clicked()
                        && let Some(dir) = rfd::FileDialog::new()
                            .set_directory(existing_ancestor(&self.settings.output_dir))
                            .pick_folder()
                    {
                        self.settings.output_dir = dir;
                    }
                    if ui.button("Open").clicked() && self.output_problem().is_none() {
                        let dir = self.settings.output_dir.clone();
                        let _ = std::fs::create_dir_all(&dir);
                        self.open(&dir);
                    }
                });
                if let Some(problem) = self.output_problem() {
                    ui.colored_label(RED, problem);
                } else if let Some(service) = cloud_synced(&self.settings.output_dir) {
                    ui.colored_label(
                        AMBER,
                        format!(
                            "⚠ This folder may be uploaded to {service}. Your secret keys \
                             should not leave this computer: choose a folder that is not synced."
                        ),
                    );
                }
            });

            section(ui, "3. Hardware", |ui| {
                ui.horizontal(|ui| {
                    ui.label("CPU threads:");
                    ui.add(egui::Slider::new(&mut self.settings.threads, 1..=self.max_threads));
                    ui.label(RichText::new(format!("of {}", self.max_threads)).weak());
                });
                match (&self.gpus, self.best_gpu().cloned()) {
                    (None, _) => {
                        ui.label(RichText::new("Looking for a graphics card…").weak());
                    }
                    (Some(_), None) => {
                        ui.label(RichText::new("No compatible graphics card found — using the CPU.").weak());
                    }
                    (Some(_), Some(gpu)) => {
                        let mut use_gpu = self.use_gpu();
                        let anywhere = self.settings.anywhere;
                        if ui
                            .add_enabled(
                                !anywhere,
                                egui::Checkbox::new(
                                    &mut use_gpu,
                                    format!("Also use the graphics card: {} ({})", gpu.name, gpu.backend),
                                ),
                            )
                            .changed()
                        {
                            self.settings.use_gpu = Some(use_gpu);
                        }
                        if anywhere {
                            ui.label(
                                RichText::new("The graphics card only helps with text at the start of the address.")
                                    .small()
                                    .weak(),
                            );
                        } else if !gpu.preferred && use_gpu {
                            ui.label(
                                RichText::new("This is integrated graphics; it may add little speed.")
                                    .small()
                                    .weak(),
                            );
                        }
                    }
                }
            });
        });

        // Difficulty and time estimate for the current input.
        if let Ok(matcher) = &matcher {
            let estimate = Estimate::new(matcher, self.count());
            let (rate, measured) = self.speed();
            let seconds = estimate.expected_keys() / rate;
            let color = if seconds > 30.0 * 86_400.0 {
                RED
            } else if seconds > 86_400.0 {
                AMBER
            } else {
                GREEN
            };
            section(ui, "Estimate", |ui| {
                egui::Grid::new("estimate")
                    .num_columns(2)
                    .spacing([16.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Chance per key:");
                        ui.label(format_odds(estimate.hardest()));
                        ui.end_row();
                        ui.label("Speed:");
                        let label = if measured { "measured" } else { "rough guess" };
                        ui.label(format!("{} ({label})", format_rate(rate)));
                        ui.end_row();
                        ui.label(if estimate.count().is_some() {
                            "Expected time:"
                        } else {
                            "Time per address:"
                        });
                        ui.colored_label(color, RichText::new(format_duration(seconds)).strong());
                        ui.end_row();
                        if estimate.count().is_some() {
                            ui.label("Very likely (90%) within:");
                            ui.label(format_duration(estimate.keys_for_chance(0.9) / rate));
                            ui.end_row();
                        }
                    });
                if seconds > 365.0 * 86_400.0 {
                    ui.colored_label(
                        RED,
                        "That is too long to be practical — try a shorter pattern. Every character you remove makes it 32× faster.",
                    );
                }
            });
        }
    }

    fn action_section(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let size = [200.0, 40.0];
            if self.running() {
                if ui
                    .add_sized(size, egui::Button::new(RichText::new("■  Stop").size(18.0)))
                    .clicked()
                    && let Some(run) = &self.run
                {
                    run.search.stop();
                }
            } else {
                let ready = self.build_matcher().is_ok() && self.output_problem().is_none();
                let button =
                    egui::Button::new(RichText::new("▶  Start search").size(18.0).strong());
                if ui
                    .add_enabled_ui(ready, |ui| ui.add_sized(size, button))
                    .inner
                    .clicked()
                {
                    self.start();
                }
                if !ready && self.settings.patterns.trim().is_empty() {
                    ui.label(RichText::new("Type the text you want in your address first.").weak());
                }
            }
        });
    }

    fn progress_section(&mut self, ui: &mut egui::Ui) {
        let Some(run) = &self.run else { return };
        let stats = run.stats;
        section(ui, "Progress", |ui| {
            let status = if !run.finished {
                RichText::new("Searching…").color(AMBER).strong()
            } else if run.failed {
                RichText::new("Stopped because of an error")
                    .color(RED)
                    .strong()
            } else if run.search.is_complete() {
                RichText::new("Done!").color(GREEN).strong()
            } else {
                RichText::new("Stopped").strong()
            };
            ui.label(status);
            egui::Grid::new("progress")
                .num_columns(2)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Time:");
                    ui.label(format_duration(stats.elapsed.as_secs_f64()));
                    ui.end_row();
                    ui.label("Speed:");
                    let rate = if run.finished {
                        stats.keys_checked as f64 / stats.elapsed.as_secs_f64().max(1e-3)
                    } else {
                        run.rate
                    };
                    let mut speed = format_rate(rate);
                    if stats.gpu_keys_checked > 0 {
                        let share = 100.0 * stats.gpu_keys_checked as f64
                            / stats.keys_checked.max(1) as f64;
                        speed += &format!("  ({share:.0}% on the GPU)");
                    }
                    ui.label(speed);
                    ui.end_row();
                    ui.label("Keys checked:");
                    ui.label(format_count(stats.keys_checked as f64));
                    ui.end_row();
                    ui.label("Found:");
                    match run.estimate.total() {
                        Some(total) => ui.label(format!("{} of {total}", stats.found)),
                        None => ui.label(stats.found.to_string()),
                    };
                    ui.end_row();
                    if run.estimate.total().is_some() && stats.found == 0 {
                        ui.label("Chance found by now:");
                        let chance = run.estimate.chance_after(stats.keys_checked as f64);
                        ui.add(
                            egui::ProgressBar::new(chance as f32)
                                .desired_width(240.0)
                                .text(format!("{:.0}%", chance * 100.0)),
                        );
                        ui.end_row();
                    }
                    if stats.near_misses > 0 {
                        ui.label("Near misses:");
                        ui.label(format!(
                            "{} matched but lacked a \"must also contain\" word",
                            format_count(stats.near_misses as f64)
                        ));
                        ui.end_row();
                    }
                });
        });
    }

    fn messages_section(&mut self, ui: &mut egui::Ui) {
        for message in &self.messages {
            match message {
                Message::Info(m) => ui.label(RichText::new(format!("ℹ {m}")).weak()),
                Message::Warning(m) => ui.colored_label(AMBER, format!("⚠ {m}")),
                Message::Error(m) => ui.colored_label(RED, format!("✖ {m}")),
            };
        }
    }

    fn results_section(&mut self, ui: &mut egui::Ui) {
        if self.found.is_empty() {
            return;
        }
        let mut clear = false;
        let mut open = None;
        section(ui, "Your addresses", |ui| {
            // Newest first; a long list would make the window sluggish.
            for found in self.found.iter().rev().take(MAX_SHOWN) {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("✔").color(GREEN));
                    ui.add(
                        egui::Label::new(RichText::new(&found.address).monospace())
                            .selectable(true),
                    );
                });
                ui.horizontal(|ui| {
                    ui.add_space(18.0);
                    if ui.button("📋 Copy address").clicked() {
                        ui.ctx().copy_text(found.address.clone());
                    }
                    if ui.button("📁 Show keys folder").clicked() {
                        open = Some(found.path.clone());
                    }
                    ui.label(RichText::new(format!("matched \"{}\"", found.pattern)).weak());
                });
                ui.add_space(4.0);
            }
            if self.found.len() > MAX_SHOWN {
                ui.label(
                    RichText::new(format!(
                        "…and {} more in the keys folder.",
                        self.found.len() - MAX_SHOWN
                    ))
                    .weak(),
                );
            }
            if !self.running() && ui.small_button("Clear list").clicked() {
                clear = true;
            }
        });
        if let Some(path) = open {
            self.open(&path);
        }
        if clear {
            self.found.clear();
        }
    }
}

/// The cloud service that probably uploads `path`, judging by its name.
fn cloud_synced(path: &Path) -> Option<&'static str> {
    let lower = path.to_string_lossy().to_lowercase();
    let known = [
        ("onedrive", "OneDrive"),
        ("dropbox", "Dropbox"),
        ("google drive", "Google Drive"),
        ("googledrive", "Google Drive"),
        ("icloud", "iCloud Drive"),
        ("mobile documents", "iCloud Drive"),
        ("nextcloud", "Nextcloud"),
    ];
    if let Some((_, service)) = known.iter().find(|(needle, _)| lower.contains(needle)) {
        return Some(service);
    }
    // macOS can upload Desktop and Documents to iCloud Drive.
    if cfg!(target_os = "macos")
        && let Some(dirs) = directories::UserDirs::new()
        && [dirs.desktop_dir(), dirs.document_dir()]
            .into_iter()
            .flatten()
            .any(|d| path.starts_with(d))
    {
        return Some("iCloud Drive (if Desktop & Documents syncing is on)");
    }
    None
}

fn help_section(ui: &mut egui::Ui) {
    egui::CollapsingHeader::new("How do I use my new address with Tor?").show(ui, |ui| {
        ui.label("Each address is saved as a folder containing hostname, hs_ed25519_public_key, hs_ed25519_secret_key and authorized_clients.");
        ui.label("1. Copy the whole folder to the machine running Tor (for example /var/lib/tor/my_site/).");
        ui.label("2. Point Tor at it in torrc:");
        ui.add(
            egui::Label::new(
                RichText::new("HiddenServiceDir /var/lib/tor/my_site/\nHiddenServicePort 80 127.0.0.1:8080").monospace(),
            )
            .selectable(true),
        );
        ui.label("3. Make the folder owned by the Tor user with permissions 700, then restart Tor.");
        ui.hyperlink_to("Full instructions", format!("{REPO_URL}#using-your-address-with-tor"));
    });
    egui::CollapsingHeader::new("How long will it take?").show(ui, |ui| {
        ui.label(
            "Every extra character makes the search 32 times longer. Roughly, on a modern PC:",
        );
        ui.label(
            "• 4–5 characters: seconds   • 6 characters: minutes   • 7 characters: about an hour",
        );
        ui.label("• 8 characters: hours to a day   • 9+ characters: weeks or longer");
        ui.label("Searching for several patterns at once costs nothing extra.");
    });
}

fn section(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::group(ui.style())
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).strong());
            ui.add_space(4.0);
            add_contents(ui);
        });
    ui.add_space(6.0);
}

/// The nearest existing parent of `path`, for the folder picker.
fn existing_ancestor(path: &Path) -> PathBuf {
    path.ancestors()
        .find(|p| p.is_dir())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

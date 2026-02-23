//! Advanced progress bars and logging system for OxiGDAL CLI
//!
//! This module provides comprehensive progress tracking and structured logging
//! capabilities for long-running geospatial operations.
//!
//! # Features
//!
//! - Multi-progress bar support for parallel operations
//! - Tile processing progress with spatial context
//! - Byte transfer progress with throughput statistics
//! - Accurate time estimates based on operation history
//! - Structured logging with multiple outputs
//! - Configurable verbosity levels
//! - File-based logging
//! - Rich color support
//!
//! # Example
//!
//! ```no_run
//! use oxigdal_cli::progress::{ProgressManager, VerbosityLevel, LogConfig};
//!
//! let config = LogConfig::new()
//!     .with_verbosity(VerbosityLevel::Info)
//!     .with_colors(true);
//! let manager = ProgressManager::new(config);
//!
//! let tile_tracker = manager.create_tile_tracker(100, 100, "Processing tiles")?;
//! for y in 0..100 {
//!     for x in 0..100 {
//!         tile_tracker.advance_tile(x, y);
//!     }
//! }
//! tile_tracker.finish();
//! # Ok::<(), anyhow::Error>(())
//! ```

use console::{Style, Term};
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use std::collections::VecDeque;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime};
use thiserror::Error;
use tracing::{Level, Subscriber};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, fmt as tracing_fmt};

/// Progress system error types
#[derive(Debug, Error)]
pub enum ProgressError {
    #[error("Failed to create log file: {0}")]
    LogFileCreation(#[source] std::io::Error),

    #[error("Failed to write to log: {0}")]
    LogWrite(#[source] std::io::Error),

    #[error("Progress style template error: {0}")]
    StyleTemplate(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Lock acquisition failed")]
    LockFailed,
}

/// Result type for progress operations
pub type ProgressResult<T> = Result<T, ProgressError>;

/// Verbosity levels for logging output
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum VerbosityLevel {
    /// Suppress all output except errors
    Quiet,
    /// Show only error messages
    Error,
    /// Show errors and warnings
    Warn,
    /// Standard informational output
    #[default]
    Info,
    /// Detailed debug information
    Debug,
    /// Maximum verbosity with trace-level details
    Trace,
}

impl VerbosityLevel {
    /// Convert to tracing Level
    #[must_use]
    pub const fn to_tracing_level(self) -> Level {
        match self {
            Self::Quiet | Self::Error => Level::ERROR,
            Self::Warn => Level::WARN,
            Self::Info => Level::INFO,
            Self::Debug => Level::DEBUG,
            Self::Trace => Level::TRACE,
        }
    }

    /// Check if logging is enabled at this level
    #[must_use]
    pub const fn is_enabled(&self, target: Self) -> bool {
        *self as u8 >= target as u8
    }
}

impl fmt::Display for VerbosityLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Quiet => write!(f, "quiet"),
            Self::Error => write!(f, "error"),
            Self::Warn => write!(f, "warn"),
            Self::Info => write!(f, "info"),
            Self::Debug => write!(f, "debug"),
            Self::Trace => write!(f, "trace"),
        }
    }
}

impl std::str::FromStr for VerbosityLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "quiet" | "q" | "0" => Ok(Self::Quiet),
            "error" | "e" | "1" => Ok(Self::Error),
            "warn" | "warning" | "w" | "2" => Ok(Self::Warn),
            "info" | "i" | "3" => Ok(Self::Info),
            "debug" | "d" | "4" => Ok(Self::Debug),
            "trace" | "t" | "5" => Ok(Self::Trace),
            _ => Err(format!("Unknown verbosity level: {}", s)),
        }
    }
}

/// Color theme configuration for progress output
#[derive(Debug, Clone)]
pub struct ColorTheme {
    /// Style for progress bar fill
    pub progress_fill: Style,
    /// Style for progress bar background
    pub progress_bg: Style,
    /// Style for success messages
    pub success: Style,
    /// Style for warning messages
    pub warning: Style,
    /// Style for error messages
    pub error: Style,
    /// Style for info messages
    pub info: Style,
    /// Style for debug messages
    pub debug: Style,
    /// Style for labels/headers
    pub label: Style,
    /// Style for values/data
    pub value: Style,
    /// Style for paths/filenames
    pub path: Style,
    /// Style for metrics/numbers
    pub metric: Style,
    /// Style for timestamps
    pub timestamp: Style,
}

impl Default for ColorTheme {
    fn default() -> Self {
        Self {
            progress_fill: Style::new().cyan(),
            progress_bg: Style::new().blue().dim(),
            success: Style::new().green().bold(),
            warning: Style::new().yellow().bold(),
            error: Style::new().red().bold(),
            info: Style::new().white(),
            debug: Style::new().dim(),
            label: Style::new().bold(),
            value: Style::new().cyan(),
            path: Style::new().green(),
            metric: Style::new().magenta(),
            timestamp: Style::new().dim(),
        }
    }
}

impl ColorTheme {
    /// Create a theme with no colors (for non-TTY output)
    #[must_use]
    pub fn plain() -> Self {
        Self {
            progress_fill: Style::new(),
            progress_bg: Style::new(),
            success: Style::new(),
            warning: Style::new(),
            error: Style::new(),
            info: Style::new(),
            debug: Style::new(),
            label: Style::new(),
            value: Style::new(),
            path: Style::new(),
            metric: Style::new(),
            timestamp: Style::new(),
        }
    }

    /// Create a dark theme optimized for dark terminals
    #[must_use]
    pub fn dark() -> Self {
        Self {
            progress_fill: Style::new().cyan().bright(),
            progress_bg: Style::new().blue().dim(),
            success: Style::new().green().bright().bold(),
            warning: Style::new().yellow().bright().bold(),
            error: Style::new().red().bright().bold(),
            info: Style::new().white().bright(),
            debug: Style::new().white().dim(),
            label: Style::new().white().bold(),
            value: Style::new().cyan().bright(),
            path: Style::new().green().bright(),
            metric: Style::new().magenta().bright(),
            timestamp: Style::new().white().dim(),
        }
    }

    /// Create a light theme optimized for light terminals
    #[must_use]
    pub fn light() -> Self {
        Self {
            progress_fill: Style::new().blue(),
            progress_bg: Style::new().black().dim(),
            success: Style::new().green(),
            warning: Style::new().yellow(),
            error: Style::new().red(),
            info: Style::new().black(),
            debug: Style::new().black().dim(),
            label: Style::new().black().bold(),
            value: Style::new().blue(),
            path: Style::new().green(),
            metric: Style::new().magenta(),
            timestamp: Style::new().black().dim(),
        }
    }
}

/// Configuration for the logging system
#[derive(Debug, Clone)]
pub struct LogConfig {
    /// Verbosity level
    pub verbosity: VerbosityLevel,
    /// Enable colored output
    pub colors_enabled: bool,
    /// Color theme
    pub theme: ColorTheme,
    /// Log file path (optional)
    pub log_file: Option<PathBuf>,
    /// Include timestamps in output
    pub show_timestamps: bool,
    /// Include target module in output
    pub show_targets: bool,
    /// Include thread IDs in output
    pub show_thread_ids: bool,
    /// Include source file/line info
    pub show_source_location: bool,
    /// Maximum log line width (0 = no limit)
    pub max_line_width: usize,
    /// Structured JSON logging
    pub json_output: bool,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            verbosity: VerbosityLevel::Info,
            colors_enabled: true,
            theme: ColorTheme::default(),
            log_file: None,
            show_timestamps: false,
            show_targets: false,
            show_thread_ids: false,
            show_source_location: false,
            max_line_width: 0,
            json_output: false,
        }
    }
}

impl LogConfig {
    /// Create a new log configuration
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set verbosity level
    #[must_use]
    pub fn with_verbosity(mut self, level: VerbosityLevel) -> Self {
        self.verbosity = level;
        self
    }

    /// Enable or disable colors
    #[must_use]
    pub fn with_colors(mut self, enabled: bool) -> Self {
        self.colors_enabled = enabled;
        if !enabled {
            self.theme = ColorTheme::plain();
        }
        self
    }

    /// Set color theme
    #[must_use]
    pub fn with_theme(mut self, theme: ColorTheme) -> Self {
        self.theme = theme;
        self
    }

    /// Set log file path
    #[must_use]
    pub fn with_log_file(mut self, path: PathBuf) -> Self {
        self.log_file = Some(path);
        self
    }

    /// Enable timestamps
    #[must_use]
    pub fn with_timestamps(mut self, enabled: bool) -> Self {
        self.show_timestamps = enabled;
        self
    }

    /// Enable target module display
    #[must_use]
    pub fn with_targets(mut self, enabled: bool) -> Self {
        self.show_targets = enabled;
        self
    }

    /// Enable thread ID display
    #[must_use]
    pub fn with_thread_ids(mut self, enabled: bool) -> Self {
        self.show_thread_ids = enabled;
        self
    }

    /// Enable source location display
    #[must_use]
    pub fn with_source_location(mut self, enabled: bool) -> Self {
        self.show_source_location = enabled;
        self
    }

    /// Enable JSON output
    #[must_use]
    pub fn with_json(mut self, enabled: bool) -> Self {
        self.json_output = enabled;
        self
    }

    /// Set maximum line width
    #[must_use]
    pub fn with_max_width(mut self, width: usize) -> Self {
        self.max_line_width = width;
        self
    }

    /// Check if TTY is available and configure colors accordingly
    #[must_use]
    pub fn auto_colors(mut self) -> Self {
        self.colors_enabled = Term::stdout().is_term();
        if !self.colors_enabled {
            self.theme = ColorTheme::plain();
        }
        self
    }
}

/// Time estimation tracker for accurate ETA calculations
#[derive(Debug)]
pub struct TimeEstimator {
    /// Start time of the operation
    start_time: Instant,
    /// History of completion rates (items per second)
    rate_history: VecDeque<f64>,
    /// Maximum history size
    history_size: usize,
    /// Total items to process
    total_items: u64,
    /// Items processed so far
    processed_items: AtomicU64,
    /// Last update timestamp
    last_update: RwLock<Instant>,
    /// Items at last update
    last_items: AtomicU64,
    /// Smoothing factor for exponential moving average
    smoothing_factor: f64,
    /// Current smoothed rate
    smoothed_rate: RwLock<f64>,
}

impl TimeEstimator {
    /// Create a new time estimator
    #[must_use]
    pub fn new(total_items: u64) -> Self {
        Self {
            start_time: Instant::now(),
            rate_history: VecDeque::with_capacity(20),
            history_size: 20,
            total_items,
            processed_items: AtomicU64::new(0),
            last_update: RwLock::new(Instant::now()),
            last_items: AtomicU64::new(0),
            smoothing_factor: 0.3,
            smoothed_rate: RwLock::new(0.0),
        }
    }

    /// Create with custom history size
    #[must_use]
    pub fn with_history_size(mut self, size: usize) -> Self {
        self.history_size = size;
        self.rate_history = VecDeque::with_capacity(size);
        self
    }

    /// Update the progress
    pub fn update(&self, processed: u64) {
        self.processed_items.store(processed, Ordering::Relaxed);

        let now = Instant::now();
        let last_update = self.last_update.read().map_or(now, |guard| *guard);
        let elapsed = now.duration_since(last_update);

        // Only update rate every 100ms to avoid noise
        if elapsed >= Duration::from_millis(100) {
            let last_items = self.last_items.load(Ordering::Relaxed);
            let items_delta = processed.saturating_sub(last_items);
            let rate = items_delta as f64 / elapsed.as_secs_f64();

            // Update smoothed rate using exponential moving average
            if let Ok(mut smoothed) = self.smoothed_rate.write() {
                if *smoothed == 0.0 {
                    *smoothed = rate;
                } else {
                    *smoothed = self.smoothing_factor * rate + (1.0 - self.smoothing_factor) * *smoothed;
                }
            }

            // Update last values
            if let Ok(mut guard) = self.last_update.write() {
                *guard = now;
            }
            self.last_items.store(processed, Ordering::Relaxed);
        }
    }

    /// Increment progress by one
    pub fn increment(&self) {
        let processed = self.processed_items.fetch_add(1, Ordering::Relaxed) + 1;
        self.update(processed);
    }

    /// Get elapsed time
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Get estimated time remaining
    #[must_use]
    pub fn eta(&self) -> Option<Duration> {
        let processed = self.processed_items.load(Ordering::Relaxed);
        if processed == 0 {
            return None;
        }

        let remaining = self.total_items.saturating_sub(processed);
        if remaining == 0 {
            return Some(Duration::ZERO);
        }

        let rate = self.smoothed_rate.read().ok().map(|r| *r)?;
        if rate <= 0.0 {
            return None;
        }

        let eta_secs = remaining as f64 / rate;
        Some(Duration::from_secs_f64(eta_secs))
    }

    /// Get current processing rate (items per second)
    #[must_use]
    pub fn rate(&self) -> f64 {
        self.smoothed_rate.read().map_or(0.0, |r| *r)
    }

    /// Get progress percentage
    #[must_use]
    pub fn percentage(&self) -> f64 {
        if self.total_items == 0 {
            return 100.0;
        }
        let processed = self.processed_items.load(Ordering::Relaxed);
        (processed as f64 / self.total_items as f64) * 100.0
    }

    /// Format ETA as human-readable string
    #[must_use]
    pub fn format_eta(&self) -> String {
        match self.eta() {
            Some(eta) if eta.as_secs() == 0 => "< 1s".to_string(),
            Some(eta) => format_duration(eta),
            None => "calculating...".to_string(),
        }
    }
}

/// Format duration as human-readable string
fn format_duration(duration: Duration) -> String {
    let total_secs = duration.as_secs();

    if total_secs < 60 {
        return format!("{}s", total_secs);
    }

    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, seconds)
    } else {
        format!("{}m {}s", minutes, seconds)
    }
}

/// Tile-aware progress tracker for raster operations
#[derive(Debug)]
pub struct TileProgressTracker {
    /// Progress bar
    progress_bar: ProgressBar,
    /// Time estimator
    estimator: Arc<TimeEstimator>,
    /// Number of tiles in X direction
    tiles_x: u64,
    /// Number of tiles in Y direction
    tiles_y: u64,
    /// Current tile X
    current_x: AtomicU64,
    /// Current tile Y
    current_y: AtomicU64,
    /// Whether to show spatial position
    show_position: bool,
}

impl TileProgressTracker {
    /// Create a new tile progress tracker
    pub fn new(
        multi: &MultiProgress,
        tiles_x: u64,
        tiles_y: u64,
        message: &str,
    ) -> ProgressResult<Self> {
        let total = tiles_x * tiles_y;
        let estimator = Arc::new(TimeEstimator::new(total));

        let style = ProgressStyle::default_bar()
            .template("{msg} [{bar:40.cyan/blue}] {pos}/{len} tiles ({percent}%) | ETA: {eta} | {per_sec}")
            .map_err(|e| ProgressError::StyleTemplate(e.to_string()))?
            .progress_chars("#>-");

        let pb = multi.add(ProgressBar::new(total));
        pb.set_style(style);
        pb.set_message(message.to_string());
        pb.enable_steady_tick(Duration::from_millis(100));

        Ok(Self {
            progress_bar: pb,
            estimator,
            tiles_x,
            tiles_y,
            current_x: AtomicU64::new(0),
            current_y: AtomicU64::new(0),
            show_position: true,
        })
    }

    /// Enable or disable spatial position display
    #[must_use]
    pub fn with_position_display(self, enabled: bool) -> Self {
        Self {
            show_position: enabled,
            ..self
        }
    }

    /// Advance to the next tile
    pub fn advance(&self) {
        self.progress_bar.inc(1);
        self.estimator.increment();
    }

    /// Advance to a specific tile position
    pub fn advance_tile(&self, x: u64, y: u64) {
        self.current_x.store(x, Ordering::Relaxed);
        self.current_y.store(y, Ordering::Relaxed);

        let pos = y * self.tiles_x + x;
        self.progress_bar.set_position(pos);
        self.estimator.update(pos);

        if self.show_position {
            self.progress_bar.set_message(format!(
                "Processing tile ({}, {}) of ({}, {})",
                x, y, self.tiles_x, self.tiles_y
            ));
        }
    }

    /// Get the current tile position
    #[must_use]
    pub fn current_position(&self) -> (u64, u64) {
        (
            self.current_x.load(Ordering::Relaxed),
            self.current_y.load(Ordering::Relaxed),
        )
    }

    /// Get estimated time remaining
    #[must_use]
    pub fn eta(&self) -> Option<Duration> {
        self.estimator.eta()
    }

    /// Finish the progress bar
    pub fn finish(&self) {
        self.progress_bar.finish_with_message("Completed");
    }

    /// Finish with error
    pub fn finish_with_error(&self, error: &str) {
        self.progress_bar.abandon_with_message(format!("Error: {}", error));
    }
}

/// Byte transfer progress tracker with throughput statistics
#[derive(Debug)]
pub struct ByteTransferTracker {
    /// Progress bar
    progress_bar: ProgressBar,
    /// Time estimator
    estimator: Arc<TimeEstimator>,
    /// Total bytes to transfer
    total_bytes: u64,
    /// Bytes transferred so far
    transferred_bytes: AtomicU64,
    /// Peak transfer rate
    peak_rate: RwLock<f64>,
    /// Transfer start time
    start_time: Instant,
}

impl ByteTransferTracker {
    /// Create a new byte transfer tracker
    pub fn new(
        multi: &MultiProgress,
        total_bytes: u64,
        label: &str,
    ) -> ProgressResult<Self> {
        let estimator = Arc::new(TimeEstimator::new(total_bytes));

        let style = ProgressStyle::default_bar()
            .template("{msg} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}) | ETA: {eta}")
            .map_err(|e| ProgressError::StyleTemplate(e.to_string()))?
            .progress_chars("#>-");

        let pb = multi.add(ProgressBar::new(total_bytes));
        pb.set_style(style);
        pb.set_message(label.to_string());
        pb.enable_steady_tick(Duration::from_millis(100));

        Ok(Self {
            progress_bar: pb,
            estimator,
            total_bytes,
            transferred_bytes: AtomicU64::new(0),
            peak_rate: RwLock::new(0.0),
            start_time: Instant::now(),
        })
    }

    /// Add transferred bytes
    pub fn add_bytes(&self, bytes: u64) {
        let total = self.transferred_bytes.fetch_add(bytes, Ordering::Relaxed) + bytes;
        self.progress_bar.set_position(total);
        self.estimator.update(total);

        // Track peak rate
        let current_rate = self.estimator.rate();
        if let Ok(mut peak) = self.peak_rate.write() {
            if current_rate > *peak {
                *peak = current_rate;
            }
        }
    }

    /// Set absolute position
    pub fn set_position(&self, bytes: u64) {
        self.transferred_bytes.store(bytes, Ordering::Relaxed);
        self.progress_bar.set_position(bytes);
        self.estimator.update(bytes);
    }

    /// Get current transfer rate (bytes per second)
    #[must_use]
    pub fn rate(&self) -> f64 {
        self.estimator.rate()
    }

    /// Get peak transfer rate
    #[must_use]
    pub fn peak_rate(&self) -> f64 {
        self.peak_rate.read().map_or(0.0, |r| *r)
    }

    /// Get average transfer rate
    #[must_use]
    pub fn average_rate(&self) -> f64 {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        if elapsed == 0.0 {
            return 0.0;
        }
        self.transferred_bytes.load(Ordering::Relaxed) as f64 / elapsed
    }

    /// Format rate as human-readable string
    #[must_use]
    pub fn format_rate(&self) -> String {
        format_bytes_per_sec(self.rate())
    }

    /// Get statistics summary
    #[must_use]
    pub fn statistics(&self) -> TransferStatistics {
        TransferStatistics {
            total_bytes: self.total_bytes,
            transferred_bytes: self.transferred_bytes.load(Ordering::Relaxed),
            elapsed: self.start_time.elapsed(),
            current_rate: self.rate(),
            peak_rate: self.peak_rate(),
            average_rate: self.average_rate(),
        }
    }

    /// Finish the transfer
    pub fn finish(&self) {
        let stats = self.statistics();
        self.progress_bar.finish_with_message(format!(
            "Completed - Avg: {}/s, Peak: {}/s",
            format_bytes(stats.average_rate as u64),
            format_bytes(stats.peak_rate as u64)
        ));
    }

    /// Finish with error
    pub fn finish_with_error(&self, error: &str) {
        self.progress_bar.abandon_with_message(format!("Error: {}", error));
    }
}

/// Transfer statistics
#[derive(Debug, Clone)]
pub struct TransferStatistics {
    /// Total bytes to transfer
    pub total_bytes: u64,
    /// Bytes transferred
    pub transferred_bytes: u64,
    /// Time elapsed
    pub elapsed: Duration,
    /// Current transfer rate
    pub current_rate: f64,
    /// Peak transfer rate
    pub peak_rate: f64,
    /// Average transfer rate
    pub average_rate: f64,
}

impl TransferStatistics {
    /// Get percentage complete
    #[must_use]
    pub fn percentage(&self) -> f64 {
        if self.total_bytes == 0 {
            return 100.0;
        }
        (self.transferred_bytes as f64 / self.total_bytes as f64) * 100.0
    }
}

/// Format bytes as human-readable string
fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB", "PB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;

    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }

    if unit_idx == 0 {
        format!("{} {}", bytes, UNITS[unit_idx])
    } else {
        format!("{:.2} {}", size, UNITS[unit_idx])
    }
}

/// Format bytes per second as human-readable string
fn format_bytes_per_sec(bps: f64) -> String {
    format!("{}/s", format_bytes(bps as u64))
}

/// Multi-operation progress manager
#[derive(Debug)]
pub struct ProgressManager {
    /// Multi-progress container
    multi: MultiProgress,
    /// Configuration
    config: LogConfig,
    /// Active trackers count
    active_trackers: AtomicU64,
    /// Whether the manager is finished
    finished: AtomicBool,
    /// Log file writer (if enabled)
    log_writer: Option<Arc<Mutex<BufWriter<File>>>>,
}

impl ProgressManager {
    /// Create a new progress manager
    pub fn new(config: LogConfig) -> ProgressResult<Self> {
        let log_writer = if let Some(ref path) = config.log_file {
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(ProgressError::LogFileCreation)?;
            Some(Arc::new(Mutex::new(BufWriter::new(file))))
        } else {
            None
        };

        let multi = MultiProgress::new();

        // Configure draw target based on colors/TTY
        if !config.colors_enabled || !Term::stdout().is_term() {
            multi.set_draw_target(ProgressDrawTarget::hidden());
        }

        Ok(Self {
            multi,
            config,
            active_trackers: AtomicU64::new(0),
            finished: AtomicBool::new(false),
            log_writer,
        })
    }

    /// Create a simple progress bar
    pub fn create_progress_bar(&self, total: u64, message: &str) -> ProgressResult<ProgressBar> {
        let style = ProgressStyle::default_bar()
            .template("{msg} [{bar:40.cyan/blue}] {pos}/{len} ({percent}%) | ETA: {eta}")
            .map_err(|e| ProgressError::StyleTemplate(e.to_string()))?
            .progress_chars("#>-");

        let pb = self.multi.add(ProgressBar::new(total));
        pb.set_style(style);
        pb.set_message(message.to_string());
        pb.enable_steady_tick(Duration::from_millis(100));

        self.active_trackers.fetch_add(1, Ordering::Relaxed);
        Ok(pb)
    }

    /// Create a spinner for indeterminate operations
    pub fn create_spinner(&self, message: &str) -> ProgressResult<ProgressBar> {
        let style = ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg} [{elapsed_precise}]")
            .map_err(|e| ProgressError::StyleTemplate(e.to_string()))?
            .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ");

        let pb = self.multi.add(ProgressBar::new_spinner());
        pb.set_style(style);
        pb.set_message(message.to_string());
        pb.enable_steady_tick(Duration::from_millis(80));

        self.active_trackers.fetch_add(1, Ordering::Relaxed);
        Ok(pb)
    }

    /// Create a tile progress tracker
    pub fn create_tile_tracker(
        &self,
        tiles_x: u64,
        tiles_y: u64,
        message: &str,
    ) -> ProgressResult<TileProgressTracker> {
        self.active_trackers.fetch_add(1, Ordering::Relaxed);
        TileProgressTracker::new(&self.multi, tiles_x, tiles_y, message)
    }

    /// Create a byte transfer tracker
    pub fn create_transfer_tracker(
        &self,
        total_bytes: u64,
        label: &str,
    ) -> ProgressResult<ByteTransferTracker> {
        self.active_trackers.fetch_add(1, Ordering::Relaxed);
        ByteTransferTracker::new(&self.multi, total_bytes, label)
    }

    /// Log a message at the specified level
    pub fn log(&self, level: VerbosityLevel, message: &str) {
        if !self.config.verbosity.is_enabled(level) {
            return;
        }

        let styled_message = if self.config.colors_enabled {
            let style = match level {
                VerbosityLevel::Error => &self.config.theme.error,
                VerbosityLevel::Warn => &self.config.theme.warning,
                VerbosityLevel::Info => &self.config.theme.info,
                VerbosityLevel::Debug | VerbosityLevel::Trace => &self.config.theme.debug,
                VerbosityLevel::Quiet => return,
            };
            format!("{}", style.apply_to(message))
        } else {
            message.to_string()
        };

        // Print to console
        if !matches!(level, VerbosityLevel::Quiet) {
            self.multi.println(&styled_message).ok();
        }

        // Write to log file
        if let Some(ref writer) = self.log_writer {
            self.write_to_log(level, message);
        }
    }

    /// Write to log file
    fn write_to_log(&self, level: VerbosityLevel, message: &str) {
        if let Some(ref writer) = self.log_writer {
            if let Ok(mut guard) = writer.lock() {
                let timestamp = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);

                let log_line = if self.config.json_output {
                    format!(
                        r#"{{"timestamp":{},"level":"{}","message":"{}"}}"#,
                        timestamp,
                        level,
                        message.replace('"', "\\\"")
                    )
                } else {
                    format!("[{}] [{}] {}", timestamp, level, message)
                };

                writeln!(guard, "{}", log_line).ok();
                guard.flush().ok();
            }
        }
    }

    /// Log an error message
    pub fn error(&self, message: &str) {
        self.log(VerbosityLevel::Error, message);
    }

    /// Log a warning message
    pub fn warn(&self, message: &str) {
        self.log(VerbosityLevel::Warn, message);
    }

    /// Log an info message
    pub fn info(&self, message: &str) {
        self.log(VerbosityLevel::Info, message);
    }

    /// Log a debug message
    pub fn debug(&self, message: &str) {
        self.log(VerbosityLevel::Debug, message);
    }

    /// Log a trace message
    pub fn trace(&self, message: &str) {
        self.log(VerbosityLevel::Trace, message);
    }

    /// Suspend progress bars for clean output
    pub fn suspend<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        self.multi.suspend(f)
    }

    /// Get the multi-progress container
    #[must_use]
    pub fn multi(&self) -> &MultiProgress {
        &self.multi
    }

    /// Get current configuration
    #[must_use]
    pub fn config(&self) -> &LogConfig {
        &self.config
    }

    /// Clear all progress bars
    pub fn clear(&self) {
        self.multi.clear().ok();
    }

    /// Check if any trackers are active
    #[must_use]
    pub fn has_active_trackers(&self) -> bool {
        self.active_trackers.load(Ordering::Relaxed) > 0
    }

    /// Mark a tracker as complete (decrement counter)
    pub fn tracker_complete(&self) {
        self.active_trackers.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Initialize the global logging system
pub fn init_logging(config: &LogConfig) -> ProgressResult<()> {
    let level = config.verbosity.to_tracing_level();

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(level.to_string()));

    let fmt_layer = tracing_fmt::layer()
        .with_ansi(config.colors_enabled)
        .with_target(config.show_targets)
        .with_thread_ids(config.show_thread_ids)
        .with_file(config.show_source_location)
        .with_line_number(config.show_source_location);

    if config.json_output {
        let json_layer = tracing_fmt::layer()
            .json()
            .with_target(config.show_targets)
            .with_thread_ids(config.show_thread_ids)
            .with_file(config.show_source_location)
            .with_line_number(config.show_source_location);

        tracing_subscriber::registry()
            .with(filter)
            .with(json_layer)
            .try_init()
            .map_err(|e| ProgressError::InvalidConfig(e.to_string()))?;
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt_layer)
            .try_init()
            .map_err(|e| ProgressError::InvalidConfig(e.to_string()))?;
    }

    Ok(())
}

/// Styled message builder for complex output
#[derive(Debug)]
pub struct StyledMessage {
    parts: Vec<String>,
    theme: ColorTheme,
}

impl StyledMessage {
    /// Create a new styled message builder
    #[must_use]
    pub fn new(theme: ColorTheme) -> Self {
        Self {
            parts: Vec::new(),
            theme,
        }
    }

    /// Add plain text
    #[must_use]
    pub fn text(mut self, text: &str) -> Self {
        self.parts.push(text.to_string());
        self
    }

    /// Add a label
    #[must_use]
    pub fn label(mut self, text: &str) -> Self {
        self.parts.push(format!("{}", self.theme.label.apply_to(text)));
        self
    }

    /// Add a value
    #[must_use]
    pub fn value(mut self, text: &str) -> Self {
        self.parts.push(format!("{}", self.theme.value.apply_to(text)));
        self
    }

    /// Add a path
    #[must_use]
    pub fn path(mut self, text: &str) -> Self {
        self.parts.push(format!("{}", self.theme.path.apply_to(text)));
        self
    }

    /// Add a metric
    #[must_use]
    pub fn metric(mut self, text: &str) -> Self {
        self.parts.push(format!("{}", self.theme.metric.apply_to(text)));
        self
    }

    /// Add success text
    #[must_use]
    pub fn success(mut self, text: &str) -> Self {
        self.parts.push(format!("{}", self.theme.success.apply_to(text)));
        self
    }

    /// Add warning text
    #[must_use]
    pub fn warning(mut self, text: &str) -> Self {
        self.parts.push(format!("{}", self.theme.warning.apply_to(text)));
        self
    }

    /// Add error text
    #[must_use]
    pub fn error(mut self, text: &str) -> Self {
        self.parts.push(format!("{}", self.theme.error.apply_to(text)));
        self
    }

    /// Build the final message
    #[must_use]
    pub fn build(self) -> String {
        self.parts.join("")
    }
}

impl fmt::Display for StyledMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for part in &self.parts {
            write!(f, "{}", part)?;
        }
        Ok(())
    }
}

/// Progress bar presets for common operations
pub mod presets {
    use super::*;

    /// Create a raster processing progress bar
    pub fn raster_processing(multi: &MultiProgress, total: u64) -> ProgressResult<ProgressBar> {
        let style = ProgressStyle::default_bar()
            .template("{spinner:.green} {msg} [{bar:40.cyan/blue}] {pos}/{len} ({percent}%) | {per_sec} | ETA: {eta}")
            .map_err(|e| ProgressError::StyleTemplate(e.to_string()))?
            .progress_chars("=>-");

        let pb = multi.add(ProgressBar::new(total));
        pb.set_style(style);
        pb.enable_steady_tick(Duration::from_millis(100));
        Ok(pb)
    }

    /// Create a file I/O progress bar
    pub fn file_io(multi: &MultiProgress, total_bytes: u64) -> ProgressResult<ProgressBar> {
        let style = ProgressStyle::default_bar()
            .template("{msg} [{bar:40.green/dim}] {bytes}/{total_bytes} | {bytes_per_sec} | {eta}")
            .map_err(|e| ProgressError::StyleTemplate(e.to_string()))?
            .progress_chars("#>-");

        let pb = multi.add(ProgressBar::new(total_bytes));
        pb.set_style(style);
        pb.enable_steady_tick(Duration::from_millis(100));
        Ok(pb)
    }

    /// Create a validation progress bar
    pub fn validation(multi: &MultiProgress, total: u64) -> ProgressResult<ProgressBar> {
        let style = ProgressStyle::default_bar()
            .template("{spinner:.yellow} Validating... [{bar:40.yellow/dim}] {pos}/{len} checks")
            .map_err(|e| ProgressError::StyleTemplate(e.to_string()))?
            .progress_chars("*>-");

        let pb = multi.add(ProgressBar::new(total));
        pb.set_style(style);
        pb.enable_steady_tick(Duration::from_millis(80));
        Ok(pb)
    }

    /// Create a conversion progress bar
    pub fn conversion(multi: &MultiProgress, total: u64) -> ProgressResult<ProgressBar> {
        let style = ProgressStyle::default_bar()
            .template("{spinner:.magenta} Converting... [{bar:40.magenta/dim}] {pos}/{len} ({percent}%)")
            .map_err(|e| ProgressError::StyleTemplate(e.to_string()))?
            .progress_chars("#>-");

        let pb = multi.add(ProgressBar::new(total));
        pb.set_style(style);
        pb.enable_steady_tick(Duration::from_millis(100));
        Ok(pb)
    }

    /// Create a multi-band processing progress bar
    pub fn band_processing(multi: &MultiProgress, total_bands: u64) -> ProgressResult<ProgressBar> {
        let style = ProgressStyle::default_bar()
            .template("{msg} Band {pos}/{len} [{bar:30.cyan/blue}] {percent}%")
            .map_err(|e| ProgressError::StyleTemplate(e.to_string()))?
            .progress_chars("#>-");

        let pb = multi.add(ProgressBar::new(total_bands));
        pb.set_style(style);
        pb.enable_steady_tick(Duration::from_millis(100));
        Ok(pb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_verbosity_level_ordering() {
        assert!(VerbosityLevel::Quiet < VerbosityLevel::Error);
        assert!(VerbosityLevel::Error < VerbosityLevel::Warn);
        assert!(VerbosityLevel::Warn < VerbosityLevel::Info);
        assert!(VerbosityLevel::Info < VerbosityLevel::Debug);
        assert!(VerbosityLevel::Debug < VerbosityLevel::Trace);
    }

    #[test]
    fn test_verbosity_level_parsing() {
        assert_eq!(
            "info".parse::<VerbosityLevel>().ok(),
            Some(VerbosityLevel::Info)
        );
        assert_eq!(
            "debug".parse::<VerbosityLevel>().ok(),
            Some(VerbosityLevel::Debug)
        );
        assert_eq!(
            "quiet".parse::<VerbosityLevel>().ok(),
            Some(VerbosityLevel::Quiet)
        );
        assert!("invalid".parse::<VerbosityLevel>().is_err());
    }

    #[test]
    fn test_verbosity_is_enabled() {
        let level = VerbosityLevel::Info;
        assert!(level.is_enabled(VerbosityLevel::Error));
        assert!(level.is_enabled(VerbosityLevel::Warn));
        assert!(level.is_enabled(VerbosityLevel::Info));
        assert!(!level.is_enabled(VerbosityLevel::Debug));
        assert!(!level.is_enabled(VerbosityLevel::Trace));
    }

    #[test]
    fn test_log_config_builder() {
        let config = LogConfig::new()
            .with_verbosity(VerbosityLevel::Debug)
            .with_colors(true)
            .with_timestamps(true)
            .with_targets(true);

        assert_eq!(config.verbosity, VerbosityLevel::Debug);
        assert!(config.colors_enabled);
        assert!(config.show_timestamps);
        assert!(config.show_targets);
    }

    #[test]
    fn test_time_estimator() {
        let estimator = TimeEstimator::new(100);

        // Update progress
        estimator.update(25);
        thread::sleep(Duration::from_millis(10));
        estimator.update(50);

        assert!(estimator.elapsed() > Duration::ZERO);
        assert!(estimator.percentage() >= 50.0);
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(Duration::from_secs(30)), "30s");
        assert_eq!(format_duration(Duration::from_secs(90)), "1m 30s");
        assert_eq!(format_duration(Duration::from_secs(3661)), "1h 1m 1s");
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(1_048_576), "1.00 MB");
        assert_eq!(format_bytes(1_073_741_824), "1.00 GB");
    }

    #[test]
    fn test_color_themes() {
        let default = ColorTheme::default();
        let plain = ColorTheme::plain();
        let dark = ColorTheme::dark();
        let light = ColorTheme::light();

        // Just ensure they can be created without panicking
        let _ = default.success.apply_to("test");
        let _ = plain.success.apply_to("test");
        let _ = dark.success.apply_to("test");
        let _ = light.success.apply_to("test");
    }

    #[test]
    fn test_styled_message_builder() {
        let theme = ColorTheme::default();
        let msg = StyledMessage::new(theme)
            .label("File: ")
            .path("/path/to/file.tif")
            .text(" - ")
            .metric("1.5 MB")
            .build();

        assert!(!msg.is_empty());
    }

    #[test]
    fn test_progress_manager_creation() {
        let config = LogConfig::new().with_colors(false);
        let manager = ProgressManager::new(config);
        assert!(manager.is_ok());
    }

    #[test]
    fn test_progress_manager_spinner() {
        let config = LogConfig::new().with_colors(false);
        let manager = ProgressManager::new(config).expect("Failed to create manager");

        let spinner = manager.create_spinner("Processing...");
        assert!(spinner.is_ok());

        if let Ok(pb) = spinner {
            pb.finish();
        }
    }

    #[test]
    fn test_transfer_statistics() {
        let stats = TransferStatistics {
            total_bytes: 1000,
            transferred_bytes: 500,
            elapsed: Duration::from_secs(5),
            current_rate: 100.0,
            peak_rate: 150.0,
            average_rate: 100.0,
        };

        assert!((stats.percentage() - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_presets_creation() {
        let multi = MultiProgress::new();
        multi.set_draw_target(ProgressDrawTarget::hidden());

        let raster = presets::raster_processing(&multi, 100);
        assert!(raster.is_ok());

        let file_io = presets::file_io(&multi, 1024);
        assert!(file_io.is_ok());

        let validation = presets::validation(&multi, 10);
        assert!(validation.is_ok());

        let conversion = presets::conversion(&multi, 50);
        assert!(conversion.is_ok());

        let band = presets::band_processing(&multi, 4);
        assert!(band.is_ok());
    }
}

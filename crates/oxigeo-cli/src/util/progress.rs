//! Progress bar utilities for long-running operations

use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

/// Create a progress bar for processing operations
pub fn create_progress_bar(total: u64, message: &str) -> ProgressBar {
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{msg} [{bar:40.cyan/blue}] {pos}/{len} ({percent}%) {elapsed_precise}")
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("#>-"),
    );
    pb.set_message(message.to_string());
    pb.enable_steady_tick(Duration::from_millis(100));
    pb
}

/// Create a spinner for indeterminate operations
pub fn create_spinner(message: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner())
            .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ "),
    );
    pb.set_message(message.to_string());
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}

/// Create a progress bar for file transfer operations
#[allow(dead_code)]
pub fn create_transfer_bar(total_bytes: u64, file_name: &str) -> ProgressBar {
    let pb = ProgressBar::new(total_bytes);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{msg} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}) {eta}")
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("#>-"),
    );
    pb.set_message(format!("Transferring {}", file_name));
    pb
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_progress_bar() {
        let pb = create_progress_bar(100, "Testing");
        assert_eq!(pb.length(), Some(100));
        pb.finish();
    }

    #[test]
    fn test_create_spinner() {
        let pb = create_spinner("Processing");
        assert!(pb.is_hidden() || !pb.is_finished());
        pb.finish();
    }

    #[test]
    fn test_create_transfer_bar() {
        let pb = create_transfer_bar(1024, "test.tif");
        assert_eq!(pb.length(), Some(1024));
        pb.finish();
    }
}

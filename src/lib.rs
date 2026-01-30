pub mod api;
pub mod backtest;
pub mod detector;
pub mod config;
pub mod merge;
pub mod models;
pub mod monitor;
pub mod simulation;
pub mod trader;

// Re-export commonly used types
pub use api::PolymarketApi;
pub use config::Config;
pub use models::TokenPrice;

// Global file writer for history.toml (initialized by main.rs)
use std::sync::{Mutex, OnceLock};
use std::fs::File;
use std::io::Write;

static HISTORY_FILE: OnceLock<Mutex<File>> = OnceLock::new();

/// Initialize the global history file writer (called by main.rs)
pub fn init_history_file(file: File) {
    HISTORY_FILE.set(Mutex::new(file)).expect("History file already initialized");
}

// Logging functions - modules will use these
pub fn log_to_history(message: &str) {
    // Write to stderr
    eprint!("{}", message);
    use std::io::Write;
    let _ = std::io::stderr().flush();
    
    // Write to history file if initialized
    if let Some(file_mutex) = HISTORY_FILE.get() {
        if let Ok(mut file) = file_mutex.lock() {
            let _ = write!(file, "{}", message);
            let _ = file.flush();
        }
    }
}

pub fn log_trading_event(event: &str) {
    // Write structured trading event with timestamp
    use chrono::Utc;
    let timestamp = Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
    log_to_history(&format!("[{}] {}\n", timestamp, event));
}

// Macro for logging - modules use crate::log_println!
#[macro_export]
macro_rules! log_println {
    ($($arg:tt)*) => {
        {
            let message = format!($($arg)*);
            $crate::log_to_history(&format!("{}\n", message));
        }
    };
}

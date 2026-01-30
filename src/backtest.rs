// Backtest module: simulate trading strategies using historical price data

use crate::config::Config;
use anyhow::{Context, Result};
use chrono::DateTime;
use regex::Regex;
use std::fs;
use std::path::Path;

/// Represents a price snapshot from history
#[derive(Debug, Clone)]
pub struct PriceSnapshot {
    pub timestamp: DateTime<chrono::Utc>,
    pub time_remaining_seconds: u64,
    pub btc_up_bid: Option<f64>,
    pub btc_up_ask: Option<f64>,
    pub btc_down_bid: Option<f64>,
    pub btc_down_ask: Option<f64>,
    pub eth_up_bid: Option<f64>,
    pub eth_up_ask: Option<f64>,
    pub eth_down_bid: Option<f64>,
    pub eth_down_ask: Option<f64>,
    pub solana_up_bid: Option<f64>,
    pub solana_up_ask: Option<f64>,
    pub solana_down_bid: Option<f64>,
    pub solana_down_ask: Option<f64>,
    pub xrp_up_bid: Option<f64>,
    pub xrp_up_ask: Option<f64>,
    pub xrp_down_bid: Option<f64>,
    pub xrp_down_ask: Option<f64>,
}

/// Represents a position in backtest
#[derive(Debug, Clone)]
pub struct BacktestPosition {
    pub token_type: String, // "BTC_UP", "BTC_DOWN", etc.
    pub shares: f64,
    pub purchase_price: f64,
    pub purchase_time_remaining: u64, // seconds remaining when purchased
}

/// Results for a single period backtest
#[derive(Debug, Clone)]
pub struct PeriodResult {
    pub period_timestamp: u64,
    pub up_won: bool,
    pub positions: Vec<BacktestPosition>,
    pub total_cost: f64,
    pub total_value: f64,
    pub pnl: f64,
}

/// Aggregate backtest results
#[derive(Debug)]
pub struct BacktestResults {
    pub period_results: Vec<PeriodResult>,
    pub total_periods: usize,
    pub total_cost: f64,
    pub total_value: f64,
    pub total_pnl: f64,
    pub winning_periods: usize,
    pub losing_periods: usize,
}

/// Parse a price line from history file
/// Format: [TIMESTAMP] 📊 BTC: U$bid/$ask D$bid/$ask | ETH: U$bid/$ask D$bid/$ask | SOL: U$bid/$ask D$bid/$ask | XRP: U$bid/$ask D$bid/$ask | ⏱️  TIME_REMAINING
fn parse_price_line(line: &str) -> Option<PriceSnapshot> {
    // Parse timestamp: [2026-01-27T21:30:02Z]
    let timestamp_re = Regex::new(r"\[([^\]]+)\]").ok()?;
    let timestamp_str = timestamp_re.captures(line)?.get(1)?.as_str();
    let timestamp = DateTime::parse_from_rfc3339(timestamp_str)
        .ok()?
        .with_timezone(&chrono::Utc);

    // Parse time remaining: ⏱️  14m 59s or ⏱️  0s
    let time_re = Regex::new(r"⏱️\s+(\d+)m\s+(\d+)s|⏱️\s+(\d+)s").ok()?;
    let time_remaining = if let Some(caps) = time_re.captures(line) {
        if let Some(minutes) = caps.get(1) {
            let mins: u64 = minutes.as_str().parse().ok()?;
            let secs: u64 = caps.get(2)?.as_str().parse().ok()?;
            mins * 60 + secs
        } else {
            caps.get(3)?.as_str().parse().ok()?
        }
    } else {
        return None;
    };

    // Parse prices using string splitting (more reliable than regex with $)
    // Format: BTC: U$0.49/$0.50 D$0.50/$0.51 | ETH: ...
    let parse_price = |s: &str| -> Option<f64> {
        let s = s.trim();
        if s == "N/A" || s.is_empty() {
            None
        } else {
            s.parse().ok()
        }
    };
    
    let mut btc_up_bid = None;
    let mut btc_up_ask = None;
    let mut btc_down_bid = None;
    let mut btc_down_ask = None;
    let mut eth_up_bid = None;
    let mut eth_up_ask = None;
    let mut eth_down_bid = None;
    let mut eth_down_ask = None;
    let mut solana_up_bid = None;
    let mut solana_up_ask = None;
    let mut solana_down_bid = None;
    let mut solana_down_ask = None;
    let mut xrp_up_bid = None;
    let mut xrp_up_ask = None;
    let mut xrp_down_bid = None;
    let mut xrp_down_ask = None;

    // Split by | to get each asset section
    // First section may have: [timestamp] 📊 BTC: U$...
    // Other sections: ETH: U$...
    for section in line.split('|') {
        let section = section.trim();
        
        // Find asset name by looking for known assets (BTC:, ETH:, SOL:, XRP:)
        let asset = if section.contains("BTC:") {
            "BTC"
        } else if section.contains("ETH:") {
            "ETH"
        } else if section.contains("SOL:") {
            "SOL"
        } else if section.contains("XRP:") {
            "XRP"
        } else {
            continue; // Skip sections without known assets
        };
        
        // Find the colon after the asset name
        if let Some(colon_pos) = section.find(&format!("{}:", asset)) {
            let prices_part = section[colon_pos + asset.len() + 1..].trim();
            
            // Extract U$bid/ask
            if let Some(u_pos) = prices_part.find("U$") {
                let after_u = &prices_part[u_pos + 2..];
                if let Some(slash_pos) = after_u.find('/') {
                    let up_bid_str = after_u[..slash_pos].trim();
                    let after_slash = &after_u[slash_pos + 1..];
                    
                    // Find where D$ starts (or end of string)
                    // Note: up_ask may have $ prefix (format: /$1.00), so we need to handle that
                    let d_pos = after_slash.find(" D$").unwrap_or(after_slash.len());
                    let mut up_ask_str = after_slash[..d_pos].trim();
                    // Remove $ prefix if present (format can be $1.00 or 1.00)
                    if up_ask_str.starts_with('$') {
                        up_ask_str = &up_ask_str[1..];
                    }
                    
                    // Extract D$bid/ask
                    if let Some(d_start) = after_slash.find("D$") {
                        let after_d = &after_slash[d_start + 2..];
                        if let Some(slash_pos2) = after_d.find('/') {
                            let down_bid_str = after_d[..slash_pos2].trim();
                            // Get down_ask (everything after / until space or end)
                            // Note: down_ask may have $ prefix (format: /$0.00), so we need to handle that
                            let down_ask_rest = &after_d[slash_pos2 + 1..];
                            let mut down_ask_str = down_ask_rest.split_whitespace().next().unwrap_or("").trim();
                            // Remove $ prefix if present (format can be $0.00 or 0.00)
                            if down_ask_str.starts_with('$') {
                                down_ask_str = &down_ask_str[1..];
                            }
                            
                            let up_bid = parse_price(up_bid_str);
                            let up_ask = parse_price(up_ask_str);
                            let down_bid = parse_price(down_bid_str);
                            let down_ask = parse_price(down_ask_str);
                            
                            match asset {
                                "BTC" => {
                                    btc_up_bid = up_bid;
                                    btc_up_ask = up_ask;
                                    btc_down_bid = down_bid;
                                    btc_down_ask = down_ask;
                                }
                                "ETH" => {
                                    eth_up_bid = up_bid;
                                    eth_up_ask = up_ask;
                                    eth_down_bid = down_bid;
                                    eth_down_ask = down_ask;
                                }
                                "SOL" => {
                                    solana_up_bid = up_bid;
                                    solana_up_ask = up_ask;
                                    solana_down_bid = down_bid;
                                    solana_down_ask = down_ask;
                                }
                                "XRP" => {
                                    xrp_up_bid = up_bid;
                                    xrp_up_ask = up_ask;
                                    xrp_down_bid = down_bid;
                                    xrp_down_ask = down_ask;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }

    Some(PriceSnapshot {
        timestamp,
        time_remaining_seconds: time_remaining,
        btc_up_bid,
        btc_up_ask,
        btc_down_bid,
        btc_down_ask,
        eth_up_bid,
        eth_up_ask,
        eth_down_bid,
        eth_down_ask,
        solana_up_bid,
        solana_up_ask,
        solana_down_bid,
        solana_down_ask,
        xrp_up_bid,
        xrp_up_ask,
        xrp_down_bid,
        xrp_down_ask,
    })
}

/// Load price history from a file
pub fn load_price_history(file_path: &Path) -> Result<Vec<PriceSnapshot>> {
    let content = fs::read_to_string(file_path)
        .with_context(|| format!("Failed to read history file: {:?}", file_path))?;
    
    let mut snapshots = Vec::new();
    
    for line in content.lines() {
        if let Some(snapshot) = parse_price_line(line) {
            snapshots.push(snapshot);
        }
    }
    
    // Sort by timestamp (oldest first)
    snapshots.sort_by_key(|s| s.timestamp);
    
    Ok(snapshots)
}

/// Determine winner from final prices
/// Winner is the token with ask price > 0.50 at the end
/// Handles resolved state: $1.00/$0.00 means winner is $1.00 token
fn determine_winner(final_snapshot: &PriceSnapshot, asset: &str) -> Option<bool> {
    match asset {
        "BTC" => {
            if let (Some(up_ask), Some(down_ask)) = (final_snapshot.btc_up_ask, final_snapshot.btc_down_ask) {
                // Explicitly handle resolved state ($1.00/$0.00)
                if up_ask >= 1.0 || (up_ask > 0.50 && down_ask <= 0.50) {
                    Some(true) // Up won
                } else if down_ask >= 1.0 || (down_ask > 0.50 && up_ask <= 0.50) {
                    Some(false) // Down won
                } else {
                    // Fallback: whichever is higher
                    if up_ask > down_ask {
                        Some(true)
                    } else if down_ask > up_ask {
                        Some(false)
                    } else {
                        None
                    }
                }
            } else {
                None
            }
        }
        "ETH" => {
            if let (Some(up_ask), Some(down_ask)) = (final_snapshot.eth_up_ask, final_snapshot.eth_down_ask) {
                if up_ask >= 1.0 || (up_ask > 0.50 && down_ask <= 0.50) {
                    Some(true)
                } else if down_ask >= 1.0 || (down_ask > 0.50 && up_ask <= 0.50) {
                    Some(false)
                } else {
                    if up_ask > down_ask {
                        Some(true)
                    } else if down_ask > up_ask {
                        Some(false)
                    } else {
                        None
                    }
                }
            } else {
                None
            }
        }
        "SOL" => {
            if let (Some(up_ask), Some(down_ask)) = (final_snapshot.solana_up_ask, final_snapshot.solana_down_ask) {
                if up_ask >= 1.0 || (up_ask > 0.50 && down_ask <= 0.50) {
                    Some(true)
                } else if down_ask >= 1.0 || (down_ask > 0.50 && up_ask <= 0.50) {
                    Some(false)
                } else {
                    if up_ask > down_ask {
                        Some(true)
                    } else if down_ask > up_ask {
                        Some(false)
                    } else {
                        None
                    }
                }
            } else {
                None
            }
        }
        "XRP" => {
            if let (Some(up_ask), Some(down_ask)) = (final_snapshot.xrp_up_ask, final_snapshot.xrp_down_ask) {
                if up_ask >= 1.0 || (up_ask > 0.50 && down_ask <= 0.50) {
                    Some(true)
                } else if down_ask >= 1.0 || (down_ask > 0.50 && up_ask <= 0.50) {
                    Some(false)
                } else {
                    if up_ask > down_ask {
                        Some(true)
                    } else if down_ask > up_ask {
                        Some(false)
                    } else {
                        None
                    }
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Run backtest for a single period
pub fn backtest_period(
    snapshots: &[PriceSnapshot],
    period_timestamp: u64,
    config: &Config,
    asset: &str,
) -> Result<PeriodResult> {
    let limit_price = config.trading.dual_limit_price.unwrap_or(0.45);
    let shares = config.trading.dual_limit_shares.unwrap_or_else(|| {
        config.trading.fixed_trade_amount / limit_price
    });
    let hedge_after_minutes = config.trading.dual_limit_hedge_after_minutes.unwrap_or(10);
    let hedge_price = config.trading.dual_limit_hedge_price.unwrap_or(0.85);

    let mut positions: Vec<BacktestPosition> = Vec::new();
    let mut up_filled = false;
    let mut down_filled = false;
    let mut up_fill_time: Option<u64> = None;
    let mut down_fill_time: Option<u64> = None;
    let mut hedge_triggered = false;

    // Process snapshots chronologically
    for snapshot in snapshots {
        let time_elapsed_seconds = 900 - snapshot.time_remaining_seconds;
        let time_elapsed_minutes = time_elapsed_seconds / 60;

        // Get current prices for this asset
        let (up_ask, down_ask) = match asset {
            "BTC" => (snapshot.btc_up_ask, snapshot.btc_down_ask),
            "ETH" => (snapshot.eth_up_ask, snapshot.eth_down_ask),
            "SOL" => (snapshot.solana_up_ask, snapshot.solana_down_ask),
            "XRP" => (snapshot.xrp_up_ask, snapshot.xrp_down_ask),
            _ => (None, None),
        };

        // Check limit order fills (ask price <= limit_price)
        if !up_filled {
            if let Some(ask) = up_ask {
                if ask <= limit_price {
                    positions.push(BacktestPosition {
                        token_type: format!("{}_UP", asset),
                        shares,
                        purchase_price: ask,
                        purchase_time_remaining: snapshot.time_remaining_seconds,
                    });
                    up_filled = true;
                    up_fill_time = Some(time_elapsed_seconds);
                }
            }
        }

        if !down_filled {
            if let Some(ask) = down_ask {
                if ask <= limit_price {
                    positions.push(BacktestPosition {
                        token_type: format!("{}_DOWN", asset),
                        shares,
                        purchase_price: ask,
                        purchase_time_remaining: snapshot.time_remaining_seconds,
                    });
                    down_filled = true;
                    down_fill_time = Some(time_elapsed_seconds);
                }
            }
        }

        // Check hedge logic at/after 10 minutes (if only one side filled)
        // This continues checking until either:
        // 1. The unfilled order fills naturally (ask <= 0.45) - nothing to do
        // 2. The unfilled token's ask >= 0.85 - buy at 0.85
        if !hedge_triggered 
            && time_elapsed_minutes >= hedge_after_minutes 
            && (up_filled != down_filled) {
            
            let unfilled_ask = if up_filled {
                down_ask
            } else {
                up_ask
            };

            if let Some(ask) = unfilled_ask {
                // If price is already >= hedge_price, buy immediately at current price
                if ask >= hedge_price {
                    // Cancel unfilled order and buy at current ask price (not hedge_price)
                    let token_type = if up_filled {
                        format!("{}_DOWN", asset)
                    } else {
                        format!("{}_UP", asset)
                    };

                    positions.push(BacktestPosition {
                        token_type,
                        shares,
                        purchase_price: ask, // Buy at current price, not hedge_price
                        purchase_time_remaining: snapshot.time_remaining_seconds,
                    });

                    if up_filled {
                        down_filled = true;
                    } else {
                        up_filled = true;
                    }
                    hedge_triggered = true;
                }
                // If price goes down and fills naturally (ask <= 0.45), the limit order fill logic above will handle it
                // and set up_filled/down_filled to true, which will prevent hedge from triggering
            }
        }
    }

    // Determine winner from final snapshot with valid prices
    // Look for snapshots near the end (time_remaining <= 30s) with valid prices
    // This handles cases where market resolves early (prices show $1.00/$0.00 at 3s, 2s, 1s, 0s)
    let final_snapshot = snapshots.iter()
        .rev()
        .find(|snapshot| {
            // Check if time remaining is low (near end of period) and has valid prices
            let has_valid_prices = match asset {
                "BTC" => snapshot.btc_up_ask.is_some() && snapshot.btc_down_ask.is_some(),
                "ETH" => snapshot.eth_up_ask.is_some() && snapshot.eth_down_ask.is_some(),
                "SOL" => snapshot.solana_up_ask.is_some() && snapshot.solana_down_ask.is_some(),
                "XRP" => snapshot.xrp_up_ask.is_some() && snapshot.xrp_down_ask.is_some(),
                _ => false,
            };
            
            // Prefer snapshots with low time remaining (<= 30s) that have valid prices
            // This captures resolved markets ($1.00/$0.00) that appear near the end
            has_valid_prices && snapshot.time_remaining_seconds <= 30
        })
        .or_else(|| {
            // Fallback: if no snapshot with low time remaining, use any snapshot with valid prices
            snapshots.iter()
                .rev()
                .find(|snapshot| {
                    match asset {
                        "BTC" => snapshot.btc_up_ask.is_some() && snapshot.btc_down_ask.is_some(),
                        "ETH" => snapshot.eth_up_ask.is_some() && snapshot.eth_down_ask.is_some(),
                        "SOL" => snapshot.solana_up_ask.is_some() && snapshot.solana_down_ask.is_some(),
                        "XRP" => snapshot.xrp_up_ask.is_some() && snapshot.xrp_down_ask.is_some(),
                        _ => false,
                    }
                })
        })
        .ok_or_else(|| {
            // Debug: show what snapshots we have
            let sample_prices: Vec<String> = snapshots.iter()
                .rev()
                .take(5)
                .map(|s| {
                    let (up, down) = match asset {
                        "BTC" => (s.btc_up_ask, s.btc_down_ask),
                        "ETH" => (s.eth_up_ask, s.eth_down_ask),
                        "SOL" => (s.solana_up_ask, s.solana_down_ask),
                        "XRP" => (s.xrp_up_ask, s.xrp_down_ask),
                        _ => (None, None),
                    };
                    format!("(t={}s, up={:?}, down={:?})", s.time_remaining_seconds, up, down)
                })
                .collect();
            anyhow::anyhow!("No snapshot with valid prices found for {} in period {}. Sample snapshots: {:?}", 
                asset, period_timestamp, sample_prices)
        })?;
    
    let up_won = determine_winner(final_snapshot, asset)
        .ok_or_else(|| {
            let (up_ask, down_ask) = match asset {
                "BTC" => (final_snapshot.btc_up_ask, final_snapshot.btc_down_ask),
                "ETH" => (final_snapshot.eth_up_ask, final_snapshot.eth_down_ask),
                "SOL" => (final_snapshot.solana_up_ask, final_snapshot.solana_down_ask),
                "XRP" => (final_snapshot.xrp_up_ask, final_snapshot.xrp_down_ask),
                _ => (None, None),
            };
            anyhow::anyhow!("Could not determine winner from final prices for {} (Up ask: {:?}, Down ask: {:?}, time_remaining: {}s)", 
                asset, up_ask, down_ask, final_snapshot.time_remaining_seconds)
        })?;

    // Calculate PnL
    let mut total_cost = 0.0;
    let mut total_value = 0.0;

    for position in &positions {
        total_cost += position.shares * position.purchase_price;
        
        let position_won = match position.token_type.as_str() {
            s if s.ends_with("_UP") => up_won,
            s if s.ends_with("_DOWN") => !up_won,
            _ => false,
        };

        let final_value = if position_won { 1.0 } else { 0.0 };
        total_value += position.shares * final_value;
    }

    let pnl = total_value - total_cost;

    Ok(PeriodResult {
        period_timestamp,
        up_won,
        positions,
        total_cost,
        total_value,
        pnl,
    })
}

/// Run backtest on all history files
pub fn run_backtest(config: &Config) -> Result<BacktestResults> {
    let history_dir = Path::new("history");
    if !history_dir.exists() {
        anyhow::bail!("History directory does not exist");
    }

    // Find all price history files
    let mut history_files: Vec<_> = fs::read_dir(history_dir)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.is_file() && path.file_name()?.to_string_lossy().starts_with("market_") 
                && path.extension()? == "toml" {
                Some(path)
            } else {
                None
            }
        })
        .collect();

    // Sort by filename (which contains timestamp)
    history_files.sort();

    eprintln!("📊 Found {} history files", history_files.len());

    let mut period_results = Vec::new();
    let mut processed_periods = std::collections::HashSet::new();

    // Determine which assets to backtest
    let assets = vec![
        ("BTC", true), // Always enabled
        ("ETH", config.trading.enable_eth_trading),
        ("SOL", config.trading.enable_solana_trading),
        ("XRP", config.trading.enable_xrp_trading),
    ];

    for file_path in &history_files {
        // Extract period timestamp from filename: market_1769549400_prices.toml
        let filename = file_path.file_name().unwrap().to_string_lossy();
        let period_str = filename
            .strip_prefix("market_")
            .and_then(|s| s.strip_suffix("_prices.toml"))
            .ok_or_else(|| anyhow::anyhow!("Invalid filename format: {}", filename))?;
        
        let period_timestamp: u64 = period_str.parse()
            .with_context(|| format!("Failed to parse period timestamp: {}", period_str))?;

        // Skip if we've already processed this period for any asset
        if processed_periods.contains(&period_timestamp) {
            continue;
        }

        // Load price history
        let snapshots = load_price_history(file_path)?;
        if snapshots.is_empty() {
            continue;
        }

        // Backtest each enabled asset
        for (asset, enabled) in &assets {
            if !enabled {
                continue;
            }

            match backtest_period(&snapshots, period_timestamp, config, asset) {
                Ok(result) => {
                    period_results.push(result);
                    processed_periods.insert(period_timestamp);
                }
                Err(e) => {
                    eprintln!("⚠️  Failed to backtest {} period {}: {}", asset, period_timestamp, e);
                }
            }
        }
    }

    // Calculate aggregate results
    let total_periods = period_results.len();
    let total_cost: f64 = period_results.iter().map(|r| r.total_cost).sum();
    let total_value: f64 = period_results.iter().map(|r| r.total_value).sum();
    let total_pnl = total_value - total_cost;
    let winning_periods = period_results.iter().filter(|r| r.pnl > 0.0).count();
    let losing_periods = period_results.iter().filter(|r| r.pnl < 0.0).count();

    Ok(BacktestResults {
        period_results,
        total_periods,
        total_cost,
        total_value,
        total_pnl,
        winning_periods,
        losing_periods,
    })
}

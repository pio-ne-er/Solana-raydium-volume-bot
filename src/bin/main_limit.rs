// Limit order version of the bot
// At min_elapsed_minutes, places limit buy orders for both Up and Down tokens
// When a buy order fills, immediately places limit sell orders

use polymarket_arbitrage_bot::*;
use anyhow::{Context, Result};
use clap::Parser;
use polymarket_arbitrage_bot::config::{Args, Config};
use log::warn;
use std::sync::Arc;
use std::io::{self, Write};
use std::fs::{File, OpenOptions};
use std::sync::{Mutex, OnceLock};
use chrono::Utc;

use polymarket_arbitrage_bot::api::PolymarketApi;
use polymarket_arbitrage_bot::detector::PriceDetector;
use polymarket_arbitrage_bot::monitor::MarketMonitor;
use polymarket_arbitrage_bot::trader::Trader;

/// A writer that writes to both stderr (terminal) and a file
struct DualWriter {
    stderr: io::Stderr,
    file: Mutex<File>,
}

impl Write for DualWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let _ = self.stderr.write_all(buf);
        let _ = self.stderr.flush();
        let mut file = self.file.lock().unwrap();
        file.write_all(buf)?;
        file.flush()?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stderr.flush()?;
        let mut file = self.file.lock().unwrap();
        file.flush()?;
        Ok(())
    }
}

unsafe impl Send for DualWriter {}
unsafe impl Sync for DualWriter {}

static HISTORY_FILE: OnceLock<Mutex<File>> = OnceLock::new();

fn init_history_file(file: File) {
    HISTORY_FILE.set(Mutex::new(file)).expect("History file already initialized");
}

pub fn log_to_history(message: &str) {
    eprint!("{}", message);
    let _ = io::stderr().flush();
    if let Some(file_mutex) = HISTORY_FILE.get() {
        if let Ok(mut file) = file_mutex.lock() {
            let _ = write!(file, "{}", message);
            let _ = file.flush();
        }
    }
}

pub fn log_trading_event(event: &str) {
    let timestamp = Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
    let message = format!("[{}] {}\n", timestamp, event);
    log_to_history(&message);
}

#[macro_export]
macro_rules! log_println {
    ($($arg:tt)*) => {
        {
            let message = format!($($arg)*);
            $crate::log_to_history(&format!("{}\n", message));
        }
    };
}

#[tokio::main]
async fn main() -> Result<()> {
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("history.toml")
        .context("Failed to open history.toml for logging")?;
    
    init_history_file(log_file.try_clone().context("Failed to clone history file")?);
    polymarket_arbitrage_bot::init_history_file(log_file.try_clone().context("Failed to clone history file for lib.rs")?);
    
    let dual_writer = DualWriter {
        stderr: io::stderr(),
        file: Mutex::new(log_file),
    };
    
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .target(env_logger::Target::Pipe(Box::new(dual_writer)))
        .init();

    let args = Args::parse();
    let config = Config::load(&args.config)?;

    eprintln!("🚀 Starting Polymarket Limit Order Trading Bot");
    eprintln!("📝 Logs are being saved to: history.toml");
    let is_simulation = args.is_simulation();
    eprintln!("Mode: {}", if is_simulation { "SIMULATION" } else { "PRODUCTION" });
    eprintln!("Strategy: Limit orders - Buy both Up/Down at min_elapsed_minutes, sell when filled");
    if config.trading.enable_eth_trading {
        eprintln!("✅ Trading enabled for both BTC and ETH 15-minute markets");
    } else {
        eprintln!("✅ Trading enabled for BTC 15-minute markets only (ETH trading disabled)");
    }

    let api = Arc::new(PolymarketApi::new(
        config.polymarket.gamma_api_url.clone(),
        config.polymarket.clob_api_url.clone(),
        config.polymarket.api_key.clone(),
        config.polymarket.api_secret.clone(),
        config.polymarket.api_passphrase.clone(),
        config.polymarket.private_key.clone(),
        config.polymarket.proxy_wallet_address.clone(),
        config.polymarket.signature_type,
    ));

    if !is_simulation {
        eprintln!("\n═══════════════════════════════════════════════════════════");
        eprintln!("🔐 Authenticating with Polymarket CLOB API...");
        eprintln!("═══════════════════════════════════════════════════════════");
        
        match api.authenticate().await {
            Ok(_) => {
                eprintln!("✅ Authentication successful!");
                eprintln!("═══════════════════════════════════════════════════════════");
            }
            Err(e) => {
                warn!("⚠️  Failed to authenticate: {}", e);
                warn!("⚠️  The bot will continue, but order placement may fail");
                eprintln!("");
            }
        }
    } else {
        eprintln!("💡 Simulation mode: Skipping authentication");
        eprintln!("");
    }

    eprintln!("🔍 Discovering BTC, ETH, Solana, and XRP markets...");
    let (eth_market_data, btc_market_data, solana_market_data, xrp_market_data) = 
        get_or_discover_markets(&api, &config).await?;
    
    let monitor = MarketMonitor::new(
        api.clone(),
        eth_market_data,
        btc_market_data,
        solana_market_data,
        xrp_market_data,
        config.trading.check_interval_ms,
        is_simulation,
    )?;
    let monitor_arc = Arc::new(monitor);

    // For limit orders, we ignore max_buy_price and min_time_remaining_seconds
    let detector = PriceDetector::new(
        config.trading.trigger_price,
        1.0, // max_buy_price - ignored for limit orders
        config.trading.min_elapsed_minutes,
        0, // min_time_remaining_seconds - ignored for limit orders
        config.trading.enable_eth_trading,
        config.trading.enable_solana_trading,
    );

    let detector_arc = Arc::new(detector);
    
    let trader = Trader::new(
        api.clone(),
        config.trading.clone(),
        is_simulation,
        Some(detector_arc.clone()),
    )?;
    let trader_arc = Arc::new(trader);
    let trader_clone = trader_arc.clone();
    
    crate::log_println!("🔄 Syncing pending trades with portfolio balance...");
    if let Err(e) = trader_clone.sync_trades_with_portfolio().await {
        warn!("Error syncing trades with portfolio: {}", e);
    }
    
    // Background task to check pending trades and sell points
    let trader_check = trader_clone.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(500));
        let mut summary_interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(e) = trader_check.check_pending_trades().await {
                        warn!("Error checking pending trades: {}", e);
                    }
                }
                _ = summary_interval.tick() => {
                    trader_check.print_trade_summary().await;
                }
            }
        }
    });

    // Background task to check market closure
    let trader_closure = trader_clone.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(
            config.trading.market_closure_check_interval_seconds
        ));
        loop {
            interval.tick().await;
            if let Err(e) = trader_closure.check_market_closure().await {
                warn!("Error checking market closure: {}", e);
            }
        }
    });

    // Background task to detect new 15-minute periods
    let monitor_for_period_check = monitor_arc.clone();
    let api_for_period_check = api.clone();
    let trader_for_period_reset = trader_clone.clone();
    let detector_for_period_reset = detector_arc.clone();
    tokio::spawn(async move {
        loop {
            let current_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            
            let current_period = (current_time / 900) * 900;
            let current_market_timestamp = monitor_for_period_check.get_current_market_timestamp().await;
            
            if current_market_timestamp != current_period && current_market_timestamp != 0 {
                eprintln!("🔄 Market period mismatch detected! Current market: {}, Current period: {}", 
                    current_market_timestamp, current_period);
            } else {
                let next_period_timestamp = current_period + 900;
                let sleep_duration = if next_period_timestamp > current_time {
                    next_period_timestamp - current_time
                } else {
                    0
                };
            
                eprintln!("⏰ Current market period: {}, next period starts in {} seconds", 
                    current_market_timestamp, sleep_duration);
            
                if sleep_duration > 0 && sleep_duration < 1800 {
                    tokio::time::sleep(tokio::time::Duration::from_secs(sleep_duration)).await;
                } else if sleep_duration == 0 {
                    eprintln!("🔄 Next period already started, discovering new market...");
                } else {
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    continue;
                }
            }
            
            let current_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let current_period = (current_time / 900) * 900;
            
            eprintln!("🔄 New 15-minute period detected! (Period: {}) Discovering new markets...", current_period);
            
            let mut seen_ids = std::collections::HashSet::new();
            let (eth_id, btc_id) = monitor_for_period_check.get_current_condition_ids().await;
            seen_ids.insert(eth_id);
            seen_ids.insert(btc_id);
            
            let eth_result = discover_market(&api_for_period_check, "ETH", &["eth"], current_time, &mut seen_ids).await;
            let btc_result = discover_market(&api_for_period_check, "BTC", &["btc"], current_time, &mut seen_ids).await;
            let solana_market = discover_solana_market(&api_for_period_check, current_time, &mut seen_ids).await;
            let xrp_market = discover_xrp_market(&api_for_period_check, current_time, &mut seen_ids).await;
            
            match (eth_result, btc_result) {
                (Ok(eth_market), Ok(btc_market)) => {
                    if let Err(e) = monitor_for_period_check.update_markets(eth_market, btc_market, solana_market, xrp_market).await {
                        warn!("Failed to update markets: {}", e);
                    } else {
                        trader_for_period_reset.reset_period(current_market_timestamp).await;
                        detector_for_period_reset.reset_period().await;
                    }
                }
                (Err(e), _) => warn!("Failed to discover new ETH market: {}", e),
                (_, Err(e)) => warn!("Failed to discover new BTC market: {}", e),
            }
        }
    });
    
    // Start monitoring with limit order strategy
    monitor_arc.start_monitoring(move |snapshot| {
        let detector = detector_arc.clone();
        let trader = trader_clone.clone();
        
        async move {
            // First buy logic: same as market bot – use detect_opportunities (trigger_price, max_buy_price, min_time_remaining, reset, BID).
            // Limit-order strategy: first buy must be Up only; Down is acquired via hedge (limit buy at 1 - stop_loss after Up buy).
            use crate::detector::TokenType;
            let opportunities: Vec<_> = detector.detect_opportunities(&snapshot).await
                .into_iter()
                .filter(|o| matches!(o.token_type, TokenType::BtcUp | TokenType::EthUp | TokenType::SolanaUp))
                .collect();
            if opportunities.is_empty() {
                return;
            }

            if let Some(ref first) = opportunities.first() {
                trader.cleanup_old_abandoned_trades(first.period_timestamp).await;
            }

            for opportunity in opportunities {
                if trader.has_active_position(opportunity.period_timestamp, opportunity.token_type.clone()).await {
                    eprintln!("⏸️  Skip buy ({} position exists in period {})", 
                        match opportunity.token_type {
                            crate::detector::TokenType::BtcUp | crate::detector::TokenType::BtcDown => "BTC",
                            crate::detector::TokenType::EthUp | crate::detector::TokenType::EthDown => "ETH",
                            crate::detector::TokenType::SolanaUp | crate::detector::TokenType::SolanaDown => "Solana",
                            crate::detector::TokenType::XrpUp | crate::detector::TokenType::XrpDown => "XRP",
                        },
                        opportunity.period_timestamp);
                    continue;
                }
                
                if let Err(e) = trader.execute_buy(&opportunity).await {
                    warn!("Error executing buy: {}", e);
                }
            }
        }
    }).await;

    Ok(())
}

// Copy helper functions from main.rs
async fn get_or_discover_markets(
    api: &PolymarketApi,
    _config: &Config,
) -> Result<(crate::models::Market, crate::models::Market, crate::models::Market, crate::models::Market)> {
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    let mut seen_ids = std::collections::HashSet::new();
    
    let eth_market = discover_market(api, "ETH", &["eth"], current_time, &mut seen_ids).await
        .unwrap_or_else(|_| {
            eprintln!("⚠️  Could not discover ETH market - using fallback");
            crate::models::Market {
                condition_id: "dummy_eth_fallback".to_string(),
                slug: "eth-updown-15m-fallback".to_string(),
                active: false,
                closed: true,
                market_id: None,
                question: "ETH Trading Disabled".to_string(),
                resolution_source: None,
                end_date_iso: None,
                end_date_iso_alt: None,
                tokens: None,
                clob_token_ids: None,
                outcomes: None,
            }
        });
    seen_ids.insert(eth_market.condition_id.clone());
    
    eprintln!("🔍 Discovering BTC market...");
    let btc_market = discover_market(api, "BTC", &["btc"], current_time, &mut seen_ids).await
        .context("Failed to discover BTC market")?;
    seen_ids.insert(btc_market.condition_id.clone());

    eprintln!("🔍 Discovering Solana market...");
    let solana_market = discover_solana_market(api, current_time, &mut seen_ids).await;

    eprintln!("🔍 Discovering XRP market...");
    let xrp_market = discover_xrp_market(api, current_time, &mut seen_ids).await;

    if eth_market.condition_id == btc_market.condition_id && eth_market.condition_id != "dummy_eth_fallback" {
        anyhow::bail!("ETH and BTC markets have the same condition ID: {}. This is incorrect.", eth_market.condition_id);
    }
    if solana_market.condition_id != "dummy_solana_fallback" {
        if eth_market.condition_id == solana_market.condition_id && eth_market.condition_id != "dummy_eth_fallback" {
            anyhow::bail!("ETH and Solana markets have the same condition ID: {}. This is incorrect.", eth_market.condition_id);
        }
        if btc_market.condition_id == solana_market.condition_id {
            anyhow::bail!("BTC and Solana markets have the same condition ID: {}. This is incorrect.", btc_market.condition_id);
        }
    }
    if xrp_market.condition_id != "dummy_xrp_fallback" {
        if eth_market.condition_id == xrp_market.condition_id && eth_market.condition_id != "dummy_eth_fallback" {
            anyhow::bail!("ETH and XRP markets have the same condition ID: {}. This is incorrect.", eth_market.condition_id);
        }
        if btc_market.condition_id == xrp_market.condition_id {
            anyhow::bail!("BTC and XRP markets have the same condition ID: {}. This is incorrect.", btc_market.condition_id);
        }
        if solana_market.condition_id == xrp_market.condition_id && solana_market.condition_id != "dummy_solana_fallback" {
            anyhow::bail!("Solana and XRP markets have the same condition ID: {}. This is incorrect.", solana_market.condition_id);
        }
    }

    Ok((eth_market, btc_market, solana_market, xrp_market))
}

async fn discover_solana_market(
    api: &PolymarketApi,
    current_time: u64,
    seen_ids: &mut std::collections::HashSet<String>,
) -> crate::models::Market {
    eprintln!("🔍 Discovering Solana market...");
    if let Ok(market) = discover_market(api, "Solana", &["solana", "sol"], current_time, seen_ids).await {
        return market;
    }
    eprintln!("⚠️  Could not discover Solana 15-minute market. Using fallback - Solana trading disabled.");
    crate::models::Market {
        condition_id: "dummy_solana_fallback".to_string(),
        slug: "solana-updown-15m-fallback".to_string(),
        active: false,
        closed: true,
        market_id: None,
        question: "Solana Trading (market not found)".to_string(),
        resolution_source: None,
        end_date_iso: None,
        end_date_iso_alt: None,
        tokens: None,
        clob_token_ids: None,
        outcomes: None,
    }
}

async fn discover_xrp_market(
    api: &PolymarketApi,
    current_time: u64,
    seen_ids: &mut std::collections::HashSet<String>,
) -> crate::models::Market {
    eprintln!("🔍 Discovering XRP market...");
    if let Ok(market) = discover_market(api, "XRP", &["xrp"], current_time, seen_ids).await {
        return market;
    }
    eprintln!("⚠️  Could not discover XRP 15-minute market. Using fallback - XRP trading disabled.");
    crate::models::Market {
        condition_id: "dummy_xrp_fallback".to_string(),
        slug: "xrp-updown-15m-fallback".to_string(),
        active: false,
        closed: true,
        market_id: None,
        question: "XRP Trading (market not found)".to_string(),
        resolution_source: None,
        end_date_iso: None,
        end_date_iso_alt: None,
        tokens: None,
        clob_token_ids: None,
        outcomes: None,
    }
}

async fn discover_market(
    api: &PolymarketApi,
    market_name: &str,
    slug_prefixes: &[&str],
    current_time: u64,
    seen_ids: &mut std::collections::HashSet<String>,
) -> Result<crate::models::Market> {
    let rounded_time = (current_time / 900) * 900;

    for (i, prefix) in slug_prefixes.iter().enumerate() {
        if i > 0 {
            eprintln!("🔍 Trying {} market with slug prefix '{}'...", market_name, prefix);
        }

        let slug = format!("{}-updown-15m-{}", prefix, rounded_time);
        if let Ok(market) = api.get_market_by_slug(&slug).await {
            if !seen_ids.contains(&market.condition_id) && market.active && !market.closed {
                eprintln!("Found {} market by slug: {} | Condition ID: {}", market_name, market.slug, market.condition_id);
                return Ok(market);
            }
        }
    
        for offset in 1..=3 {
            let try_time = rounded_time - (offset * 900);
            let try_slug = format!("{}-updown-15m-{}", prefix, try_time);
            eprintln!("Trying previous {} market by slug: {}", market_name, try_slug);
            if let Ok(market) = api.get_market_by_slug(&try_slug).await {
                if !seen_ids.contains(&market.condition_id) && market.active && !market.closed {
                    eprintln!("Found {} market by slug: {} | Condition ID: {}", market_name, market.slug, market.condition_id);
                    return Ok(market);
                }
            }
        }
    }

    let tried = slug_prefixes.join(", ");
    anyhow::bail!(
        "Could not find active {} 15-minute up/down market (tried prefixes: {}).",
        market_name,
        tried
    )
}

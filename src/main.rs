// Modules are in lib.rs for reuse
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
/// Wrapped in Arc<Mutex<>> for thread-safe access
struct DualWriter {
    stderr: io::Stderr,
    file: Mutex<File>,
}

impl Write for DualWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // Write to stderr (terminal) - stderr is already thread-safe
        let _ = self.stderr.write_all(buf);
        let _ = self.stderr.flush();
        
        // Write to file (protected by Mutex for thread safety)
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

// Make DualWriter Send + Sync for use with env_logger
unsafe impl Send for DualWriter {}
unsafe impl Sync for DualWriter {}

/// Global file writer for eprintln! messages to be saved to history.toml
static HISTORY_FILE: OnceLock<Mutex<File>> = OnceLock::new();

/// Initialize the global history file writer
fn init_history_file(file: File) {
    HISTORY_FILE.set(Mutex::new(file)).expect("History file already initialized");
}

/// Write a message to both stderr and history.toml (without timestamp/level prefix)
pub fn log_to_history(message: &str) {
    // Write to stderr
    eprint!("{}", message);
    let _ = io::stderr().flush();
    
    // Write to history file
    if let Some(file_mutex) = HISTORY_FILE.get() {
        if let Ok(mut file) = file_mutex.lock() {
            let _ = write!(file, "{}", message);
            let _ = file.flush();
        }
    }
}

/// Log structured trading event to history.toml
/// Only logs essential trading events: buy orders, market results, redemption status
pub fn log_trading_event(event: &str) {
    let timestamp = Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
    let message = format!("[{}] {}\n", timestamp, event);
    log_to_history(&message);
}

/// Macro to log to both stderr and history.toml (like eprintln! but also saves to file)
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
    // Open log file in append mode
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("history.toml")
        .context("Failed to open history.toml for logging")?;
    
    // Initialize global history file for eprintln! messages
    init_history_file(log_file.try_clone().context("Failed to clone history file")?);
    
    // Also initialize the lib.rs history file (so modules can write to it)
    polymarket_arbitrage_bot::init_history_file(log_file.try_clone().context("Failed to clone history file for lib.rs")?);
    
    // Create dual writer
    let dual_writer = DualWriter {
        stderr: io::stderr(),
        file: Mutex::new(log_file),
    };
    
    // Initialize logger with dual writer
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .target(env_logger::Target::Pipe(Box::new(dual_writer)))
        .init();

    let args = Args::parse();
    let config = Config::load(&args.config)?;

    eprintln!("🚀 Starting Polymarket Trend Trading Bot");
    eprintln!("📝 Logs are being saved to: history.toml");
    let is_simulation = args.is_simulation();
    eprintln!("Mode: {}", if is_simulation { "SIMULATION" } else { "PRODUCTION" });
    if config.trading.enable_eth_trading {
        eprintln!("✅ Trading enabled for both BTC and ETH 15-minute markets");
    } else {
        eprintln!("✅ Trading enabled for BTC 15-minute markets only (ETH trading disabled)");
    }

    // Initialize API client
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

    // Authenticate with Polymarket CLOB API at startup
    // This verifies credentials and creates an authenticated client
    // Equivalent to JavaScript: new ClobClient(HOST, CHAIN_ID, signer, apiCreds)
    if !is_simulation {
        eprintln!("");
        eprintln!("═══════════════════════════════════════════════════════════");
        eprintln!("🔐 Authenticating with Polymarket CLOB API...");
        eprintln!("═══════════════════════════════════════════════════════════");
        
        match api.authenticate().await {
            Ok(_) => {
                eprintln!("✅ Authentication successful!");
                eprintln!("   Using private key and API credentials for signing");
                eprintln!("═══════════════════════════════════════════════════════════");
            }
            Err(e) => {
                warn!("⚠️  Failed to authenticate: {}", e);
                warn!("⚠️  The bot will continue, but order placement may fail");
                warn!("⚠️  Please verify your credentials:");
                warn!("     1. private_key (hex string)");
                warn!("     2. api_key, api_secret, api_passphrase");
                eprintln!("");
            }
        }
    } else {
        eprintln!("💡 Simulation mode: Skipping authentication");
        eprintln!("");
    }

    // Get market data for BTC, ETH, and Solana markets
    eprintln!("🔍 Discovering BTC, ETH, Solana, and XRP markets...");
    let (eth_market_data, btc_market_data, solana_market_data, xrp_market_data) = 
        get_or_discover_markets(&api, &config).await?;
    
    // DISABLED: Pre-approve all conditional tokens at startup using setApprovalForAll
    // Temporarily disabled - approval functions are disabled throughout the codebase
    // if !is_simulation {
    //     eprintln!("");
    //     eprintln!("═══════════════════════════════════════════════════════════");
    //     eprintln!("🔐 Pre-approving all conditional tokens for trading...");
    //     eprintln!("═══════════════════════════════════════════════════════════");
    //     match api.set_approval_for_all_clob().await {
    //         Ok(_) => {
    //             eprintln!("✅ Successfully approved all tokens via setApprovalForAll()");
    //             eprintln!("   Transaction confirmed - this will prevent allowance errors when selling tokens");
    //         }
    //         Err(e) => {
    //             warn!("⚠️  Failed to pre-approve tokens via setApprovalForAll: {}", e);
    //             warn!("   Attempting fallback: approving individual tokens...");
    //             
    //             // Fallback: Approve individual tokens (ETH Up/Down, BTC Up/Down) with large allowance
    //             // This triggers SDK auto-approval by placing tiny test sell orders
    //             match api.approve_individual_tokens(&eth_market_data, &btc_market_data).await {
    //                 Ok(_) => {
    //                     eprintln!("✅ Successfully approved individual tokens with large allowance");
    //                     eprintln!("   This will prevent allowance errors when selling tokens");
    //                 }
    //                 Err(fallback_err) => {
    //                     warn!("⚠️  Failed to approve individual tokens: {}", fallback_err);
    //                     warn!("   The bot will continue, but you may encounter allowance errors when selling");
    //                     warn!("   The bot will attempt to approve tokens automatically when needed");
    //                 }
    //             }
    //         }
    //     }
    //     eprintln!("═══════════════════════════════════════════════════════════");
    //     eprintln!("");
    // }

    // Initialize components
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

    let max_buy_price = config.trading.max_buy_price.unwrap_or(0.95);
    let min_time_remaining = config.trading.min_time_remaining_seconds.unwrap_or(30);
    let detector = PriceDetector::new(
        config.trading.trigger_price,
        max_buy_price,
        config.trading.min_elapsed_minutes,
        min_time_remaining,
        config.trading.enable_eth_trading,
        config.trading.enable_solana_trading,
    );

    // Start monitoring
    let detector_arc = Arc::new(detector);
    let detector_clone = detector_arc.clone();
    
    let trader = Trader::new(
        api.clone(),
        config.trading.clone(),
        is_simulation,
        Some(detector_arc.clone()),
    )?;
    let trader_arc = Arc::new(trader);
    let trader_clone = trader_arc.clone();
    
    // Sync pending trades with portfolio on startup (check if tokens were already redeemed)
    crate::log_println!("🔄 Syncing pending trades with portfolio balance...");
    if let Err(e) = trader_clone.sync_trades_with_portfolio().await {
        warn!("Error syncing trades with portfolio: {}", e);
    }
    
    // Start a background task to check pending trades and sell points
    let trader_check = trader_clone.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(500)); // Check every 500ms (0.5s) for sell retries
        let mut summary_interval = tokio::time::interval(tokio::time::Duration::from_secs(30)); // Print summary every 30 seconds
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

    // Start a background task to check market closure
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

    // Start a background task to detect new 15-minute periods and discover new markets
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
            
            // Check if we need to discover a new market (current market is from a different period)
            if current_market_timestamp != current_period && current_market_timestamp != 0 {
                eprintln!("🔄 Market period mismatch detected! Current market: {}, Current period: {}", 
                    current_market_timestamp, current_period);
                // Fall through to discover new market immediately
            } else {
                // Calculate when next period starts
                let next_period_timestamp = current_period + 900;
            let sleep_duration = if next_period_timestamp > current_time {
                next_period_timestamp - current_time
            } else {
                    0 // Next period already started
            };
            
            eprintln!("⏰ Current market period: {}, next period starts in {} seconds", 
                current_market_timestamp, sleep_duration);
            
                // Only sleep if we have a reasonable duration (avoid infinite loops)
                if sleep_duration > 0 && sleep_duration < 1800 {
            tokio::time::sleep(tokio::time::Duration::from_secs(sleep_duration)).await;
                } else if sleep_duration == 0 {
                    // Next period already started, discover new market immediately
                    eprintln!("🔄 Next period already started, discovering new market...");
                } else {
                    // If calculation is wrong, wait a bit and recalculate
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    continue;
                }
            }
            
            // Recalculate current time and period after sleep
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
            
            // Discover ETH, BTC, Solana, and XRP for the new period (Solana/XRP may return fallback)
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
    
    // Start monitoring with detector (BTC, ETH, and optionally Solana trading enabled)
    monitor_arc.start_monitoring(move |snapshot| {
        let detector = detector_clone.clone();
        let trader = trader_clone.clone();
        
        async move {
            // Detect all opportunities (BTC Up/Down, ETH Up/Down) so we can buy both ETH Down and BTC Down when both qualify
            let opportunities = detector.detect_opportunities(&snapshot).await;
            if opportunities.is_empty() {
                return;
            }

            // Clean up old abandoned trades from previous periods (once per callback)
            if let Some(ref first) = opportunities.first() {
                trader.cleanup_old_abandoned_trades(first.period_timestamp).await;
            }

            for opportunity in opportunities {
                // Only allow one position per token type (BTC or ETH) per market period
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

async fn get_or_discover_markets(
    api: &PolymarketApi,
    _config: &Config,
) -> Result<(crate::models::Market, crate::models::Market, crate::models::Market, crate::models::Market)> {
    
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    // Try multiple discovery methods - use a set to track seen IDs
    let mut seen_ids = std::collections::HashSet::new();
    
    // Discover ETH, BTC, and Solana markets (each can try multiple slug prefixes, e.g. Solana: ["solana","sol"])
    let eth_market = discover_market(api, "ETH", &["eth"], current_time, &mut seen_ids).await
        .unwrap_or_else(|_| {
            // If ETH market discovery fails, create a minimal market struct as fallback
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

    // Discover Solana market
    eprintln!("🔍 Discovering Solana market...");
    let solana_market = discover_solana_market(api, current_time, &mut seen_ids).await;

    // Discover XRP market
    eprintln!("🔍 Discovering XRP market...");
    let xrp_market = discover_xrp_market(api, current_time, &mut seen_ids).await;

    if eth_market.condition_id == btc_market.condition_id && eth_market.condition_id != "dummy_eth_fallback" {
        anyhow::bail!("ETH and BTC markets have the same condition ID: {}. This is incorrect. Please set condition IDs manually in config.json", eth_market.condition_id);
    }
    if solana_market.condition_id != "dummy_solana_fallback" {
        if eth_market.condition_id == solana_market.condition_id && eth_market.condition_id != "dummy_eth_fallback" {
            anyhow::bail!("ETH and Solana markets have the same condition ID: {}. This is incorrect. Please set condition IDs manually in config.json", eth_market.condition_id);
        }
        if btc_market.condition_id == solana_market.condition_id {
            anyhow::bail!("BTC and Solana markets have the same condition ID: {}. This is incorrect. Please set condition IDs manually in config.json", btc_market.condition_id);
        }
    }
    if xrp_market.condition_id != "dummy_xrp_fallback" {
        if eth_market.condition_id == xrp_market.condition_id && eth_market.condition_id != "dummy_eth_fallback" {
            anyhow::bail!("ETH and XRP markets have the same condition ID: {}. This is incorrect. Please set condition IDs manually in config.json", eth_market.condition_id);
        }
        if btc_market.condition_id == xrp_market.condition_id {
            anyhow::bail!("BTC and XRP markets have the same condition ID: {}. This is incorrect. Please set condition IDs manually in config.json", btc_market.condition_id);
        }
        if solana_market.condition_id == xrp_market.condition_id && solana_market.condition_id != "dummy_solana_fallback" {
            anyhow::bail!("Solana and XRP markets have the same condition ID: {}. This is incorrect. Please set condition IDs manually in config.json", solana_market.condition_id);
        }
    }

    Ok((eth_market, btc_market, solana_market, xrp_market))
}

/// Discover Solana 15m market. Tries slug prefixes ["solana", "sol"] via discover_market.
/// Returns a dummy fallback if not found so the bot can run without Solana.
async fn discover_solana_market(
    api: &PolymarketApi,
    current_time: u64,
    seen_ids: &mut std::collections::HashSet<String>,
) -> crate::models::Market {
    eprintln!("🔍 Discovering Solana market...");
    if let Ok(market) = discover_market(api, "Solana", &["solana", "sol"], current_time, seen_ids).await {
        return market;
    }
    eprintln!("⚠️  Could not discover Solana 15-minute market (tried: solana, sol). Using fallback - Solana trading disabled for this run.");
    eprintln!("   To enable Solana: set solana_condition_id in config.json, or ensure Polymarket has an active solana/sol 15m up/down market.");
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

/// Discover XRP 15m market. Tries slug prefix ["xrp"] via discover_market.
/// Returns a dummy fallback if not found so the bot can run without XRP.
async fn discover_xrp_market(
    api: &PolymarketApi,
    current_time: u64,
    seen_ids: &mut std::collections::HashSet<String>,
) -> crate::models::Market {
    eprintln!("🔍 Discovering XRP market...");
    if let Ok(market) = discover_market(api, "XRP", &["xrp"], current_time, seen_ids).await {
        return market;
    }
    eprintln!("⚠️  Could not discover XRP 15-minute market (tried: xrp). Using fallback - XRP trading disabled for this run.");
    eprintln!("   To enable XRP: set xrp_condition_id in config.json, or ensure Polymarket has an active xrp 15m up/down market.");
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

/// Discover a 15-minute up/down market by trying each slug prefix in order.
/// For each prefix: try current period, then previous 3 periods.
/// Pattern: {prefix}-updown-15m-{timestamp} (e.g. btc-updown-15m-1769116500, sol-updown-15m-1769116500).
async fn discover_market(
    api: &PolymarketApi,
    market_name: &str,
    slug_prefixes: &[&str],
    current_time: u64,
    seen_ids: &mut std::collections::HashSet<String>,
) -> Result<crate::models::Market> {
    let rounded_time = (current_time / 900) * 900; // Round to nearest 15 minutes

    for (i, prefix) in slug_prefixes.iter().enumerate() {
        if i > 0 {
            eprintln!("🔍 Trying {} market with slug prefix '{}'...", market_name, prefix);
        }

        // Try current period with this prefix
        let slug = format!("{}-updown-15m-{}", prefix, rounded_time);
    if let Ok(market) = api.get_market_by_slug(&slug).await {
        if !seen_ids.contains(&market.condition_id) && market.active && !market.closed {
            eprintln!("Found {} market by slug: {} | Condition ID: {}", market_name, market.slug, market.condition_id);
            return Ok(market);
        }
    }
    
        // Try previous periods with this prefix
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
        "Could not find active {} 15-minute up/down market (tried prefixes: {}). Set condition_id in config.json if needed.",
        market_name,
        tried
    )
}


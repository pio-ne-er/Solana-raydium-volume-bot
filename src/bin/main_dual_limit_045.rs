// Limit order version: place Up/Down limit buys at market start with fixed price

use polymarket_arbitrage_bot::*;
use anyhow::{Context, Result};
use clap::Parser;
use polymarket_arbitrage_bot::config::{Args, Config};
use log::{warn, debug};
use std::sync::Arc;
use std::io::{self, Write};
use std::fs::{File, OpenOptions};
use std::sync::{Mutex, OnceLock};
use chrono::Utc;

use polymarket_arbitrage_bot::api::PolymarketApi;
use polymarket_arbitrage_bot::monitor::MarketMonitor;
use polymarket_arbitrage_bot::detector::BuyOpportunity;
use polymarket_arbitrage_bot::trader::Trader;

const LIMIT_PRICE: f64 = 0.45;
const PERIOD_DURATION: u64 = 900;
const DEFAULT_HEDGE_AFTER_MINUTES: u64 = 10;
const DEFAULT_HEDGE_PRICE: f64 = 0.85;

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

    eprintln!("🚀 Starting Polymarket Dual Limit-Start Bot");
    eprintln!("📝 Logs are being saved to: history.toml");
    let is_simulation = args.is_simulation();
    eprintln!("Mode: {}", if is_simulation { "SIMULATION" } else { "PRODUCTION" });
    let limit_price = config.trading.dual_limit_price.unwrap_or(LIMIT_PRICE);
    let limit_shares = config.trading.dual_limit_shares;
    let hedge_after_minutes = config
        .trading
        .dual_limit_hedge_after_minutes
        .unwrap_or(DEFAULT_HEDGE_AFTER_MINUTES);
    let early_hedge_after_minutes = config
        .trading
        .dual_limit_early_hedge_minutes
        .unwrap_or(5); // Default to 5 minutes
    let hedge_price = config
        .trading
        .dual_limit_hedge_price
        .unwrap_or(DEFAULT_HEDGE_PRICE);
    eprintln!(
        "Strategy: At market start, place limit buys for BTC, ETH, SOL, and XRP Up/Down at ${:.2}",
        limit_price
    );
    eprintln!(
        "Hedge rule: If only one side fills, after {} minutes start monitoring the other side; if BUY price >= ${:.2}, cancel the unfilled ${:.2} order and buy at ${:.2} for the same shares",
        hedge_after_minutes,
        hedge_price,
        limit_price,
        hedge_price
    );
    if let Some(shares) = limit_shares {
        eprintln!("Shares per order (config): {:.6}", shares);
    } else {
        eprintln!("Shares per order: fixed_trade_amount / price");
    }
    eprintln!(
        "✅ Trading enabled for BTC and {} 15-minute markets",
        enabled_markets_label(
            config.trading.enable_eth_trading,
            config.trading.enable_solana_trading,
            config.trading.enable_xrp_trading
        )
    );

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
        get_or_discover_markets(
            &api,
            config.trading.enable_eth_trading,
            config.trading.enable_solana_trading,
            config.trading.enable_xrp_trading,
        ).await?;

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

    let trader = Trader::new(
        api.clone(),
        config.trading.clone(),
        is_simulation,
        None,
    )?;
    let trader_arc = Arc::new(trader);
    let trader_clone = trader_arc.clone();

    crate::log_println!("🔄 Syncing pending trades with portfolio balance...");
    if let Err(e) = trader_clone.sync_trades_with_portfolio().await {
        warn!("Error syncing trades with portfolio: {}", e);
    }
    
    // Start a background task to check pending trades and limit order fills (for simulation mode)
    let trader_check = trader_clone.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(1000)); // Check every 1s for limit order fills
        let mut summary_interval = tokio::time::interval(tokio::time::Duration::from_secs(30)); // Print summary every 30 seconds
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(e) = trader_check.check_pending_trades().await {
                        warn!("Error checking pending trades: {}", e);
                    }
                }
                _ = summary_interval.tick() => {
                    // trader_check.print_trade_summary().await; // Temporarily disabled
                }
            }
        }
    });

    // Background task to detect new 15-minute periods
    let monitor_for_period_check = monitor_arc.clone();
    let api_for_period_check = api.clone();
    let trader_for_period_reset = trader_clone.clone();
    let enable_eth = config.trading.enable_eth_trading;
    let enable_solana = config.trading.enable_solana_trading;
    let enable_xrp = config.trading.enable_xrp_trading;
    let simulation_tracker_for_market_start = if is_simulation {
        trader_clone.get_simulation_tracker()
    } else {
        None
    };
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

            let eth_result = if enable_eth {
                discover_market(&api_for_period_check, "ETH", &["eth"], current_time, &mut seen_ids, true).await
            } else {
                Ok(disabled_eth_market())
            };
            let btc_result = discover_market(&api_for_period_check, "BTC", &["btc"], current_time, &mut seen_ids, true).await;
            let solana_market = if enable_solana {
                discover_solana_market(&api_for_period_check, current_time, &mut seen_ids).await
            } else {
                disabled_solana_market()
            };
            let xrp_market = if enable_xrp {
                discover_xrp_market(&api_for_period_check, current_time, &mut seen_ids).await
            } else {
                disabled_xrp_market()
            };

            match (eth_result, btc_result) {
                (Ok(eth_market), Ok(btc_market)) => {
                    if let Err(e) = monitor_for_period_check.update_markets(eth_market.clone(), btc_market.clone(), solana_market.clone(), xrp_market.clone()).await {
                        warn!("Failed to update markets: {}", e);
                    } else {
                        // Log market start in simulation mode
                        if let Some(tracker) = &simulation_tracker_for_market_start {
                            let period = (current_time / 900) * 900;
                            tracker.log_market_start(
                                period,
                                &eth_market.condition_id,
                                &btc_market.condition_id,
                                &solana_market.condition_id,
                                &xrp_market.condition_id
                            ).await;
                        }
                        trader_for_period_reset.reset_period(current_market_timestamp).await;
                    }
                }
                (Err(e), _) => warn!("Failed to discover new ETH market: {}", e),
                (_, Err(e)) => warn!("Failed to discover new BTC market: {}", e),
            }
        }
    });

    let last_placed_period = Arc::new(tokio::sync::Mutex::new(None::<u64>));
    let last_seen_period = Arc::new(tokio::sync::Mutex::new(None::<u64>));
    let enable_eth = config.trading.enable_eth_trading;
    let enable_solana = config.trading.enable_solana_trading;
    let enable_xrp = config.trading.enable_xrp_trading;
    let hedge_after_seconds = hedge_after_minutes * 60;
    let early_hedge_after_seconds = early_hedge_after_minutes * 60;
    let hedge_price = hedge_price;
    let is_simulation = is_simulation;
    let config_for_trends = config.clone();
    // Track previous prices for threshold crossing detection
    let previous_prices: Arc<tokio::sync::Mutex<std::collections::HashMap<String, f64>>> = 
        Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
    // Track if we've already executed early hedge for this period
    let early_hedge_executed: Arc<tokio::sync::Mutex<std::collections::HashSet<u64>>> = 
        Arc::new(tokio::sync::Mutex::new(std::collections::HashSet::new()));

    monitor_arc.start_monitoring(move |snapshot| {
        let trader = trader_clone.clone();
        let last_placed_period = last_placed_period.clone();
        let last_seen_period = last_seen_period.clone();
        let enable_eth = enable_eth;
        let enable_solana = enable_solana;
        let enable_xrp = enable_xrp;
        let hedge_after_seconds = hedge_after_seconds;
        let early_hedge_after_seconds = early_hedge_after_seconds;
        let hedge_price = hedge_price;
        let is_simulation = is_simulation;
        let config = config_for_trends.clone();
        let fixed_trade_amount = config.trading.fixed_trade_amount;
        let previous_prices = previous_prices.clone();
        let early_hedge_executed = early_hedge_executed.clone();

        async move {
            if snapshot.time_remaining_seconds == 0 {
                return;
            }

            // Skip the current market if the bot starts after it has already begun.
            {
                let mut seen = last_seen_period.lock().await;
                if seen.is_none() {
                    *seen = Some(snapshot.period_timestamp);
                    return;
                }
                if *seen != Some(snapshot.period_timestamp) {
                    *seen = Some(snapshot.period_timestamp);
                }
            }

            let time_elapsed_seconds = PERIOD_DURATION - snapshot.time_remaining_seconds;
            // Market-start placement (first ~2 seconds)
            let mut opportunities: Vec<BuyOpportunity> = Vec::new();
            if time_elapsed_seconds <= 2 {
            {
                let mut last = last_placed_period.lock().await;
                if last.map(|p| p == snapshot.period_timestamp).unwrap_or(false) {
                        // already placed for this period
                    } else {
                *last = Some(snapshot.period_timestamp);

            if let Some(btc_up) = snapshot.btc_market.up_token.as_ref() {
                opportunities.push(BuyOpportunity {
                    condition_id: snapshot.btc_market.condition_id.clone(),
                    token_id: btc_up.token_id.clone(),
                    token_type: crate::detector::TokenType::BtcUp,
                    bid_price: limit_price,
                    period_timestamp: snapshot.period_timestamp,
                    time_remaining_seconds: snapshot.time_remaining_seconds,
                    time_elapsed_seconds,
                    use_market_order: false,
                                investment_amount_override: None,
                                is_individual_hedge: false,
                                is_standard_hedge: false,
                                dual_limit_shares: None,
                });
            }
            if let Some(btc_down) = snapshot.btc_market.down_token.as_ref() {
                opportunities.push(BuyOpportunity {
                    condition_id: snapshot.btc_market.condition_id.clone(),
                    token_id: btc_down.token_id.clone(),
                    token_type: crate::detector::TokenType::BtcDown,
                    bid_price: limit_price,
                    period_timestamp: snapshot.period_timestamp,
                    time_remaining_seconds: snapshot.time_remaining_seconds,
                    time_elapsed_seconds,
                    use_market_order: false,
                                investment_amount_override: None,
                                is_individual_hedge: false,
                                is_standard_hedge: false,
                                dual_limit_shares: None,
                });
            }

            if enable_eth {
                if let Some(eth_up) = snapshot.eth_market.up_token.as_ref() {
                    opportunities.push(BuyOpportunity {
                        condition_id: snapshot.eth_market.condition_id.clone(),
                        token_id: eth_up.token_id.clone(),
                        token_type: crate::detector::TokenType::EthUp,
                        bid_price: limit_price,
                        period_timestamp: snapshot.period_timestamp,
                        time_remaining_seconds: snapshot.time_remaining_seconds,
                        time_elapsed_seconds,
                        use_market_order: false,
                                    investment_amount_override: None,
                                    is_individual_hedge: false,
                                    is_standard_hedge: false,
                                    dual_limit_shares: None,
                    });
                }
                if let Some(eth_down) = snapshot.eth_market.down_token.as_ref() {
                    opportunities.push(BuyOpportunity {
                        condition_id: snapshot.eth_market.condition_id.clone(),
                        token_id: eth_down.token_id.clone(),
                        token_type: crate::detector::TokenType::EthDown,
                        bid_price: limit_price,
                        period_timestamp: snapshot.period_timestamp,
                        time_remaining_seconds: snapshot.time_remaining_seconds,
                        time_elapsed_seconds,
                        use_market_order: false,
                                    investment_amount_override: None,
                                    is_individual_hedge: false,
                                    is_standard_hedge: false,
                                    dual_limit_shares: None,
                    });
                }
            }
            if enable_solana {
                if let Some(solana_up) = snapshot.solana_market.up_token.as_ref() {
                    opportunities.push(BuyOpportunity {
                        condition_id: snapshot.solana_market.condition_id.clone(),
                        token_id: solana_up.token_id.clone(),
                        token_type: crate::detector::TokenType::SolanaUp,
                        bid_price: limit_price,
                        period_timestamp: snapshot.period_timestamp,
                        time_remaining_seconds: snapshot.time_remaining_seconds,
                        time_elapsed_seconds,
                        use_market_order: false,
                                    investment_amount_override: None,
                                    is_individual_hedge: false,
                                    is_standard_hedge: false,
                                    dual_limit_shares: None,
                    });
                }
                if let Some(solana_down) = snapshot.solana_market.down_token.as_ref() {
                    opportunities.push(BuyOpportunity {
                        condition_id: snapshot.solana_market.condition_id.clone(),
                        token_id: solana_down.token_id.clone(),
                        token_type: crate::detector::TokenType::SolanaDown,
                        bid_price: limit_price,
                        period_timestamp: snapshot.period_timestamp,
                        time_remaining_seconds: snapshot.time_remaining_seconds,
                        time_elapsed_seconds,
                        use_market_order: false,
                                    investment_amount_override: None,
                                    is_individual_hedge: false,
                                    is_standard_hedge: false,
                                    dual_limit_shares: None,
                    });
                }
            }

            if enable_xrp {
                if let Some(xrp_up) = snapshot.xrp_market.up_token.as_ref() {
                    opportunities.push(BuyOpportunity {
                        condition_id: snapshot.xrp_market.condition_id.clone(),
                        token_id: xrp_up.token_id.clone(),
                        token_type: crate::detector::TokenType::XrpUp,
                        bid_price: limit_price,
                        period_timestamp: snapshot.period_timestamp,
                        time_remaining_seconds: snapshot.time_remaining_seconds,
                        time_elapsed_seconds,
                        use_market_order: false,
                                    investment_amount_override: None,
                                    is_individual_hedge: false,
                                    is_standard_hedge: false,
                                    dual_limit_shares: None,
                    });
                }
                if let Some(xrp_down) = snapshot.xrp_market.down_token.as_ref() {
                    opportunities.push(BuyOpportunity {
                        condition_id: snapshot.xrp_market.condition_id.clone(),
                        token_id: xrp_down.token_id.clone(),
                        token_type: crate::detector::TokenType::XrpDown,
                        bid_price: limit_price,
                        period_timestamp: snapshot.period_timestamp,
                        time_remaining_seconds: snapshot.time_remaining_seconds,
                        time_elapsed_seconds,
                        use_market_order: false,
                                    investment_amount_override: None,
                                    is_individual_hedge: false,
                                    is_standard_hedge: false,
                                    dual_limit_shares: None,
                    });
                            }
                        }
                    }
                }
            }

            if opportunities.is_empty() {
                // continue - may still need to hedge later in the period
            } else {
                crate::log_println!("🎯 Market start detected - placing limit buys at ${:.2}", limit_price);
                
                // Check all positions in parallel first
                let position_check_handles: Vec<_> = opportunities.iter()
                    .map(|opp| {
                        let trader_clone = trader.clone();
                        let opp_clone = opp.clone();
                        let period = opp.period_timestamp;
                        let token_type = opp.token_type.clone();
                        tokio::spawn(async move {
                            let has_position = trader_clone.has_active_position(period, token_type).await;
                            (opp_clone, !has_position)
                        })
                    })
                    .collect();
                
                // Wait for all position checks to complete
                let mut checked_opportunities = Vec::new();
                for handle in position_check_handles {
                    if let Ok((opp, should_place)) = handle.await {
                        if should_place {
                            checked_opportunities.push(opp);
                        }
                    }
                }
                
                // Place all orders in parallel
                if !checked_opportunities.is_empty() {
                    crate::log_println!("📤 Placing {} limit buy orders in parallel...", checked_opportunities.len());
                    let order_handles: Vec<_> = checked_opportunities.into_iter()
                        .map(|opp| {
                            let trader_clone = trader.clone();
                            let limit_shares_clone = limit_shares;
                            tokio::spawn(async move {
                                trader_clone.execute_limit_buy(&opp, false, limit_shares_clone).await
                            })
                        })
                        .collect();
                    
                    // Execute all orders concurrently and wait for completion
                    let mut error_count = 0;
                    for (i, handle) in order_handles.into_iter().enumerate() {
                        match handle.await {
                            Ok(Ok(_)) => {
                                // Success
                            }
                            Ok(Err(e)) => {
                                warn!("Error executing limit buy #{}: {}", i + 1, e);
                                error_count += 1;
                            }
                            Err(e) => {
                                warn!("Error awaiting limit buy task #{}: {}", i + 1, e);
                                error_count += 1;
                            }
                        }
                    }
                    
                    if error_count == 0 {
                        crate::log_println!("✅ Successfully placed all limit buy orders");
                    } else {
                        crate::log_println!("⚠️  Placed orders with {} errors", error_count);
                    }
                }
            }

            // Price tracking and trend analysis logging (simulation mode only)
            if is_simulation {
                let simulation_tracker_opt = trader.get_simulation_tracker();
                if let Some(simulation_tracker) = simulation_tracker_opt {
                    let trend_history_size = config.trading.dual_limit_trend_history_size.unwrap_or(60);
                    let min_samples = 10; // Increased minimum samples for longer trend analysis
                    
                    // Track prices for all markets
                    // BTC Market
                    if let Some(btc_up) = snapshot.btc_market.up_token.as_ref() {
                        if let Some(bid) = btc_up.bid {
                            let bid_f64 = f64::try_from(bid).unwrap_or(0.0);
                            if bid_f64 > 0.0 {
                                simulation_tracker.track_price(
                                    snapshot.period_timestamp,
                                    &btc_up.token_id,
                                    time_elapsed_seconds,
                                    bid_f64,
                                    trend_history_size,
                                ).await;
                            }
                        } else {
                            debug!("BTC Up token has no bid price");
                        }
                    } else {
                        debug!("BTC Up token not available");
                    }
                    if let Some(btc_down) = snapshot.btc_market.down_token.as_ref() {
                        if let Some(bid) = btc_down.bid {
                            let bid_f64 = f64::try_from(bid).unwrap_or(0.0);
                            if bid_f64 > 0.0 {
                                simulation_tracker.track_price(
                                    snapshot.period_timestamp,
                                    &btc_down.token_id,
                                    time_elapsed_seconds,
                                    bid_f64,
                                    trend_history_size,
                                ).await;
                            }
                        } else {
                            debug!("BTC Down token has no bid price");
                        }
                    } else {
                        debug!("BTC Down token not available");
                    }
                    
                    // ETH Market
                    if enable_eth {
                        if let Some(eth_up) = snapshot.eth_market.up_token.as_ref() {
                            if let Some(bid) = eth_up.bid {
                                let bid_f64 = f64::try_from(bid).unwrap_or(0.0);
                                simulation_tracker.track_price(
                                    snapshot.period_timestamp,
                                    &eth_up.token_id,
                                    time_elapsed_seconds,
                                    bid_f64,
                                    trend_history_size,
                                ).await;
                            }
                        }
                        if let Some(eth_down) = snapshot.eth_market.down_token.as_ref() {
                            if let Some(bid) = eth_down.bid {
                                let bid_f64 = f64::try_from(bid).unwrap_or(0.0);
                                simulation_tracker.track_price(
                                    snapshot.period_timestamp,
                                    &eth_down.token_id,
                                    time_elapsed_seconds,
                                    bid_f64,
                                    trend_history_size,
                                ).await;
                            }
                        }
                    }
                    
                    // SOL Market
                    if enable_solana {
                        if let Some(solana_up) = snapshot.solana_market.up_token.as_ref() {
                            if let Some(bid) = solana_up.bid {
                                let bid_f64 = f64::try_from(bid).unwrap_or(0.0);
                                simulation_tracker.track_price(
                                    snapshot.period_timestamp,
                                    &solana_up.token_id,
                                    time_elapsed_seconds,
                                    bid_f64,
                                    trend_history_size,
                                ).await;
                            }
                        }
                        if let Some(solana_down) = snapshot.solana_market.down_token.as_ref() {
                            if let Some(bid) = solana_down.bid {
                                let bid_f64 = f64::try_from(bid).unwrap_or(0.0);
                                simulation_tracker.track_price(
                                    snapshot.period_timestamp,
                                    &solana_down.token_id,
                                    time_elapsed_seconds,
                                    bid_f64,
                                    trend_history_size,
                                ).await;
                            }
                        }
                    }
                    
                    // XRP Market
                    if enable_xrp {
                        if let Some(xrp_up) = snapshot.xrp_market.up_token.as_ref() {
                            if let Some(bid) = xrp_up.bid {
                                let bid_f64 = f64::try_from(bid).unwrap_or(0.0);
                                simulation_tracker.track_price(
                                    snapshot.period_timestamp,
                                    &xrp_up.token_id,
                                    time_elapsed_seconds,
                                    bid_f64,
                                    trend_history_size,
                                ).await;
                            }
                        }
                        if let Some(xrp_down) = snapshot.xrp_market.down_token.as_ref() {
                            if let Some(bid) = xrp_down.bid {
                                let bid_f64 = f64::try_from(bid).unwrap_or(0.0);
                                simulation_tracker.track_price(
                                    snapshot.period_timestamp,
                                    &xrp_down.token_id,
                                    time_elapsed_seconds,
                                    bid_f64,
                                    trend_history_size,
                                ).await;
                            }
                        }
                    }
                    
                    // Log trend analysis periodically (every 30 seconds or at key intervals)
                    let should_log_trends = time_elapsed_seconds % 30 == 0 || 
                                          time_elapsed_seconds == 300 ||  // 5 minutes
                                          time_elapsed_seconds == 600;    // 10 minutes
                    
                    if should_log_trends {
                        let mut trend_count = 0;
                        
                        crate::log_println!("═══════════════════════════════════════════════════════════");
                        crate::log_println!("📊 TREND ANALYSIS REPORT - {}m {}s elapsed", 
                                          time_elapsed_seconds / 60, 
                                          time_elapsed_seconds % 60);
                        crate::log_println!("═══════════════════════════════════════════════════════════");
                        
                        // BTC Market
                        if let Some(btc_up) = snapshot.btc_market.up_token.as_ref() {
                            debug!("Logging trend for BTC Up token: {}", &btc_up.token_id[..16]);
                            simulation_tracker.log_trend_analysis(
                                snapshot.period_timestamp,
                                &btc_up.token_id,
                                &crate::detector::TokenType::BtcUp,
                                min_samples,
                            ).await;
                            trend_count += 1;
                        } else {
                            debug!("BTC Up token not available in snapshot");
                            crate::log_println!("⚠️  BTC Up: Token not available");
                        }
                        if let Some(btc_down) = snapshot.btc_market.down_token.as_ref() {
                            debug!("Logging trend for BTC Down token: {}", &btc_down.token_id[..16]);
                            simulation_tracker.log_trend_analysis(
                                snapshot.period_timestamp,
                                &btc_down.token_id,
                                &crate::detector::TokenType::BtcDown,
                                min_samples,
                            ).await;
                            trend_count += 1;
                        } else {
                            debug!("BTC Down token not available in snapshot");
                            crate::log_println!("⚠️  BTC Down: Token not available");
                        }
                        
                        // ETH Market
                        if enable_eth {
                            if let Some(eth_up) = snapshot.eth_market.up_token.as_ref() {
                                simulation_tracker.log_trend_analysis(
                                    snapshot.period_timestamp,
                                    &eth_up.token_id,
                                    &crate::detector::TokenType::EthUp,
                                    min_samples,
                                ).await;
                                trend_count += 1;
                            } else {
                                crate::log_println!("⚠️  ETH Up: Token not available");
                            }
                            if let Some(eth_down) = snapshot.eth_market.down_token.as_ref() {
                                simulation_tracker.log_trend_analysis(
                                    snapshot.period_timestamp,
                                    &eth_down.token_id,
                                    &crate::detector::TokenType::EthDown,
                                    min_samples,
                                ).await;
                                trend_count += 1;
                            } else {
                                crate::log_println!("⚠️  ETH Down: Token not available");
                            }
                        }
                        
                        // SOL Market
                        if enable_solana {
                            if let Some(solana_up) = snapshot.solana_market.up_token.as_ref() {
                                simulation_tracker.log_trend_analysis(
                                    snapshot.period_timestamp,
                                    &solana_up.token_id,
                                    &crate::detector::TokenType::SolanaUp,
                                    min_samples,
                                ).await;
                                trend_count += 1;
                            } else {
                                crate::log_println!("⚠️  SOL Up: Token not available");
                            }
                            if let Some(solana_down) = snapshot.solana_market.down_token.as_ref() {
                                simulation_tracker.log_trend_analysis(
                                    snapshot.period_timestamp,
                                    &solana_down.token_id,
                                    &crate::detector::TokenType::SolanaDown,
                                    min_samples,
                                ).await;
                                trend_count += 1;
                            } else {
                                crate::log_println!("⚠️  SOL Down: Token not available");
                            }
                        }
                        
                        // XRP Market
                        if enable_xrp {
                            if let Some(xrp_up) = snapshot.xrp_market.up_token.as_ref() {
                                simulation_tracker.log_trend_analysis(
                                    snapshot.period_timestamp,
                                    &xrp_up.token_id,
                                    &crate::detector::TokenType::XrpUp,
                                    min_samples,
                                ).await;
                                trend_count += 1;
                            } else {
                                crate::log_println!("⚠️  XRP Up: Token not available");
                            }
                            if let Some(xrp_down) = snapshot.xrp_market.down_token.as_ref() {
                                simulation_tracker.log_trend_analysis(
                                    snapshot.period_timestamp,
                                    &xrp_down.token_id,
                                    &crate::detector::TokenType::XrpDown,
                                    min_samples,
                                ).await;
                                trend_count += 1;
                            } else {
                                crate::log_println!("⚠️  XRP Down: Token not available");
                            }
                        }
                        
                        if trend_count == 0 {
                            crate::log_println!("⚠️  No tokens available for trend analysis");
                        }
                        
                        crate::log_println!("═══════════════════════════════════════════════════════════");
                    }
                }
            }

            // Early hedge detection: Monitor for price crossing $0.85 threshold and pattern detection
            // This runs continuously (not just at 10 minutes) to catch the pattern early
            if time_elapsed_seconds >= early_hedge_after_seconds { // Only check after dual_limit_early_hedge_minutes elapsed
                // Check if we've already executed early hedge for this period
                let hedge_executed = early_hedge_executed.lock().await;
                if !hedge_executed.contains(&snapshot.period_timestamp) {
                    drop(hedge_executed);
                    
                    async fn check_early_hedge_pattern(
                        trader: &Trader,
                        simulation_tracker: Option<&crate::simulation::SimulationTracker>,
                        snapshot: &crate::monitor::MarketSnapshot,
                        time_elapsed_seconds: u64,
                        hedge_price: f64,
                        limit_shares: Option<f64>,
                        fixed_trade_amount: f64,
                        _hedge_after_seconds: u64,
                        enable_eth: bool,
                        enable_solana: bool,
                        enable_xrp: bool,
                        trend_strength_threshold: f64,
                        min_samples: usize,
                        previous_prices: &Arc<tokio::sync::Mutex<std::collections::HashMap<String, f64>>>,
                    ) -> bool {
                        let mut markets_to_check = Vec::new();
                        
                        // BTC Market
                        if let (Some(btc_up), Some(btc_down)) = (snapshot.btc_market.up_token.as_ref(), snapshot.btc_market.down_token.as_ref()) {
                            markets_to_check.push((
                                "BTC",
                                &snapshot.btc_market.condition_id,
                                btc_up,
                                btc_down,
                                crate::detector::TokenType::BtcUp,
                                crate::detector::TokenType::BtcDown,
                            ));
                        }
                        
                        // ETH Market
                        if enable_eth {
                            if let (Some(eth_up), Some(eth_down)) = (snapshot.eth_market.up_token.as_ref(), snapshot.eth_market.down_token.as_ref()) {
                                markets_to_check.push((
                                    "ETH",
                                    &snapshot.eth_market.condition_id,
                                    eth_up,
                                    eth_down,
                                    crate::detector::TokenType::EthUp,
                                    crate::detector::TokenType::EthDown,
                                ));
                            }
                        }
                        
                        // SOL Market
                        if enable_solana {
                            if let (Some(sol_up), Some(sol_down)) = (snapshot.solana_market.up_token.as_ref(), snapshot.solana_market.down_token.as_ref()) {
                                markets_to_check.push((
                                    "SOL",
                                    &snapshot.solana_market.condition_id,
                                    sol_up,
                                    sol_down,
                                    crate::detector::TokenType::SolanaUp,
                                    crate::detector::TokenType::SolanaDown,
                                ));
                            }
                        }
                        
                        // XRP Market
                        if enable_xrp {
                            if let (Some(xrp_up), Some(xrp_down)) = (snapshot.xrp_market.up_token.as_ref(), snapshot.xrp_market.down_token.as_ref()) {
                                markets_to_check.push((
                                    "XRP",
                                    &snapshot.xrp_market.condition_id,
                                    xrp_up,
                                    xrp_down,
                                    crate::detector::TokenType::XrpUp,
                                    crate::detector::TokenType::XrpDown,
                                ));
                            }
                        }
                        
                        if markets_to_check.is_empty() {
                            return false;
                        }
                        
                        // Check each market for threshold crossing - collect ALL tokens that cross $0.85 OR are already above it
                        let mut crossed_tokens: Vec<(String, String, &crate::models::TokenPrice, crate::detector::TokenType, f64, &crate::models::TokenPrice, crate::detector::TokenType)> = Vec::new();
                        let mut prev_prices = previous_prices.lock().await;
                        
                        // Check all tokens (Up and Down) for threshold crossing OR already above threshold
                        for (market_name, condition_id, up_token, down_token, up_type, down_type) in &markets_to_check {
                            // Check if Up token price crossed $0.85 threshold (upward) OR is already above it
                            if let Some(current_bid) = up_token.bid {
                                let current_bid_f64 = f64::try_from(current_bid).unwrap_or(0.0);
                                let previous_price = prev_prices.get(&up_token.token_id).copied().unwrap_or(0.0);
                                
                                // Check if price crossed threshold OR is already above it (first time seeing it above)
                                let crossed_threshold = previous_price < hedge_price && current_bid_f64 >= hedge_price;
                                let already_above = previous_price == 0.0 && current_bid_f64 >= hedge_price; // First check, price already above
                                
                                if crossed_threshold || already_above {
                                    crossed_tokens.push((
                                        market_name.to_string(),
                                        condition_id.to_string(),
                                        up_token,
                                        up_type.clone(),
                                        current_bid_f64,
                                        down_token,
                                        down_type.clone(),
                                    ));
                                    crate::log_println!("🚨 {} Up token {} threshold: previous=${:.4}, current=${:.4}, threshold=${:.2}", 
                                        market_name, if crossed_threshold { "crossed" } else { "already above" }, previous_price, current_bid_f64, hedge_price);
                                }
                                
                                prev_prices.insert(up_token.token_id.clone(), current_bid_f64);
                            }
                            
                            // Check if Down token price crossed $0.85 threshold (upward) OR is already above it
                            if let Some(current_bid) = down_token.bid {
                                let current_bid_f64 = f64::try_from(current_bid).unwrap_or(0.0);
                                let previous_price = prev_prices.get(&down_token.token_id).copied().unwrap_or(0.0);
                                
                                // Check if price crossed threshold OR is already above it (first time seeing it above)
                                let crossed_threshold = previous_price < hedge_price && current_bid_f64 >= hedge_price;
                                let already_above = previous_price == 0.0 && current_bid_f64 >= hedge_price; // First check, price already above
                                
                                if crossed_threshold || already_above {
                                    crossed_tokens.push((
                                        market_name.to_string(),
                                        condition_id.to_string(),
                                        down_token,
                                        down_type.clone(),
                                        current_bid_f64,
                                        up_token,
                                        up_type.clone(),
                                    ));
                                    crate::log_println!("🚨 {} Down token {} threshold: previous=${:.4}, current=${:.4}, threshold=${:.2}", 
                                        market_name, if crossed_threshold { "crossed" } else { "already above" }, previous_price, current_bid_f64, hedge_price);
                                }
                                
                                prev_prices.insert(down_token.token_id.clone(), current_bid_f64);
                            }
                        }
                        drop(prev_prices);
                        
                        // Handle individual hedging for ALL markets with one-sided fill pattern
                        // If any token crossed $0.85, check ALL markets and handle individually
                        if !crossed_tokens.is_empty() {
                            crate::log_println!("🚨 {} token(s) crossed ${:.2} threshold - Checking ALL markets for individual hedging", 
                                crossed_tokens.len(), hedge_price);
                            
                            let mut any_hedge_executed = false;
                            
                            // Check ALL markets for one-sided fill pattern and handle individually
                            for (market_name, condition_id, up_token, down_token, up_type, down_type) in &markets_to_check {
                                // Check fill status for this market
                                let up_trade = trader.get_pending_limit_trade(snapshot.period_timestamp, &up_token.token_id).await;
                                let down_trade = trader.get_pending_limit_trade(snapshot.period_timestamp, &down_token.token_id).await;
                                
                                // Log if trades are missing
                                if up_trade.is_none() {
                                    crate::log_println!("⚠️  {} Up trade not found for period {} (token_id: {})", 
                                        market_name, snapshot.period_timestamp, &up_token.token_id[..16]);
                                }
                                if down_trade.is_none() {
                                    crate::log_println!("⚠️  {} Down trade not found for period {} (token_id: {})", 
                                        market_name, snapshot.period_timestamp, &down_token.token_id[..16]);
                                }
                                
                                // Handle case where one or both trades exist
                                // We need at least one trade to determine fill status
                                let (up_filled, down_filled) = match (up_trade.as_ref(), down_trade.as_ref()) {
                                    (Some(up_t), Some(down_t)) => {
                                        // Both trades exist - check for one-sided fill pattern
                                        (up_t.buy_order_confirmed, down_t.buy_order_confirmed)
                                    }
                                    (Some(up_t), None) => {
                                        // Only Up trade exists - check if it's filled and Down token price is above threshold
                                        let up_filled = up_t.buy_order_confirmed;
                                        if up_filled {
                                            // Up is filled, check if Down token price is above $0.85
                                            if let Some(down_bid) = down_token.bid {
                                                let down_price = f64::try_from(down_bid).unwrap_or(0.0);
                                                if down_price >= hedge_price {
                                                    crate::log_println!("⚠️  {} Up filled, Down trade missing but Down price ${:.4} >= ${:.2} - checking for hedge", 
                                                        market_name, down_price, hedge_price);
                                                    // Treat as if Down is unfilled and price is above threshold
                                                    (true, false)
                                                } else {
                                                    continue; // Down price not high enough
                                                }
                                            } else {
                                                continue; // No Down price available
                                            }
                                        } else {
                                            continue; // Up not filled, can't determine pattern
                                        }
                                    }
                                    (None, Some(down_t)) => {
                                        // Only Down trade exists - check if it's filled and Up token price is above threshold
                                        let down_filled = down_t.buy_order_confirmed;
                                        if down_filled {
                                            // Down is filled, check if Up token price is above $0.85
                                            if let Some(up_bid) = up_token.bid {
                                                let up_price = f64::try_from(up_bid).unwrap_or(0.0);
                                                if up_price >= hedge_price {
                                                    crate::log_println!("⚠️  {} Down filled, Up trade missing but Up price ${:.4} >= ${:.2} - checking for hedge", 
                                                        market_name, up_price, hedge_price);
                                                    // Treat as if Up is unfilled and price is above threshold
                                                    (false, true)
                                                } else {
                                                    continue; // Up price not high enough
                                                }
                                            } else {
                                                continue; // No Up price available
                                            }
                                        } else {
                                            continue; // Down not filled, can't determine pattern
                                        }
                                    }
                                    (None, None) => {
                                        // Neither trade exists - skip this market
                                        continue;
                                    }
                                };
                                
                                // Pattern: Exactly one side filled, other side NOT filled
                                if up_filled != down_filled {
                                    // Determine which token is unfilled
                                    let (unfilled_token, unfilled_type, filled_side_shares, current_price) = if !up_filled && down_filled {
                                        // Up is unfilled, Down is filled
                                        let price = if let Some(bid) = up_token.bid {
                                            f64::try_from(bid).unwrap_or(0.0)
                                        } else {
                                            continue;
                                        };
                                        let shares = down_trade.as_ref().map(|t| t.units).unwrap_or(0.0);
                                        (up_token, up_type.clone(), shares, price)
                                    } else if up_filled && !down_filled {
                                        // Down is unfilled, Up is filled
                                        let price = if let Some(bid) = down_token.bid {
                                            f64::try_from(bid).unwrap_or(0.0)
                                        } else {
                                            continue;
                                        };
                                        let shares = up_trade.as_ref().map(|t| t.units).unwrap_or(0.0);
                                        (down_token, down_type.clone(), shares, price)
                                    } else {
                                        continue;
                                    };
                                    
                                    // Verify price is actually above threshold
                                    if current_price < hedge_price {
                                        crate::log_println!("⚠️  {} {} token price ${:.4} < ${:.2} threshold - skipping hedge", 
                                            market_name, 
                                            match unfilled_type {
                                                crate::detector::TokenType::BtcUp | crate::detector::TokenType::EthUp | 
                                                crate::detector::TokenType::SolanaUp | crate::detector::TokenType::XrpUp => "Up",
                                                _ => "Down",
                                            },
                                            current_price, hedge_price);
                                        continue;
                                    }
                                    
                                    // Check if the UNFILLED token is uptrending
                                    let is_uptrending = if let Some(tracker) = simulation_tracker {
                                        tracker.is_token_uptrending(
                                            snapshot.period_timestamp,
                                            &unfilled_token.token_id,
                                            trend_strength_threshold,
                                            min_samples,
                                        ).await
                                    } else {
                                        // In production mode, if price is above threshold, consider it uptrending
                                        current_price >= hedge_price
                                    };
                                    
                                    let token_side = match unfilled_type {
                                        crate::detector::TokenType::BtcUp | 
                                        crate::detector::TokenType::EthUp | 
                                        crate::detector::TokenType::SolanaUp | 
                                        crate::detector::TokenType::XrpUp => "Up",
                                        _ => "Down",
                                    };
                                    
                                    if is_uptrending {
                                        // Buy this uptrending unfilled token with market order
                                        let hedge_shares = if let Some(limit_shares_val) = limit_shares {
                                            limit_shares_val
                                        } else if filled_side_shares > 0.0 {
                                            filled_side_shares
                                        } else {
                                            continue;
                                        };
                                        
                                        if hedge_shares <= 0.0 {
                                            continue;
                                        }
                                        
                                        // Always buy 2x the original amount for individual hedges
                                        let investment_amount = fixed_trade_amount * 2.0;
                                        let expected_shares = investment_amount / current_price;
                                        
                                        let has_crossed = current_price >= hedge_price;
                                        if has_crossed {
                                            crate::log_println!("🚨 INDIVIDUAL HEDGE: {} {} token crossed ${:.2} and is uptrending", market_name, token_side, hedge_price);
                                        } else {
                                            crate::log_println!("🚨 INDIVIDUAL HEDGE: {} {} token is uptrending (price ${:.4}, hasn't crossed ${:.2} yet)", 
                                                market_name, token_side, current_price, hedge_price);
                                        }
                                        crate::log_println!("   Pattern: One side filled, unfilled side is uptrending");
                                        crate::log_println!("   Executing MARKET order for {} unfilled token at ${:.4}...", market_name, current_price);
                                        crate::log_println!("   💰 Buying DOUBLE amount: ${:.2} for ~{:.6} shares", 
                                            investment_amount, expected_shares);
                                        
                                    // Cancel the unfilled limit order (if it exists)
                                    if let Err(e) = trader.cancel_pending_limit_buy(snapshot.period_timestamp, &unfilled_token.token_id).await {
                                        warn!("Failed to cancel {} {} limit order: {} (may not exist)", market_name, token_side, e);
                                        // Continue anyway - order may not exist
                                    }
                                    
                                    // Place MARKET buy order
                                    let opp = BuyOpportunity {
                                        condition_id: condition_id.to_string(),
                                        token_id: unfilled_token.token_id.clone(),
                                        token_type: unfilled_type.clone(),
                                        bid_price: current_price,
                                        period_timestamp: snapshot.period_timestamp,
                                        time_remaining_seconds: snapshot.time_remaining_seconds,
                                        time_elapsed_seconds,
                                        use_market_order: true, // MARKET ORDER
                                        investment_amount_override: Some(investment_amount), // Always 2x for individual hedges
                                        is_individual_hedge: true, // Mark as individual hedge to place limit sell order
                                        is_standard_hedge: false,
                                        dual_limit_shares: limit_shares, // Pass dual_limit_shares for sell orders
                                    };
                                    
                                    if let Err(e) = trader.execute_buy(&opp).await {
                                        warn!("Failed to execute market buy for {} {}: {}", market_name, token_side, e);
                                        continue; // Try next market
                                    } else {
                                        crate::log_println!("✅ Market buy executed for {} {}: ~{:.6} shares (double amount) at ~${:.4}", 
                                            market_name, token_side, expected_shares, current_price);
                                        crate::log_println!("   📤 Two limit sell orders will be placed at $0.93 and $0.98 after 7 seconds");
                                        any_hedge_executed = true;
                                    }
                                } else {
                                    // Not uptrending - skip it, will be handled later when it crosses $0.85 or at standard hedge time
                                    crate::log_println!("⏸️  {} {} unfilled token is NOT uptrending (price ${:.4}) - will be handled later", 
                                        market_name, token_side, current_price);
                                }
                            }
                        }
                            
                            if any_hedge_executed {
                                return true; // At least one individual hedge was executed
                            }
                        }
                        
                        false
                    }
                    
                    // Execute early hedge check
                    let hedge_executed = if is_simulation {
                        if let Some(simulation_tracker) = trader.get_simulation_tracker() {
                            check_early_hedge_pattern(
                                &trader,
                                Some(&*simulation_tracker),
                                &snapshot,
                                time_elapsed_seconds,
                                hedge_price,
                                limit_shares,
                                config.trading.fixed_trade_amount,
                                hedge_after_seconds,
                                enable_eth,
                                enable_solana,
                                enable_xrp,
                                config.trading.dual_limit_trend_strength_threshold.unwrap_or(0.3),
                                10, // min_samples
                                &previous_prices,
                            ).await
                        } else {
                            false
                        }
                    } else {
                        // In production mode, check without trend analysis
                        check_early_hedge_pattern(
                            &trader,
                            None,
                            &snapshot,
                            time_elapsed_seconds,
                            hedge_price,
                            limit_shares,
                            config.trading.fixed_trade_amount,
                            hedge_after_seconds,
                            enable_eth,
                            enable_solana,
                            enable_xrp,
                            0.3, // trend_strength_threshold (not used in production)
                            10,  // min_samples (not used in production)
                            &previous_prices,
                        ).await
                    };
                    
                    // Mark as executed if hedge was triggered
                    if hedge_executed {
                        let mut hedge_executed_set = early_hedge_executed.lock().await;
                        hedge_executed_set.insert(snapshot.period_timestamp);
                    }
                }
            }

            // Hedge logic: after N minutes, if exactly one side filled, buy the other side at current price (if >= hedge_price) and cancel the unfilled initial limit order.
            if time_elapsed_seconds >= hedge_after_seconds {
                async fn maybe_hedge_pair(
                    trader: &Trader,
                    condition_id: &str,
                    up: &Option<polymarket_arbitrage_bot::models::TokenPrice>,
                    down: &Option<polymarket_arbitrage_bot::models::TokenPrice>,
                    up_type: crate::detector::TokenType,
                    down_type: crate::detector::TokenType,
                    period_timestamp: u64,
                    time_remaining_seconds: u64,
                    time_elapsed_seconds: u64,
                    hedge_price: f64,
                    limit_shares: Option<f64>,
                    fixed_trade_amount: f64,
                ) {
                    let (Some(up), Some(down)) = (up.as_ref(), down.as_ref()) else { return; };

                    let up_trade = trader.get_pending_limit_trade(period_timestamp, &up.token_id).await;
                    let down_trade = trader.get_pending_limit_trade(period_timestamp, &down.token_id).await;

                    let (Some(up_trade), Some(down_trade)) = (up_trade, down_trade) else { return; };

                    let up_filled = up_trade.buy_order_confirmed;
                    let down_filled = down_trade.buy_order_confirmed;

                    // Only act when exactly one side is filled
                    if up_filled == down_filled {
                        return;
                    }

                    let (filled_trade, unfilled_trade, unfilled_token_id, unfilled_token_type, unfilled_buy_price_opt) = if up_filled {
                        (up_trade, down_trade, down.token_id.clone(), down_type, down.bid)
                    } else {
                        (down_trade, up_trade, up.token_id.clone(), up_type, up.bid)
                    };

                    // Avoid re-hedging: if the unfilled order is already at/above hedge_price, do nothing.
                    if unfilled_trade.purchase_price >= hedge_price - 1e-9 {
                        return;
                    }

                    let Some(unfilled_buy_price) = unfilled_buy_price_opt else { return; };
                    let unfilled_buy_price_f64 = f64::try_from(unfilled_buy_price).unwrap_or(0.0);
                    
                    // If price is already >= hedge_price, buy immediately at current price
                    if unfilled_buy_price_f64 >= hedge_price {
                        // Use dual_limit_shares if available, otherwise use filled trade's shares
                        let shares = if let Some(limit_shares_val) = limit_shares {
                            limit_shares_val
                        } else if filled_trade.units > 0.0 {
                            filled_trade.units
                        } else {
                            return; // Can't determine shares
                        };
                        
                        if shares <= 0.0 {
                            return;
                        }

                        // Double the investment amount for hedge benefit
                        let double_investment_amount = fixed_trade_amount * 2.0;

                        crate::log_println!(
                            "🛑 Hedge trigger: {} filled, {} unfilled. BUY price {:.4} >= {:.2}. Cancelling unfilled order and buying {} at current price {:.4} with market order (DOUBLE amount: ${:.6})",
                            filled_trade.token_type.display_name(),
                            unfilled_token_type.display_name(),
                            unfilled_buy_price_f64,
                            hedge_price,
                            unfilled_token_type.display_name(),
                            unfilled_buy_price_f64,
                            double_investment_amount
                        );

                        if let Err(e) = trader.cancel_pending_limit_buy(period_timestamp, &unfilled_token_id).await {
                            warn!("Failed to cancel unfilled limit buy: {}", e);
                            return;
                        }

                        let opp = BuyOpportunity {
                            condition_id: condition_id.to_string(),
                            token_id: unfilled_token_id,
                            token_type: unfilled_token_type,
                            bid_price: unfilled_buy_price_f64, // Buy at current price, not hedge_price
                            period_timestamp,
                            time_remaining_seconds,
                            time_elapsed_seconds,
                            use_market_order: true, // Use market order for standard hedge
                            investment_amount_override: Some(double_investment_amount),
                            is_individual_hedge: false,
                            is_standard_hedge: true, // This is a standard hedge (after dual_limit_hedge_after_minutes)
                            dual_limit_shares: limit_shares, // Pass dual_limit_shares for sell orders
                        };

                        if let Err(e) = trader.execute_buy(&opp).await {
                            warn!("Failed to place hedge buy: {}", e);
                        }
                    }
                    // If price < hedge_price, continue monitoring (will check again on next snapshot)
                }

                maybe_hedge_pair(
                    &trader,
                    &snapshot.btc_market.condition_id,
                    &snapshot.btc_market.up_token,
                    &snapshot.btc_market.down_token,
                    crate::detector::TokenType::BtcUp,
                    crate::detector::TokenType::BtcDown,
                    snapshot.period_timestamp,
                    snapshot.time_remaining_seconds,
                    time_elapsed_seconds,
                    hedge_price,
                    limit_shares,
                    fixed_trade_amount,
                )
                .await;

                if enable_eth {
                    maybe_hedge_pair(
                        &trader,
                        &snapshot.eth_market.condition_id,
                        &snapshot.eth_market.up_token,
                        &snapshot.eth_market.down_token,
                        crate::detector::TokenType::EthUp,
                        crate::detector::TokenType::EthDown,
                        snapshot.period_timestamp,
                        snapshot.time_remaining_seconds,
                        time_elapsed_seconds,
                        hedge_price,
                        limit_shares,
                        fixed_trade_amount,
                    )
                    .await;
                }

                if enable_solana {
                    maybe_hedge_pair(
                        &trader,
                        &snapshot.solana_market.condition_id,
                        &snapshot.solana_market.up_token,
                        &snapshot.solana_market.down_token,
                        crate::detector::TokenType::SolanaUp,
                        crate::detector::TokenType::SolanaDown,
                        snapshot.period_timestamp,
                        snapshot.time_remaining_seconds,
                        time_elapsed_seconds,
                        hedge_price,
                        limit_shares,
                        fixed_trade_amount,
                    )
                    .await;
                }

                if enable_xrp {
                    maybe_hedge_pair(
                        &trader,
                        &snapshot.xrp_market.condition_id,
                        &snapshot.xrp_market.up_token,
                        &snapshot.xrp_market.down_token,
                        crate::detector::TokenType::XrpUp,
                        crate::detector::TokenType::XrpDown,
                        snapshot.period_timestamp,
                        snapshot.time_remaining_seconds,
                        time_elapsed_seconds,
                        hedge_price,
                        limit_shares,
                        fixed_trade_amount,
                    )
                    .await;
                }
            }
        }
    }).await;

    Ok(())
}

// Copy helper functions from main.rs
async fn get_or_discover_markets(
    api: &PolymarketApi,
    enable_eth: bool,
    enable_solana: bool,
    enable_xrp: bool,
) -> Result<(crate::models::Market, crate::models::Market, crate::models::Market, crate::models::Market)> {
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let mut seen_ids = std::collections::HashSet::new();

    let eth_market = if enable_eth {
        discover_market(api, "ETH", &["eth"], current_time, &mut seen_ids, true).await
            .unwrap_or_else(|_| {
                eprintln!("⚠️  Could not discover ETH market - using fallback");
                disabled_eth_market()
            })
    } else {
        disabled_eth_market()
    };
    seen_ids.insert(eth_market.condition_id.clone());

    eprintln!("🔍 Discovering BTC market...");
    let btc_market = discover_market(api, "BTC", &["btc"], current_time, &mut seen_ids, true).await
        .unwrap_or_else(|_| {
            eprintln!("⚠️  Could not discover BTC market - using fallback");
            crate::models::Market {
                condition_id: "dummy_btc_fallback".to_string(),
                slug: "btc-updown-15m-fallback".to_string(),
                active: false,
                closed: true,
                market_id: None,
                question: "BTC Trading Disabled".to_string(),
                resolution_source: None,
                end_date_iso: None,
                end_date_iso_alt: None,
                tokens: None,
                clob_token_ids: None,
                outcomes: None,
            }
        });
    seen_ids.insert(btc_market.condition_id.clone());

    let solana_market = if enable_solana {
        discover_solana_market(api, current_time, &mut seen_ids).await
    } else {
        disabled_solana_market()
    };
    let xrp_market = if enable_xrp {
        discover_xrp_market(api, current_time, &mut seen_ids).await
    } else {
        disabled_xrp_market()
    };

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

fn enabled_markets_label(enable_eth: bool, enable_solana: bool, enable_xrp: bool) -> String {
    let mut enabled = Vec::new();
    if enable_eth {
        enabled.push("ETH");
    }
    if enable_solana {
        enabled.push("Solana");
    }
    if enable_xrp {
        enabled.push("XRP");
    }
    if enabled.is_empty() {
        "no additional".to_string()
    } else {
        enabled.join(", ")
    }
}

fn disabled_eth_market() -> crate::models::Market {
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
}

fn disabled_solana_market() -> crate::models::Market {
    crate::models::Market {
        condition_id: "dummy_solana_fallback".to_string(),
        slug: "solana-updown-15m-fallback".to_string(),
        active: false,
        closed: true,
        market_id: None,
        question: "Solana Trading Disabled".to_string(),
        resolution_source: None,
        end_date_iso: None,
        end_date_iso_alt: None,
        tokens: None,
        clob_token_ids: None,
        outcomes: None,
    }
}

fn disabled_xrp_market() -> crate::models::Market {
    crate::models::Market {
        condition_id: "dummy_xrp_fallback".to_string(),
        slug: "xrp-updown-15m-fallback".to_string(),
        active: false,
        closed: true,
        market_id: None,
        question: "XRP Trading Disabled".to_string(),
        resolution_source: None,
        end_date_iso: None,
        end_date_iso_alt: None,
        tokens: None,
        clob_token_ids: None,
        outcomes: None,
    }
}

async fn discover_solana_market(
    api: &PolymarketApi,
    current_time: u64,
    seen_ids: &mut std::collections::HashSet<String>,
) -> crate::models::Market {
    eprintln!("🔍 Discovering Solana market...");
    if let Ok(market) = discover_market(api, "Solana", &["solana", "sol"], current_time, seen_ids, false).await {
        return market;
    }
    eprintln!("⚠️  Could not discover Solana 15-minute market. Using fallback - Solana trading disabled.");
    disabled_solana_market()
}

async fn discover_xrp_market(
    api: &PolymarketApi,
    current_time: u64,
    seen_ids: &mut std::collections::HashSet<String>,
) -> crate::models::Market {
    eprintln!("🔍 Discovering XRP market...");
    if let Ok(market) = discover_market(api, "XRP", &["xrp"], current_time, seen_ids, false).await {
        return market;
    }
    eprintln!("⚠️  Could not discover XRP 15-minute market. Using fallback - XRP trading disabled.");
    disabled_xrp_market()
}

async fn discover_market(
    api: &PolymarketApi,
    market_name: &str,
    slug_prefixes: &[&str],
    current_time: u64,
    seen_ids: &mut std::collections::HashSet<String>,
    include_previous: bool,
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

        if include_previous {
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
    }

    let tried = slug_prefixes.join(", ");
    anyhow::bail!(
        "Could not find active {} 15-minute up/down market (tried prefixes: {}).",
        market_name,
        tried
    )
}


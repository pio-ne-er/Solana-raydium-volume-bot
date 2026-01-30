// Dual limit-start bot (1-hour): place Up/Down limit buys at market start with fixed price

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
use polymarket_arbitrage_bot::monitor::MarketMonitor;
use polymarket_arbitrage_bot::detector::BuyOpportunity;
use polymarket_arbitrage_bot::trader::Trader;

const DEFAULT_LIMIT_PRICE: f64 = 0.45;
const PERIOD_DURATION: u64 = 3600;

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
    HISTORY_FILE
        .set(Mutex::new(file))
        .expect("History file already initialized");
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
        .open("history_1h.toml")
        .context("Failed to open history_1h.toml for logging")?;

    init_history_file(log_file.try_clone().context("Failed to clone history file")?);

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

    eprintln!("🚀 Starting Polymarket Dual Limit-Start Bot (1-hour)");
    eprintln!("📝 Logs are being saved to: history_1h.toml");
    let is_simulation = args.is_simulation();
    eprintln!("Mode: {}", if is_simulation { "SIMULATION" } else { "PRODUCTION" });

    let limit_price = config.trading.dual_limit_price.unwrap_or(DEFAULT_LIMIT_PRICE);
    let limit_shares = config.trading.dual_limit_shares;
    eprintln!(
        "Strategy: At market start, place limit buys for BTC, ETH, SOL, and XRP Up/Down at ${:.2}",
        limit_price
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
        match api.authenticate().await {
            Ok(_) => eprintln!("✅ Authentication successful!"),
            Err(e) => {
                warn!("⚠️  Failed to authenticate: {}", e);
                warn!("⚠️  The bot will continue, but order placement may fail");
            }
        }
    } else {
        eprintln!("💡 Simulation mode: Skipping authentication");
    }

    eprintln!("🔍 Discovering 1-hour BTC, ETH, Solana, and XRP markets...");
    let (eth_market_data, btc_market_data, solana_market_data, xrp_market_data) =
        get_or_discover_markets_1h(
            &api,
            config.trading.enable_eth_trading,
            config.trading.enable_solana_trading,
            config.trading.enable_xrp_trading,
        )
        .await?;

    // NOTE: MarketMonitor is 15m-oriented internally but still provides bid/ask and a period timestamp.
    // For 1h markets, this binary focuses on market-start placement only.
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

    let trader = Trader::new(api.clone(), config.trading.clone(), is_simulation, None)?;
    let trader_arc = Arc::new(trader);
    let trader_clone = trader_arc.clone();

    let last_placed_period = Arc::new(tokio::sync::Mutex::new(None::<u64>));
    let last_seen_period = Arc::new(tokio::sync::Mutex::new(None::<u64>));
    let enable_eth = config.trading.enable_eth_trading;
    let enable_solana = config.trading.enable_solana_trading;
    let enable_xrp = config.trading.enable_xrp_trading;

    monitor_arc
        .start_monitoring(move |snapshot| {
            let trader = trader_clone.clone();
            let last_placed_period = last_placed_period.clone();
            let last_seen_period = last_seen_period.clone();

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

                let time_elapsed_seconds = PERIOD_DURATION.saturating_sub(snapshot.time_remaining_seconds);
                if time_elapsed_seconds > 2 {
                    return;
                }

                {
                    let mut last = last_placed_period.lock().await;
                    if last.map(|p| p == snapshot.period_timestamp).unwrap_or(false) {
                        return;
                    }
                    *last = Some(snapshot.period_timestamp);
                }

                let mut opportunities: Vec<BuyOpportunity> = Vec::new();

                if let Some(btc_up) = snapshot.btc_market.up_token.as_ref() {
                    opportunities.push(BuyOpportunity {
                        condition_id: snapshot.btc_market.condition_id.clone(),
                        token_id: btc_up.token_id.clone(),
                        token_type: polymarket_arbitrage_bot::detector::TokenType::BtcUp,
                        bid_price: limit_price,
                        period_timestamp: snapshot.period_timestamp,
                        time_remaining_seconds: snapshot.time_remaining_seconds,
                        time_elapsed_seconds,
                        use_market_order: false,
                    });
                }
                if let Some(btc_down) = snapshot.btc_market.down_token.as_ref() {
                    opportunities.push(BuyOpportunity {
                        condition_id: snapshot.btc_market.condition_id.clone(),
                        token_id: btc_down.token_id.clone(),
                        token_type: polymarket_arbitrage_bot::detector::TokenType::BtcDown,
                        bid_price: limit_price,
                        period_timestamp: snapshot.period_timestamp,
                        time_remaining_seconds: snapshot.time_remaining_seconds,
                        time_elapsed_seconds,
                        use_market_order: false,
                    });
                }

                if enable_eth {
                    if let Some(eth_up) = snapshot.eth_market.up_token.as_ref() {
                        opportunities.push(BuyOpportunity {
                            condition_id: snapshot.eth_market.condition_id.clone(),
                            token_id: eth_up.token_id.clone(),
                            token_type: polymarket_arbitrage_bot::detector::TokenType::EthUp,
                            bid_price: limit_price,
                            period_timestamp: snapshot.period_timestamp,
                            time_remaining_seconds: snapshot.time_remaining_seconds,
                            time_elapsed_seconds,
                            use_market_order: false,
                        });
                    }
                    if let Some(eth_down) = snapshot.eth_market.down_token.as_ref() {
                        opportunities.push(BuyOpportunity {
                            condition_id: snapshot.eth_market.condition_id.clone(),
                            token_id: eth_down.token_id.clone(),
                            token_type: polymarket_arbitrage_bot::detector::TokenType::EthDown,
                            bid_price: limit_price,
                            period_timestamp: snapshot.period_timestamp,
                            time_remaining_seconds: snapshot.time_remaining_seconds,
                            time_elapsed_seconds,
                            use_market_order: false,
                        });
                    }
                }

                if enable_solana {
                    if let Some(solana_up) = snapshot.solana_market.up_token.as_ref() {
                        opportunities.push(BuyOpportunity {
                            condition_id: snapshot.solana_market.condition_id.clone(),
                            token_id: solana_up.token_id.clone(),
                            token_type: polymarket_arbitrage_bot::detector::TokenType::SolanaUp,
                            bid_price: limit_price,
                            period_timestamp: snapshot.period_timestamp,
                            time_remaining_seconds: snapshot.time_remaining_seconds,
                            time_elapsed_seconds,
                            use_market_order: false,
                        });
                    }
                    if let Some(solana_down) = snapshot.solana_market.down_token.as_ref() {
                        opportunities.push(BuyOpportunity {
                            condition_id: snapshot.solana_market.condition_id.clone(),
                            token_id: solana_down.token_id.clone(),
                            token_type: polymarket_arbitrage_bot::detector::TokenType::SolanaDown,
                            bid_price: limit_price,
                            period_timestamp: snapshot.period_timestamp,
                            time_remaining_seconds: snapshot.time_remaining_seconds,
                            time_elapsed_seconds,
                            use_market_order: false,
                        });
                    }
                }

                if enable_xrp {
                    if let Some(xrp_up) = snapshot.xrp_market.up_token.as_ref() {
                        opportunities.push(BuyOpportunity {
                            condition_id: snapshot.xrp_market.condition_id.clone(),
                            token_id: xrp_up.token_id.clone(),
                            token_type: polymarket_arbitrage_bot::detector::TokenType::XrpUp,
                            bid_price: limit_price,
                            period_timestamp: snapshot.period_timestamp,
                            time_remaining_seconds: snapshot.time_remaining_seconds,
                            time_elapsed_seconds,
                            use_market_order: false,
                        });
                    }
                    if let Some(xrp_down) = snapshot.xrp_market.down_token.as_ref() {
                        opportunities.push(BuyOpportunity {
                            condition_id: snapshot.xrp_market.condition_id.clone(),
                            token_id: xrp_down.token_id.clone(),
                            token_type: polymarket_arbitrage_bot::detector::TokenType::XrpDown,
                            bid_price: limit_price,
                            period_timestamp: snapshot.period_timestamp,
                            time_remaining_seconds: snapshot.time_remaining_seconds,
                            time_elapsed_seconds,
                            use_market_order: false,
                        });
                    }
                }

                crate::log_println!("🎯 Market start detected - placing 1h limit buys at ${:.2}", limit_price);
                for opportunity in opportunities {
                    if trader.has_active_position(opportunity.period_timestamp, opportunity.token_type.clone()).await {
                        continue;
                    }
                    if let Err(e) = trader.execute_limit_buy(&opportunity, false, limit_shares).await {
                        warn!("Error executing limit buy: {}", e);
                    }
                }
            }
        })
        .await;

    Ok(())
}

async fn get_or_discover_markets_1h(
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
        discover_market_1h(api, "ETH", &["eth"], current_time, &mut seen_ids).await
            .unwrap_or_else(|_| disabled_eth_market())
    } else {
        disabled_eth_market()
    };
    seen_ids.insert(eth_market.condition_id.clone());

    let btc_market = discover_market_1h(api, "BTC", &["btc"], current_time, &mut seen_ids).await
        .unwrap_or_else(|_| {
            crate::models::Market {
                condition_id: "dummy_btc_fallback".to_string(),
                slug: "btc-updown-1h-fallback".to_string(),
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
        discover_market_1h(api, "Solana", &["solana", "sol"], current_time, &mut seen_ids).await
            .unwrap_or_else(|_| disabled_solana_market())
    } else {
        disabled_solana_market()
    };

    let xrp_market = if enable_xrp {
        discover_market_1h(api, "XRP", &["xrp"], current_time, &mut seen_ids).await
            .unwrap_or_else(|_| disabled_xrp_market())
    } else {
        disabled_xrp_market()
    };

    Ok((eth_market, btc_market, solana_market, xrp_market))
}

async fn discover_market_1h(
    api: &PolymarketApi,
    market_name: &str,
    slug_prefixes: &[&str],
    current_time: u64,
    seen_ids: &mut std::collections::HashSet<String>,
) -> Result<crate::models::Market> {
    let rounded_time = (current_time / 3600) * 3600;

    for prefix in slug_prefixes {
        let slug = format!("{}-updown-1h-{}", prefix, rounded_time);
        if let Ok(market) = api.get_market_by_slug(&slug).await {
            if !seen_ids.contains(&market.condition_id) && market.active && !market.closed {
                eprintln!("Found {} market by slug: {} | Condition ID: {}", market_name, market.slug, market.condition_id);
                return Ok(market);
            }
        }

        for offset in 1..=3 {
            let try_time = rounded_time.saturating_sub(offset * 3600);
            let try_slug = format!("{}-updown-1h-{}", prefix, try_time);
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
        "Could not find active {} 1-hour up/down market (tried prefixes: {}).",
        market_name,
        tried
    )
}

fn disabled_eth_market() -> crate::models::Market {
    crate::models::Market {
        condition_id: "dummy_eth_fallback".to_string(),
        slug: "eth-updown-1h-fallback".to_string(),
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
        slug: "solana-updown-1h-fallback".to_string(),
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
        slug: "xrp-updown-1h-fallback".to_string(),
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


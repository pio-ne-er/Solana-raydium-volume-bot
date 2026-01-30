// Price monitoring only - no trading
// Monitors real-time prices and records them to history folder
use polymarket_arbitrage_bot::*;

use anyhow::{Context, Result};
use clap::Parser;
use polymarket_arbitrage_bot::config::{Args, Config};
use log::warn;
use std::sync::Arc;

use polymarket_arbitrage_bot::api::PolymarketApi;
use polymarket_arbitrage_bot::monitor::MarketMonitor;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    let args = Args::parse();
    let config = Config::load(&args.config)?;

    eprintln!("═══════════════════════════════════════════════════════════");
    eprintln!("📊 PRICE MONITORING MODE");
    eprintln!("═══════════════════════════════════════════════════════════");
    eprintln!("This mode only monitors and records prices - NO TRADING");
    eprintln!("Prices will be recorded to: history/market_<PERIOD>_prices.toml");
    eprintln!("═══════════════════════════════════════════════════════════");
    eprintln!("");

    // Create API client (no authentication needed for price monitoring)
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

    // Get market data for BTC, ETH, Solana, and XRP markets
    eprintln!("🔍 Discovering BTC, ETH, Solana, and XRP markets...");
    let (eth_market_data, btc_market_data, solana_market_data, xrp_market_data) = 
        get_or_discover_markets(&api, &config).await?;

    eprintln!("✅ Markets discovered:");
    eprintln!("   ETH: {} ({})", eth_market_data.slug, &eth_market_data.condition_id[..16]);
    eprintln!("   BTC: {} ({})", btc_market_data.slug, &btc_market_data.condition_id[..16]);
    eprintln!("   Solana: {} ({})", solana_market_data.slug, &solana_market_data.condition_id[..16]);
    eprintln!("   XRP: {} ({})", xrp_market_data.slug, &xrp_market_data.condition_id[..16]);
    eprintln!("");

    // Create market monitor (simulation_mode = false, but we're not trading anyway)
    let monitor = Arc::new(MarketMonitor::new(
        api.clone(),
        eth_market_data,
        btc_market_data,
        solana_market_data,
        xrp_market_data,
        config.trading.check_interval_ms,
        false, // Not simulation mode, but we're just monitoring
    )?);

    eprintln!("🔄 Starting price monitoring...");
    eprintln!("   Check interval: {}ms", config.trading.check_interval_ms);
    eprintln!("   Price files: history/market_<PERIOD>_prices.toml");
    eprintln!("");

    // Start a background task to detect new 15-minute periods and discover new markets
    let monitor_for_period_check = monitor.clone();
    let api_for_period_check = api.clone();
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
                eprintln!("🔄 New 15-minute period detected! (Period: {}) Discovering new markets...", current_period);
                
                let mut seen_ids = std::collections::HashSet::new();
                let (eth_id, btc_id) = monitor_for_period_check.get_current_condition_ids().await;
                seen_ids.insert(eth_id);
                seen_ids.insert(btc_id);
                
                // Discover ETH, BTC, Solana, and XRP for the new period
                let eth_result = discover_market(&api_for_period_check, "ETH", &["eth"], current_time, &mut seen_ids).await;
                let btc_result = discover_market(&api_for_period_check, "BTC", &["btc"], current_time, &mut seen_ids).await;
                let solana_market = discover_solana_market(&api_for_period_check, current_time, &mut seen_ids).await;
                let xrp_market = discover_xrp_market(&api_for_period_check, current_time, &mut seen_ids).await;
                
                match (eth_result, btc_result) {
                    (Ok(eth_market), Ok(btc_market)) => {
                        if let Err(e) = monitor_for_period_check.update_markets(eth_market, btc_market, solana_market, xrp_market).await {
                            warn!("Failed to update markets: {}", e);
                        } else {
                            eprintln!("✅ Markets updated for period {}", current_period);
                        }
                    }
                    _ => {
                        warn!("Failed to discover markets for period {}", current_period);
                    }
                }
            } else {
                // Calculate when next period starts
                let next_period_timestamp = current_period + 900;
                let sleep_duration = if next_period_timestamp > current_time {
                    next_period_timestamp - current_time
                } else {
                    0
                };
                
                if sleep_duration > 0 {
                    tokio::time::sleep(tokio::time::Duration::from_secs(sleep_duration)).await;
                } else {
                    // Next period already started, discover new market immediately
                    eprintln!("🔄 Next period already started, discovering new market...");
                }
            }
        }
    });

    // Main loop: continuously fetch and record prices
    loop {
        match monitor.fetch_market_data().await {
            Ok(snapshot) => {
                // Prices are automatically written to history files by MarketMonitor
                // The monitor already logs prices to files, so we don't need to log here
                // Just continue monitoring
            }
            Err(e) => {
                warn!("Error fetching market data: {}", e);
            }
        }
        
        // Sleep for the configured interval
        tokio::time::sleep(tokio::time::Duration::from_millis(config.trading.check_interval_ms)).await;
    }
}

/// Get or discover markets for ETH, BTC, Solana, and XRP
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
    
    // Discover ETH, BTC, and Solana markets
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

    // Discover Solana market
    eprintln!("🔍 Discovering Solana market...");
    let solana_market = discover_solana_market(api, current_time, &mut seen_ids).await;

    // Discover XRP market
    eprintln!("🔍 Discovering XRP market...");
    let xrp_market = discover_xrp_market(api, current_time, &mut seen_ids).await;

    Ok((eth_market, btc_market, solana_market, xrp_market))
}

/// Discover Solana 15m market. Tries slug prefixes ["solana", "sol"] via discover_market.
/// Returns a dummy fallback if not found.
async fn discover_solana_market(
    api: &PolymarketApi,
    current_time: u64,
    seen_ids: &mut std::collections::HashSet<String>,
) -> crate::models::Market {
    eprintln!("🔍 Discovering Solana market...");
    if let Ok(market) = discover_market(api, "Solana", &["solana", "sol"], current_time, seen_ids).await {
        return market;
    }
    eprintln!("⚠️  Could not discover Solana 15-minute market (tried: solana, sol). Using fallback.");
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
/// Returns a dummy fallback if not found.
async fn discover_xrp_market(
    api: &PolymarketApi,
    current_time: u64,
    seen_ids: &mut std::collections::HashSet<String>,
) -> crate::models::Market {
    eprintln!("🔍 Discovering XRP market...");
    if let Ok(market) = discover_market(api, "XRP", &["xrp"], current_time, seen_ids).await {
        return market;
    }
    eprintln!("⚠️  Could not discover XRP 15-minute market (tried: xrp). Using fallback.");
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

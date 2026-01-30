use crate::api::PolymarketApi;
use crate::models::*;
use anyhow::Result;
use log::{debug, info, warn};
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use std::fs::OpenOptions;
use std::io::Write;
use chrono::Utc;

pub struct MarketMonitor {
    api: Arc<PolymarketApi>,
    eth_market: Arc<tokio::sync::Mutex<crate::models::Market>>,
    btc_market: Arc<tokio::sync::Mutex<crate::models::Market>>,
    solana_market: Arc<tokio::sync::Mutex<crate::models::Market>>,
    xrp_market: Arc<tokio::sync::Mutex<crate::models::Market>>,
    check_interval: Duration,
    // Cached token IDs from getMarket() - refreshed once per period
    eth_up_token_id: Arc<tokio::sync::Mutex<Option<String>>>,
    eth_down_token_id: Arc<tokio::sync::Mutex<Option<String>>>,
    btc_up_token_id: Arc<tokio::sync::Mutex<Option<String>>>,
    btc_down_token_id: Arc<tokio::sync::Mutex<Option<String>>>,
    solana_up_token_id: Arc<tokio::sync::Mutex<Option<String>>>,
    solana_down_token_id: Arc<tokio::sync::Mutex<Option<String>>>,
    xrp_up_token_id: Arc<tokio::sync::Mutex<Option<String>>>,
    xrp_down_token_id: Arc<tokio::sync::Mutex<Option<String>>>,
    last_market_refresh: Arc<tokio::sync::Mutex<Option<std::time::Instant>>>,
    current_period_timestamp: Arc<tokio::sync::Mutex<u64>>, // Track current 15-minute period
    btc_market_end_timestamp: Arc<tokio::sync::Mutex<Option<u64>>>, // Actual market end time from API
    eth_market_end_timestamp: Arc<tokio::sync::Mutex<Option<u64>>>, // Actual ETH market end time from API
    solana_market_end_timestamp: Arc<tokio::sync::Mutex<Option<u64>>>, // Actual Solana market end time from API
    xrp_market_end_timestamp: Arc<tokio::sync::Mutex<Option<u64>>>, // Actual XRP market end time from API
    simulation_mode: bool,
    price_monitor_file: Option<Arc<tokio::sync::Mutex<std::fs::File>>>, // File for logging price monitoring data in simulation mode
    market_price_files: Arc<tokio::sync::Mutex<std::collections::HashMap<String, Arc<tokio::sync::Mutex<std::fs::File>>>>>, // Per-market price files
}

#[derive(Debug, Clone)]
pub struct MarketSnapshot {
    pub eth_market: MarketData,
    pub btc_market: MarketData,
    pub solana_market: MarketData,
    pub xrp_market: MarketData,
    pub timestamp: std::time::Instant,
    pub time_remaining_seconds: u64, // Time remaining in the current 15-minute period
    pub period_timestamp: u64, // The 15-minute period timestamp (e.g., 1767796200)
}

impl MarketMonitor {
    pub fn new(
        api: Arc<PolymarketApi>,
        eth_market: crate::models::Market,
        btc_market: crate::models::Market,
        solana_market: crate::models::Market,
        xrp_market: crate::models::Market,
        check_interval_ms: u64,
        simulation_mode: bool,
    ) -> Result<Self> {
        // Calculate current 15-minute period timestamp
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let current_period = (current_time / 900) * 900; // Round to nearest 15 minutes
        
        // Create price monitor file if in simulation mode
        let price_monitor_file = if simulation_mode {
            match OpenOptions::new()
                .create(true)
                .append(true)
                .open("price_monitor.toml")
            {
                Ok(file) => Some(Arc::new(tokio::sync::Mutex::new(file))),
                Err(e) => {
                    warn!("Failed to open price_monitor.toml: {}", e);
                    None
                }
            }
        } else {
            None
        };
        
        Ok(Self {
            api,
            eth_market: Arc::new(tokio::sync::Mutex::new(eth_market)),
            btc_market: Arc::new(tokio::sync::Mutex::new(btc_market)),
            solana_market: Arc::new(tokio::sync::Mutex::new(solana_market)),
            xrp_market: Arc::new(tokio::sync::Mutex::new(xrp_market)),
            check_interval: Duration::from_millis(check_interval_ms),
            eth_up_token_id: Arc::new(tokio::sync::Mutex::new(None)),
            eth_down_token_id: Arc::new(tokio::sync::Mutex::new(None)),
            btc_up_token_id: Arc::new(tokio::sync::Mutex::new(None)),
            btc_down_token_id: Arc::new(tokio::sync::Mutex::new(None)),
            solana_up_token_id: Arc::new(tokio::sync::Mutex::new(None)),
            solana_down_token_id: Arc::new(tokio::sync::Mutex::new(None)),
            xrp_up_token_id: Arc::new(tokio::sync::Mutex::new(None)),
            xrp_down_token_id: Arc::new(tokio::sync::Mutex::new(None)),
            last_market_refresh: Arc::new(tokio::sync::Mutex::new(None)),
            current_period_timestamp: Arc::new(tokio::sync::Mutex::new(current_period)),
            btc_market_end_timestamp: Arc::new(tokio::sync::Mutex::new(None)),
            eth_market_end_timestamp: Arc::new(tokio::sync::Mutex::new(None)),
            solana_market_end_timestamp: Arc::new(tokio::sync::Mutex::new(None)),
            xrp_market_end_timestamp: Arc::new(tokio::sync::Mutex::new(None)),
            simulation_mode,
            price_monitor_file,
            market_price_files: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        })
    }

    /// Update markets when a new 15-minute period starts
    pub async fn update_markets(&self, eth_market: crate::models::Market, btc_market: crate::models::Market, solana_market: crate::models::Market, xrp_market: crate::models::Market) -> Result<()> {
        eprintln!("🔄 Updating to new 15-minute period markets...");
        eprintln!("✅ ETH Market: {} ({}) - Active trading", eth_market.slug, eth_market.condition_id);
        eprintln!("✅ BTC Market: {} ({}) - Active trading", btc_market.slug, btc_market.condition_id);
        eprintln!("✅ Solana Market: {} ({}) - Active trading", solana_market.slug, solana_market.condition_id);
        eprintln!("✅ XRP Market: {} ({}) - Active trading", xrp_market.slug, xrp_market.condition_id);
        
        // Log new market start to history.toml (trading event)
        let period = eth_market.slug.split('-').last().unwrap_or("unknown");
        crate::log_trading_event(&format!("🆕 NEW MARKET STARTED | Period: {} | ETH: {} | BTC: {} | SOL: {} | XRP: {}", 
            period,
            eth_market.condition_id,
            btc_market.condition_id,
            solana_market.condition_id,
            xrp_market.condition_id));
        
        *self.eth_market.lock().await = eth_market;
        *self.btc_market.lock().await = btc_market;
        *self.solana_market.lock().await = solana_market;
        *self.xrp_market.lock().await = xrp_market;
        
        // Reset token IDs - will be refreshed on next fetch
        *self.eth_up_token_id.lock().await = None;
        *self.eth_down_token_id.lock().await = None;
        *self.btc_up_token_id.lock().await = None;
        *self.btc_down_token_id.lock().await = None;
        *self.solana_up_token_id.lock().await = None;
        *self.solana_down_token_id.lock().await = None;
        *self.xrp_up_token_id.lock().await = None;
        *self.xrp_down_token_id.lock().await = None;
        *self.last_market_refresh.lock().await = None;
        
        // Update current period timestamp
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let new_period = (current_time / 900) * 900;
        *self.current_period_timestamp.lock().await = new_period;
        
        Ok(())
    }


    /// Get current market condition IDs (for checking if markets are closed)
    pub async fn get_current_condition_ids(&self) -> (String, String) {
        let eth = self.eth_market.lock().await.condition_id.clone();
        let btc = self.btc_market.lock().await.condition_id.clone();
        (eth, btc)
    }

    /// Get the current market's timestamp from the BTC market slug
    pub async fn get_current_market_timestamp(&self) -> u64 {
        let btc_market = self.btc_market.lock().await;
        let timestamp = Self::extract_timestamp_from_slug(&btc_market.slug);
        // If BTC slug doesn't have a valid timestamp, fall back to current period
        if timestamp == 0 {
            let current_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            (current_time / 900) * 900
        } else {
            timestamp
        }
    }

    /// Refresh market data once per period (15 minutes) to get token IDs
    async fn refresh_market_tokens(&self) -> Result<()> {
        // Check if we need to refresh (every 15 minutes = 900 seconds)
        let should_refresh = {
            let last_refresh = self.last_market_refresh.lock().await;
            last_refresh
                .map(|last| last.elapsed().as_secs() >= 900)
                .unwrap_or(true)
        };

        if !should_refresh {
            return Ok(());
        }


        let (eth_condition_id, btc_condition_id) = self.get_current_condition_ids().await;
        let solana_condition_id = {
            let solana_market = self.solana_market.lock().await;
            solana_market.condition_id.clone()
        };
        let xrp_condition_id = {
            let xrp_market = self.xrp_market.lock().await;
            xrp_market.condition_id.clone()
        };

        // Get ETH market details
        if let Ok(eth_details) = self.api.get_market(&eth_condition_id).await {
            for token in &eth_details.tokens {
                let outcome_upper = token.outcome.to_uppercase();
                if outcome_upper.contains("UP") || outcome_upper == "1" {
                    *self.eth_up_token_id.lock().await = Some(token.token_id.clone());
                    eprintln!("ETH Up token_id: {}", token.token_id);
                } else if outcome_upper.contains("DOWN") || outcome_upper == "0" {
                    *self.eth_down_token_id.lock().await = Some(token.token_id.clone());
                    eprintln!("ETH Down token_id: {}", token.token_id);
                }
            }
            
            // Store ETH market end time for accurate remaining time calculation
            // Parse end_date_iso (ISO 8601 format) to Unix timestamp
            // For now, we'll skip this and use the slug-based calculation
            // TODO: Implement proper ISO date parsing if needed
            // if let Ok(end_timestamp) = Self::parse_iso_to_timestamp(&eth_details.end_date_iso) {
            //     *self.eth_market_end_timestamp.lock().await = Some(end_timestamp);
            // }
        }

        // Get BTC market details
        if let Ok(btc_details) = self.api.get_market(&btc_condition_id).await {
            for token in &btc_details.tokens {
                let outcome_upper = token.outcome.to_uppercase();
                if outcome_upper.contains("UP") || outcome_upper == "1" {
                    *self.btc_up_token_id.lock().await = Some(token.token_id.clone());
                    eprintln!("BTC Up token_id: {}", token.token_id);
                } else if outcome_upper.contains("DOWN") || outcome_upper == "0" {
                    *self.btc_down_token_id.lock().await = Some(token.token_id.clone());
                    eprintln!("BTC Down token_id: {}", token.token_id);
                }
            }
            
            // Store market end time for accurate remaining time calculation
            // Parse end_date_iso (ISO 8601 format) to Unix timestamp
            // For now, we'll skip this and use the slug-based calculation
            // TODO: Implement proper ISO date parsing if needed
            // if let Ok(end_timestamp) = Self::parse_iso_to_timestamp(&btc_details.end_date_iso) {
            //     *self.btc_market_end_timestamp.lock().await = Some(end_timestamp);
            // }
        }

        // Get Solana market details (skip if dummy fallback - no real market)
        if solana_condition_id != "dummy_solana_fallback" {
            if let Ok(solana_details) = self.api.get_market(&solana_condition_id).await {
                for token in &solana_details.tokens {
                let outcome_upper = token.outcome.to_uppercase();
                if outcome_upper.contains("UP") || outcome_upper == "1" {
                    *self.solana_up_token_id.lock().await = Some(token.token_id.clone());
                    eprintln!("Solana Up token_id: {}", token.token_id);
                } else if outcome_upper.contains("DOWN") || outcome_upper == "0" {
                    *self.solana_down_token_id.lock().await = Some(token.token_id.clone());
                    eprintln!("Solana Down token_id: {}", token.token_id);
                }
            }
            
            // Store Solana market end time for accurate remaining time calculation
            // Parse end_date_iso (ISO 8601 format) to Unix timestamp
            // For now, we'll skip this and use the slug-based calculation
            // TODO: Implement proper ISO date parsing if needed
            // if let Ok(end_timestamp) = Self::parse_iso_to_timestamp(&solana_details.end_date_iso) {
            //     *self.solana_market_end_timestamp.lock().await = Some(end_timestamp);
            // }
            }
        }

        // Get XRP market details (skip if dummy fallback - no real market)
        if xrp_condition_id != "dummy_xrp_fallback" {
            if let Ok(xrp_details) = self.api.get_market(&xrp_condition_id).await {
                for token in &xrp_details.tokens {
                    let outcome_upper = token.outcome.to_uppercase();
                    if outcome_upper.contains("UP") || outcome_upper == "1" {
                        *self.xrp_up_token_id.lock().await = Some(token.token_id.clone());
                        eprintln!("XRP Up token_id: {}", token.token_id);
                    } else if outcome_upper.contains("DOWN") || outcome_upper == "0" {
                        *self.xrp_down_token_id.lock().await = Some(token.token_id.clone());
                        eprintln!("XRP Down token_id: {}", token.token_id);
                    }
                }
                // Store XRP market end time for accurate remaining time calculation (optional)
            }
        }

        *self.last_market_refresh.lock().await = Some(std::time::Instant::now());
        Ok(())
    }

    /// Fetch current market data for both ETH and BTC markets
    /// Uses get_price() endpoint continuously for real-time prices
    pub async fn fetch_market_data(&self) -> Result<MarketSnapshot> {
        // Refresh token IDs if needed (once per 15-minute period)
        self.refresh_market_tokens().await?;

        // Get market slugs to extract timestamps
        let eth_market_guard = self.eth_market.lock().await;
        let btc_market_guard = self.btc_market.lock().await;
        let solana_market_guard = self.solana_market.lock().await;
        let xrp_market_guard = self.xrp_market.lock().await;
        let eth_slug = eth_market_guard.slug.clone();
        let btc_slug = btc_market_guard.slug.clone();
        let solana_slug = solana_market_guard.slug.clone();
        let xrp_slug = xrp_market_guard.slug.clone();
        drop(eth_market_guard);
        drop(btc_market_guard);
        drop(solana_market_guard);
        drop(xrp_market_guard);

        // Extract market timestamp from slug (e.g., "eth-updown-15m-1767796200" -> 1767796200)
        let eth_market_timestamp = Self::extract_timestamp_from_slug(&eth_slug);
        let btc_market_timestamp = Self::extract_timestamp_from_slug(&btc_slug);
        let solana_market_timestamp = Self::extract_timestamp_from_slug(&solana_slug);
        let xrp_market_timestamp = Self::extract_timestamp_from_slug(&xrp_slug);

        let (eth_condition_id, btc_condition_id) = self.get_current_condition_ids().await;
        let solana_condition_id = {
            let solana_market = self.solana_market.lock().await;
            solana_market.condition_id.clone()
        };
        let xrp_condition_id = {
            let xrp_market = self.xrp_market.lock().await;
            xrp_market.condition_id.clone()
        };

        // Get current timestamp
        let current_timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Calculate remaining time - use actual market end time from API if available
        // Otherwise fall back to slug timestamp + 900 seconds
        const PERIOD_DURATION: u64 = 900; // 15 minutes in seconds (fallback)
        
        // Try to use actual market end time from API
        let btc_market_end = {
            let stored_end = self.btc_market_end_timestamp.lock().await;
            stored_end.clone()
        };
        
        let eth_market_end = {
            let stored_end = self.eth_market_end_timestamp.lock().await;
            stored_end.clone()
        };
        
        let solana_market_end = {
            let stored_end = self.solana_market_end_timestamp.lock().await;
            stored_end.clone()
        };
        let xrp_market_end = {
            let stored_end = self.xrp_market_end_timestamp.lock().await;
            stored_end.clone()
        };
        
        let btc_period_end = if let Some(api_end_time) = btc_market_end {
            api_end_time // Use actual end time from API
        } else {
            btc_market_timestamp + PERIOD_DURATION // Fallback to slug + 900s
        };
        
        let eth_period_end = if let Some(api_end_time) = eth_market_end {
            api_end_time // Use actual end time from API
        } else {
            eth_market_timestamp + PERIOD_DURATION // Fallback to slug + 900s
        };
        
        let solana_period_end = if let Some(api_end_time) = solana_market_end {
            api_end_time // Use actual end time from API
        } else {
            solana_market_timestamp + PERIOD_DURATION // Fallback to slug + 900s
        };
        let xrp_period_end = if let Some(api_end_time) = xrp_market_end {
            api_end_time // Use actual end time from API
        } else {
            xrp_market_timestamp + PERIOD_DURATION // Fallback to slug + 900s
        };
        
        let eth_remaining_secs = if eth_period_end > current_timestamp {
            eth_period_end - current_timestamp
        } else {
            0
        };
        let btc_remaining_secs = if btc_period_end > current_timestamp {
            btc_period_end - current_timestamp
        } else {
            0
        };
        let solana_remaining_secs = if solana_period_end > current_timestamp {
            solana_period_end - current_timestamp
        } else {
            0
        };
        let xrp_remaining_secs = if xrp_period_end > current_timestamp {
            xrp_period_end - current_timestamp
        } else {
            0
        };
        
        // When market is closed (remaining_secs = 0), order book prices are stale
        // At closure, tokens should be worth $1.00 (winner) or $0.00 (loser)
        // Skip repeated checks for closed markets to avoid spam
        let (btc_up_price, btc_down_price) = if btc_condition_id == "dummy_btc_fallback" {
            (None, None)
        } else if btc_remaining_secs == 0 {
            // Market is closed - skip repeated checks (only log once)
            (None, None)
        } else {
            // Market is still open - fetch prices from order book
            let btc_up_token_id = self.btc_up_token_id.lock().await.clone();
            let btc_down_token_id = self.btc_down_token_id.lock().await.clone();
            
            tokio::join!(
                self.fetch_token_price(&btc_up_token_id, "BTC", "Up"),
                self.fetch_token_price(&btc_down_token_id, "BTC", "Down"),
            )
        };
        
        // Fetch ETH prices (skip if disabled)
        let (eth_up_price, eth_down_price) = if eth_condition_id == "dummy_eth_fallback" {
            (None, None)
        } else if eth_remaining_secs == 0 {
            // Market is closed - skip repeated checks (only log once)
            (None, None)
        } else {
            // Market is still open - fetch prices from order book
            let eth_up_token_id = self.eth_up_token_id.lock().await.clone();
            let eth_down_token_id = self.eth_down_token_id.lock().await.clone();
            
            tokio::join!(
                self.fetch_token_price(&eth_up_token_id, "ETH", "Up"),
                self.fetch_token_price(&eth_down_token_id, "ETH", "Down"),
            )
        };
        
        // Fetch Solana prices (skip API when dummy fallback - no real market)
        let (solana_up_price, solana_down_price) = if solana_condition_id == "dummy_solana_fallback" {
            (None, None)
        } else if solana_remaining_secs == 0 {
            // Market is closed - skip repeated checks (only log once)
            (None, None)
        } else {
            // Market is still open - fetch prices from order book
            let solana_up_token_id = self.solana_up_token_id.lock().await.clone();
            let solana_down_token_id = self.solana_down_token_id.lock().await.clone();
            
            tokio::join!(
                self.fetch_token_price(&solana_up_token_id, "Solana", "Up"),
                self.fetch_token_price(&solana_down_token_id, "Solana", "Down"),
            )
        };

        // Fetch XRP prices (skip API when dummy fallback - no real market)
        let (xrp_up_price, xrp_down_price) = if xrp_condition_id == "dummy_xrp_fallback" {
            (None, None)
        } else if xrp_remaining_secs == 0 {
            // Market is closed - skip repeated checks (only log once)
            (None, None)
        } else {
            let xrp_up_token_id = self.xrp_up_token_id.lock().await.clone();
            let xrp_down_token_id = self.xrp_down_token_id.lock().await.clone();

            tokio::join!(
                self.fetch_token_price(&xrp_up_token_id, "XRP", "Up"),
                self.fetch_token_price(&xrp_down_token_id, "XRP", "Down"),
            )
        };
        
        // Format remaining time as "Xm Ys"
        let format_remaining_time = |secs: u64| -> String {
            if secs == 0 {
                "0s".to_string()
            } else {
                let minutes = secs / 60;
                let seconds = secs % 60;
                if minutes > 0 {
                    format!("{}m {}s", minutes, seconds)
                } else {
                    format!("{}s", seconds)
                }
            }
        };
        
        let eth_remaining_str = format_remaining_time(eth_remaining_secs);
        let btc_remaining_str = format_remaining_time(btc_remaining_secs);
        let solana_remaining_str = if solana_condition_id == "dummy_solana_fallback" {
            "N/A".to_string()
        } else {
            format_remaining_time(solana_remaining_secs)
        };
        let xrp_remaining_str = if xrp_condition_id == "dummy_xrp_fallback" {
            "N/A".to_string()
        } else {
            format_remaining_time(xrp_remaining_secs)
        };

        // Helper function to format price compactly (BID/ASK)
        let format_price_compact = |p: &TokenPrice| -> String {
            let bid = p.bid.unwrap_or(rust_decimal::Decimal::ZERO);
            let ask = p.ask.unwrap_or(rust_decimal::Decimal::ZERO);
            let bid_f64: f64 = bid.to_string().parse().unwrap_or(0.0);
            let ask_f64: f64 = ask.to_string().parse().unwrap_or(0.0);
            format!("${:.2}/${:.2}", bid_f64, ask_f64)
        };

        // Log prices with compact format
        let eth_up_str = eth_up_price.as_ref()
            .map(format_price_compact)
            .unwrap_or_else(|| "N/A".to_string());
        let eth_down_str = eth_down_price.as_ref()
            .map(format_price_compact)
            .unwrap_or_else(|| "N/A".to_string());
        let btc_up_str = btc_up_price.as_ref()
            .map(format_price_compact)
            .unwrap_or_else(|| "N/A".to_string());
        let btc_down_str = btc_down_price.as_ref()
            .map(format_price_compact)
            .unwrap_or_else(|| "N/A".to_string());
        let solana_up_str = solana_up_price.as_ref()
            .map(format_price_compact)
            .unwrap_or_else(|| "N/A".to_string());
        let solana_down_str = solana_down_price.as_ref()
            .map(format_price_compact)
            .unwrap_or_else(|| "N/A".to_string());
        let xrp_up_str = xrp_up_price.as_ref()
            .map(format_price_compact)
            .unwrap_or_else(|| "N/A".to_string());
        let xrp_down_str = xrp_down_price.as_ref()
            .map(format_price_compact)
            .unwrap_or_else(|| "N/A".to_string());

        // Use the minimum remaining time between ETH, BTC, Solana, and XRP markets for period tracking
        // When ETH/Solana/XRP is dummy fallback, exclude it from the min (use u64::MAX so it doesn't reduce the result)
        let eth_for_min = if eth_condition_id == "dummy_eth_fallback" {
            u64::MAX
        } else {
            eth_remaining_secs
        };
        let solana_for_min = if solana_condition_id == "dummy_solana_fallback" {
            u64::MAX
        } else {
            solana_remaining_secs
        };
        let xrp_for_min = if xrp_condition_id == "dummy_xrp_fallback" {
            u64::MAX
        } else {
            xrp_remaining_secs
        };
        let time_remaining_seconds = std::cmp::min(
            std::cmp::min(eth_for_min, btc_remaining_secs),
            std::cmp::min(solana_for_min, xrp_for_min)
        );

        // Log prices to terminal (real-time monitoring) - NOT saved to history.toml
        // Compact one-line format for easy monitoring: BTC/ETH/SOL/XRP Up/Down, single timer
        let time_remaining_str = format_remaining_time(time_remaining_seconds);
        let price_log_line = format!("📊 BTC: U{} D{} | ETH: U{} D{} | SOL: U{} D{} | XRP: U{} D{} | ⏱️  {}", 
            btc_up_str, btc_down_str,
            eth_up_str, eth_down_str,
            solana_up_str, solana_down_str,
            xrp_up_str, xrp_down_str,
            time_remaining_str);
        eprintln!("{}", price_log_line);
        
        // Always log prices to files (both simulation and production/price monitor mode)
        let timestamp = Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
        let log_entry = format!("[{}] {}\n", timestamp, price_log_line);
        
        // Write to main price monitor file (if in simulation mode)
        if self.simulation_mode {
            if let Some(file_mutex) = &self.price_monitor_file {
                let mut file = file_mutex.lock().await;
                let _ = write!(*file, "{}", log_entry);
                let _ = file.flush();
            }
        }
        
        // Always write to market-specific price files (for both simulation and price monitor mode)
        let period = btc_market_timestamp;
        
        // Write to a single price file per period (not per condition ID)
        // Use period as the key to avoid duplicates
        if period > 0 {
            let mut files = self.market_price_files.lock().await;
            // Use period as key instead of condition_id to have one file per period
            let period_key = format!("period_{}", period);
            let file_arc = files.entry(period_key).or_insert_with(|| {
                let file_name = format!("history/market_{}_prices.toml", period);
                eprintln!("📝 Creating price file: {}", file_name);
                let file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&file_name)
                    .unwrap_or_else(|e| {
                        warn!("Failed to create price file {}: {}", file_name, e);
                        std::fs::File::create("/dev/null").unwrap()
                    });
                Arc::new(tokio::sync::Mutex::new(file))
            });
            
            let mut file = file_arc.lock().await;
            let _ = write!(*file, "{}", log_entry);
            let _ = file.flush();
        }

        let eth_market_data = MarketData {
            condition_id: eth_condition_id,
            market_name: "ETH".to_string(),
            up_token: eth_up_price,
            down_token: eth_down_price,
        };

        let btc_market_data = MarketData {
            condition_id: btc_condition_id,
            market_name: "BTC".to_string(),
            up_token: btc_up_price,
            down_token: btc_down_price,
        };

        let solana_market_data = MarketData {
            condition_id: solana_condition_id.clone(),
            market_name: "Solana".to_string(),
            up_token: solana_up_price,
            down_token: solana_down_price,
        };
        let xrp_market_data = MarketData {
            condition_id: xrp_condition_id.clone(),
            market_name: "XRP".to_string(),
            up_token: xrp_up_price,
            down_token: xrp_down_price,
        };

        // Use BTC market timestamp as the primary period identifier (all should be the same)
        Ok(MarketSnapshot {
            eth_market: eth_market_data,
            btc_market: btc_market_data,
            solana_market: solana_market_data,
            xrp_market: xrp_market_data,
            timestamp: std::time::Instant::now(),
            time_remaining_seconds,
            period_timestamp: btc_market_timestamp, // Use BTC market timestamp as period identifier
        })
    }

    async fn fetch_token_price(
        &self,
        token_id: &Option<String>,
        market_name: &str,
        outcome: &str,
    ) -> Option<TokenPrice> {
        let token_id = token_id.as_ref()?;

        // Get BUY price (BID price - what we pay to buy, higher)
        // get_price(token_id, "BUY") returns the BID price (what we pay to buy)
        let buy_price = match self.api.get_price(token_id, "BUY").await {
            Ok(price) => Some(price),
            Err(e) => {
                warn!("Failed to fetch {} {} BUY price: {}", market_name, outcome, e);
                None
            }
        };

        // Get SELL price (ASK price - what we receive when selling, lower)
        // get_price(token_id, "SELL") returns the ASK price (what we receive when selling)
        let sell_price = match self.api.get_price(token_id, "SELL").await {
            Ok(price) => Some(price),
            Err(e) => {
                warn!("Failed to fetch {} {} SELL price: {}", market_name, outcome, e);
                None
            }
        };

        if buy_price.is_some() || sell_price.is_some() {
            Some(TokenPrice {
                token_id: token_id.clone(),
                bid: buy_price,  // BID = BUY price (what we pay to buy, higher)
                ask: sell_price, // ASK = SELL price (what we receive when selling, lower)
            })
        } else {
            None
        }
    }

    /// Fetch resolved prices when market is closed
    /// Returns prices based on market resolution: winner = $1.00, loser = $0.00
    async fn fetch_resolved_prices(&self, condition_id: &str) -> (Option<TokenPrice>, Option<TokenPrice>) {
        match self.api.get_market(condition_id).await {
            Ok(market) => {
                // Find which token won
                let mut up_token_id = None;
                let mut down_token_id = None;
                let mut up_winner = false;
                let mut down_winner = false;
                
                for token in &market.tokens {
                    let outcome_upper = token.outcome.to_uppercase();
                    if outcome_upper.contains("UP") || outcome_upper == "1" {
                        up_token_id = Some(token.token_id.clone());
                        up_winner = token.winner;
                    } else if outcome_upper.contains("DOWN") || outcome_upper == "0" {
                        down_token_id = Some(token.token_id.clone());
                        down_winner = token.winner;
                    }
                }
                
                // Create resolved prices: winner = $1.00, loser = $0.00
                use rust_decimal::Decimal;
                let up_price = up_token_id.map(|token_id| {
                    let resolved_price = if up_winner { Decimal::ONE } else { Decimal::ZERO };
                    crate::models::TokenPrice {
                        token_id,
                        bid: Some(resolved_price),
                        ask: Some(resolved_price),
                    }
                });
                
                let down_price = down_token_id.map(|token_id| {
                    let resolved_price = if down_winner { Decimal::ONE } else { Decimal::ZERO };
                    crate::models::TokenPrice {
                        token_id,
                        bid: Some(resolved_price),
                        ask: Some(resolved_price),
                    }
                });
                
                eprintln!("✅ Market resolved: BTC Up={}, BTC Down={}", 
                    if up_winner { "$1.00" } else { "$0.00" },
                    if down_winner { "$1.00" } else { "$0.00" });
                
                (up_price, down_price)
            }
            Err(e) => {
                warn!("Failed to fetch market resolution: {}", e);
                (None, None)
            }
        }
    }

    /// Extract timestamp from market slug (e.g., "eth-updown-15m-1767796200" -> 1767796200)
    pub fn extract_timestamp_from_slug(slug: &str) -> u64 {
        // Slug format: {asset}-updown-15m-{timestamp}
        // Try to extract the timestamp (last number after the last dash)
        if let Some(last_dash) = slug.rfind('-') {
            if let Ok(timestamp) = slug[last_dash + 1..].parse::<u64>() {
                return timestamp;
            }
        }
        // Fallback: return 0 if we can't parse
        0
    }

    /// Start monitoring markets continuously
    /// Returns a callback function that can be used to update markets when new period starts
    pub async fn start_monitoring<F, Fut>(&self, callback: F)
    where
        F: Fn(MarketSnapshot) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        eprintln!("Starting market monitoring...");
        
        loop {
            match self.fetch_market_data().await {
                Ok(snapshot) => {
                    debug!("Market snapshot updated");
                    callback(snapshot).await;
                }
                Err(e) => {
                    warn!("Error fetching market data: {}", e);
                }
            }
            
            sleep(self.check_interval).await;
        }
    }
}


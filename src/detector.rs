use crate::monitor::MarketSnapshot;
use rust_decimal::Decimal;
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::Mutex;
use log::debug;

/// Reset state for a token type after a buy-sell cycle
#[derive(Debug, Clone, PartialEq)]
enum ResetState {
    /// Ready to buy - no reset needed or reset completed
    Ready,
    /// Needs reset - price must drop below trigger_price before allowing another buy
    NeedsReset,
}

/// Detector for momentum-based trading strategy
/// Strategy: Buy BTC/ETH tokens (BTC/ETH Up/Down) when price reaches trigger_price (0.9) after min_elapsed_minutes (10 minutes)
/// Assumption: If price reaches 0.9 after 10 minutes, it's very likely to reach 1.0 by market close
/// 
/// Reset mechanism: After a successful buy-sell cycle, require price to drop below trigger_price
/// before allowing another buy. This prevents buying immediately after selling when price only dips slightly.
pub struct PriceDetector {
    trigger_price: f64, // Minimum price threshold to trigger buy (e.g., 0.9)
    max_buy_price: f64, // Maximum price to buy at (e.g., 0.95) - don't buy if price > this
    min_elapsed_minutes: u64, // Minimum minutes that must have elapsed (e.g., 10 minutes)
    min_time_remaining_seconds: u64, // Minimum seconds that must remain (e.g., 30 seconds) - don't buy if less time remains
    enable_eth_trading: bool, // Whether ETH trading is enabled
    enable_solana_trading: bool, // Whether Solana trading is enabled
    // Track which tokens we've bought in this period (key: token_id)
    current_period_bought: Arc<Mutex<std::collections::HashSet<String>>>,
    // Track last logged period to detect new markets
    last_logged_period: Arc<tokio::sync::Mutex<Option<u64>>>,
    // Track reset state per token type after successful buy-sell cycles
    // Key: TokenType, Value: ResetState
    reset_states: Arc<Mutex<HashMap<TokenType, ResetState>>>,
}

#[derive(Debug, Clone)]
pub struct BuyOpportunity {
    pub condition_id: String, // Market condition ID (BTC or ETH)
    pub token_id: String,      // Token ID for the token we're buying
    pub token_type: TokenType, // Type of token (BTC Up, BTC Down, ETH Up, ETH Down)
    pub bid_price: f64,        // BID price for the token we're buying
    pub period_timestamp: u64,
    pub time_remaining_seconds: u64,
    pub time_elapsed_seconds: u64, // How many seconds have elapsed in this period
    pub use_market_order: bool, // If true, use market order; if false, use limit order
    pub investment_amount_override: Option<f64>, // Optional override for investment amount (e.g., for individual hedges that need double amount)
    pub is_individual_hedge: bool, // If true, this is an individual hedge that should place a limit sell order after buy
    pub is_standard_hedge: bool, // If true, this is a standard hedge (after dual_limit_hedge_after_minutes) that should place a limit sell order at $0.98
    pub dual_limit_shares: Option<f64>, // Optional dual_limit_shares value for placing sell orders
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TokenType {
    BtcUp,
    BtcDown,
    EthUp,
    EthDown,
    SolanaUp,
    SolanaDown,
    XrpUp,
    XrpDown,
}

impl TokenType {
    pub fn display_name(&self) -> &str {
        match self {
            TokenType::BtcUp => "BTC Up",
            TokenType::BtcDown => "BTC Down",
            TokenType::EthUp => "ETH Up",
            TokenType::EthDown => "ETH Down",
            TokenType::SolanaUp => "SOL Up",
            TokenType::SolanaDown => "SOL Down",
            TokenType::XrpUp => "XRP Up",
            TokenType::XrpDown => "XRP Down",
        }
    }
    
    /// Get the opposite token type (Up <-> Down)
    pub fn opposite(&self) -> TokenType {
        match self {
            TokenType::BtcUp => TokenType::BtcDown,
            TokenType::BtcDown => TokenType::BtcUp,
            TokenType::EthUp => TokenType::EthDown,
            TokenType::EthDown => TokenType::EthUp,
            TokenType::SolanaUp => TokenType::SolanaDown,
            TokenType::SolanaDown => TokenType::SolanaUp,
            TokenType::XrpUp => TokenType::XrpDown,
            TokenType::XrpDown => TokenType::XrpUp,
        }
    }
}

impl PriceDetector {
    pub fn new(trigger_price: f64, max_buy_price: f64, min_elapsed_minutes: u64, min_time_remaining_seconds: u64, enable_eth_trading: bool, enable_solana_trading: bool) -> Self {
        Self {
            trigger_price,
            max_buy_price,
            min_elapsed_minutes,
            min_time_remaining_seconds,
            enable_eth_trading,
            enable_solana_trading,
            current_period_bought: Arc::new(Mutex::new(std::collections::HashSet::new())),
            last_logged_period: Arc::new(tokio::sync::Mutex::new(None)),
            reset_states: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// Check a single token for opportunity
    async fn check_token(
        &self,
        token: &crate::models::TokenPrice,
        token_type: TokenType,
        condition_id: &str,
        snapshot: &MarketSnapshot,
        time_elapsed_seconds: u64,
        min_elapsed_seconds: u64,
    ) -> Option<BuyOpportunity> {
        // Use BID price (what we pay to buy) - return None if bid price is missing
        let bid_price = match token.bid {
            Some(bid) => decimal_to_f64(bid),
            None => {
                if time_elapsed_seconds >= min_elapsed_seconds - 60 {
                    crate::log_println!("⚠️  {}: No BID price available, skipping", token_type.display_name());
                }
            return None;
            },
        };

        let time_elapsed_minutes = time_elapsed_seconds / 60;
        let time_remaining_minutes = snapshot.time_remaining_seconds / 60;

        // Check reset state: after a successful buy-sell cycle, require price to drop below trigger_price
        // before allowing another buy. This prevents buying immediately after selling when price only dips slightly.
        let mut reset_states = self.reset_states.lock().await;
        let reset_state = reset_states.get(&token_type).cloned().unwrap_or(ResetState::Ready);
        
        match reset_state {
            ResetState::NeedsReset => {
                // After a successful sell, we need price to drop below trigger_price to reset
                if bid_price < self.trigger_price {
                    // Price dropped below trigger - reset completed, allow buying again
                    reset_states.insert(token_type.clone(), ResetState::Ready);
                    drop(reset_states);
                    if time_elapsed_seconds >= min_elapsed_seconds {
                        eprintln!("✅ {}: Reset completed - BID=${:.6} < trigger=${:.6}, ready for next buy", 
                            token_type.display_name(), bid_price, self.trigger_price);
                    }
                    // Still return None here - we need price to go back up >= trigger_price to buy
                    return None;
                } else {
                    // Price hasn't dropped below trigger yet - still needs reset
                    drop(reset_states);
                    if time_elapsed_seconds >= min_elapsed_seconds {
                        eprintln!("⏸️  {}: Needs reset - BID=${:.6} >= trigger=${:.6}, waiting for price to drop below trigger first", 
                            token_type.display_name(), bid_price, self.trigger_price);
                    }
            return None;
        }
            }
            ResetState::Ready => {
                // Ready to buy - no reset needed
                drop(reset_states);
            }
        }

        // Log when price is close to trigger (within 0.05) or past buy window - helps debug why buys aren't triggering
        let price_diff = bid_price - self.trigger_price;
        if time_elapsed_seconds >= min_elapsed_seconds.saturating_sub(60) || price_diff.abs() < 0.05 {
            eprintln!("🔍 {}: BID=${:.6} (trigger=${:.2}, diff=${:.3}), range: ${:.2}-${:.2}, elapsed={}m{}s (need {}m), remaining={}m{}s",
                token_type.display_name(), bid_price, self.trigger_price, price_diff,
                self.trigger_price, self.max_buy_price,
                time_elapsed_minutes, time_elapsed_seconds % 60, self.min_elapsed_minutes,
                time_remaining_minutes, snapshot.time_remaining_seconds % 60);
        }

        // Check if enough time has elapsed (at least min_elapsed_minutes)
        if time_elapsed_seconds < min_elapsed_seconds {
            // Log when close to buy window or when price is near trigger - helps debug why buys aren't triggering
            let time_remaining_until_window = min_elapsed_seconds - time_elapsed_seconds;
            let price_diff = bid_price - self.trigger_price;
            if time_elapsed_seconds >= min_elapsed_seconds - 60 || price_diff.abs() < 0.05 {
                eprintln!("⏸️  {}: Time not elapsed yet: {}m{}s elapsed < {}m required (need {}s more) | BID=${:.6}",
                    token_type.display_name(), time_elapsed_minutes, time_elapsed_seconds % 60, 
                    self.min_elapsed_minutes, time_remaining_until_window, bid_price);
            }
            return None;
        }

        // Buy when price is between trigger_price (min) and max_buy_price (max)
        // Example: buy when 0.87 <= bid_price <= 0.95
        if bid_price < self.trigger_price {
            // Log when close to trigger or past buy window - helps debug why buys aren't triggering
            let price_diff = self.trigger_price - bid_price;
            if time_elapsed_seconds >= min_elapsed_seconds || price_diff < 0.05 {
                eprintln!("⏸️  {}: Price too low: BID=${:.6} < ${:.6} (trigger) - need ${:.3} more",
                    token_type.display_name(), bid_price, self.trigger_price, price_diff);
            }
            return None; // Price too low, wait for it to reach trigger_price (0.87)
        }
        
        if bid_price > self.max_buy_price {
            // Only log when close to buy window (reduce noise)
            if time_elapsed_seconds >= min_elapsed_seconds {
                debug!("{}: Price too high: ${:.6} > ${:.6} (max)", 
                    token_type.display_name(), bid_price, self.max_buy_price);
            }
            return None; // Price too high (> 0.95), skip buying and wait for price to drop
        }

        // Check if there's enough time remaining (at least min_time_remaining_seconds)
        // Don't buy if market is closing soon - too risky
        if snapshot.time_remaining_seconds < self.min_time_remaining_seconds {
            // Log prominently when skipping buy due to insufficient time remaining
            if time_elapsed_seconds >= min_elapsed_seconds {
                eprintln!("⏸️  {}: SKIPPING BUY - insufficient time remaining: {}s < {}s (minimum required)", 
                    token_type.display_name(), snapshot.time_remaining_seconds, self.min_time_remaining_seconds);
                eprintln!("   💡 Price conditions met, but market closing too soon - too risky to buy");
            }
            return None; // Too little time remaining, skip buying
        }

        // Price is in valid range! (trigger_price <= bid_price <= max_buy_price)
        // And there's enough time remaining (>= 30 seconds)
        // This should trigger a buy
        let expected_profit_at_1_0 = 1.0 - bid_price; // Profit if token reaches $1.0

        // Log momentum opportunity in compact one-line format
        eprintln!("🎯 {} BUY: BID=${:.3} | Elapsed: {}m | Remaining: {}s | Profit@$1.0: ${:.3}", 
            token_type.display_name(), bid_price, 
            time_elapsed_minutes, snapshot.time_remaining_seconds, expected_profit_at_1_0);

        Some(BuyOpportunity {
            condition_id: condition_id.to_string(),
            token_id: token.token_id.clone(),
            token_type,
            bid_price,
            period_timestamp: snapshot.period_timestamp,
            time_remaining_seconds: snapshot.time_remaining_seconds,
            time_elapsed_seconds,
            use_market_order: false, // Regular detect_opportunities always uses market orders
            investment_amount_override: None,
            is_individual_hedge: false,
            is_standard_hedge: false,
            dual_limit_shares: None,
        })
    }

    /// Detect momentum opportunities: Check BTC and ETH tokens (BTC/ETH Up/Down) for price >= trigger_price after min_elapsed_minutes
    /// Strategy: Buy BTC/ETH token when price reaches 0.9 after 10 minutes have elapsed
    /// Returns all matching opportunities so we can buy both ETH Down and BTC Down (and Up) when multiple qualify
    pub async fn detect_opportunities(&self, snapshot: &MarketSnapshot) -> Vec<BuyOpportunity> {
        let mut opportunities = Vec::new();

        // Reject expired markets (time_remaining_seconds <= 0)
        if snapshot.time_remaining_seconds == 0 {
            debug!("Market expired (time_remaining=0), skipping opportunity detection");
            return opportunities;
        }

        // Calculate time elapsed (15 minutes = 900 seconds total)
        const PERIOD_DURATION: u64 = 900; // 15 minutes in seconds
        let time_elapsed_seconds = PERIOD_DURATION - snapshot.time_remaining_seconds;
        let min_elapsed_seconds = self.min_elapsed_minutes * 60;
        
        // Log when we detect a new market period (to show we're monitoring each market)
        let time_elapsed_minutes = time_elapsed_seconds / 60;
        let time_remaining_minutes = snapshot.time_remaining_seconds / 60;
        
        {
            let mut last_period = self.last_logged_period.lock().await;
            if last_period.is_none() || last_period.unwrap() != snapshot.period_timestamp {
                eprintln!("🔄 New market (period: {}): {}m elapsed, {}m remaining (buy window: after {}m)", 
                    snapshot.period_timestamp, time_elapsed_minutes, time_remaining_minutes, self.min_elapsed_minutes);
                *last_period = Some(snapshot.period_timestamp);
            }
        }
        
        if time_elapsed_seconds < min_elapsed_seconds && time_elapsed_minutes > 0 && time_elapsed_minutes % 2 == 0 && time_elapsed_seconds % 60 < 2 {
            eprintln!("⏱️  Monitoring (period: {}): {}m elapsed, {}m remaining (buy window: after {}m)", 
                snapshot.period_timestamp, time_elapsed_minutes, time_remaining_minutes, self.min_elapsed_minutes);
        }
        
        debug!("detect_opportunity: time_elapsed={}s ({}m), remaining={}s, min_required={}s ({}m)", 
            time_elapsed_seconds, time_elapsed_seconds / 60, 
            snapshot.time_remaining_seconds, min_elapsed_seconds, self.min_elapsed_minutes);

        // Check BTC Up
        if let Some(btc_up) = snapshot.btc_market.up_token.as_ref() {
            if let Some(opp) = self.check_token(btc_up, TokenType::BtcUp, &snapshot.btc_market.condition_id, snapshot, time_elapsed_seconds, min_elapsed_seconds).await {
                opportunities.push(opp);
            }
        }
        // Check BTC Down
        if let Some(btc_down) = snapshot.btc_market.down_token.as_ref() {
            if let Some(opp) = self.check_token(btc_down, TokenType::BtcDown, &snapshot.btc_market.condition_id, snapshot, time_elapsed_seconds, min_elapsed_seconds).await {
                opportunities.push(opp);
            }
        }
        // Check ETH Up (only if ETH trading is enabled)
        if self.enable_eth_trading {
            if let Some(eth_up) = snapshot.eth_market.up_token.as_ref() {
                if let Some(opp) = self.check_token(eth_up, TokenType::EthUp, &snapshot.eth_market.condition_id, snapshot, time_elapsed_seconds, min_elapsed_seconds).await {
                    opportunities.push(opp);
                }
            }
            // Check ETH Down
            if let Some(eth_down) = snapshot.eth_market.down_token.as_ref() {
                if let Some(opp) = self.check_token(eth_down, TokenType::EthDown, &snapshot.eth_market.condition_id, snapshot, time_elapsed_seconds, min_elapsed_seconds).await {
                    opportunities.push(opp);
                }
            }
        }
        // Check Solana Up (only if Solana trading is enabled)
        if self.enable_solana_trading {
            if let Some(solana_up) = snapshot.solana_market.up_token.as_ref() {
                if let Some(opp) = self.check_token(solana_up, TokenType::SolanaUp, &snapshot.solana_market.condition_id, snapshot, time_elapsed_seconds, min_elapsed_seconds).await {
                    opportunities.push(opp);
                }
            }
            // Check Solana Down
            if let Some(solana_down) = snapshot.solana_market.down_token.as_ref() {
                if let Some(opp) = self.check_token(solana_down, TokenType::SolanaDown, &snapshot.solana_market.condition_id, snapshot, time_elapsed_seconds, min_elapsed_seconds).await {
                    opportunities.push(opp);
                }
            }
        }

        opportunities
    }

    /// Detect limit order opportunities: At min_elapsed_minutes, return both Up and Down tokens for limit buy orders
    /// Strategy: Place limit buy orders for both tokens when min_elapsed_minutes have passed
    /// This ignores price checks and max_buy_price/min_time_remaining_seconds
    pub async fn detect_limit_order_opportunities(&self, snapshot: &MarketSnapshot) -> Vec<BuyOpportunity> {
        let mut opportunities = Vec::new();

        if snapshot.time_remaining_seconds == 0 {
            debug!("Market expired (time_remaining=0), skipping limit order detection");
            return opportunities;
        }

        const PERIOD_DURATION: u64 = 900;
        let time_elapsed_seconds = PERIOD_DURATION - snapshot.time_remaining_seconds;
        let min_elapsed_seconds = self.min_elapsed_minutes * 60;
        let time_elapsed_minutes = time_elapsed_seconds / 60;

        // Only trigger once when min_elapsed_minutes is reached
        if time_elapsed_seconds < min_elapsed_seconds {
            return opportunities;
        }

        // Check if we've already placed orders for this period (avoid placing multiple times)
        let mut bought = self.current_period_bought.lock().await;
        let period_key = format!("{}_limit_orders", snapshot.period_timestamp);
        if bought.contains(&period_key) {
            drop(bought);
            return opportunities;
        }
        bought.insert(period_key.clone());
        drop(bought);

        eprintln!("🎯 LIMIT ORDER WINDOW: {}m elapsed - Checking prices for Up/Down tokens", time_elapsed_minutes);
        eprintln!("   Using MARKET orders only (limit buy orders removed - want to buy at exact market price)");

        // Always use market orders for buying (no limit buy orders)
        // Limit orders would fill immediately if ask < limit price, which is not desired
        // We want to buy at the exact current market price
        
        // BTC Up
        if let Some(btc_up) = snapshot.btc_market.up_token.as_ref() {
            let ask_f64 = btc_up.ask
                .and_then(|a| f64::try_from(a).ok())
                .unwrap_or(0.0);
            
            eprintln!("   BTC Up: ask=${:.6} -> MARKET order at ${:.6}", ask_f64, ask_f64);
            
            opportunities.push(BuyOpportunity {
                condition_id: snapshot.btc_market.condition_id.clone(),
                token_id: btc_up.token_id.clone(),
                token_type: TokenType::BtcUp,
                bid_price: ask_f64,
                period_timestamp: snapshot.period_timestamp,
                time_remaining_seconds: snapshot.time_remaining_seconds,
                time_elapsed_seconds,
                use_market_order: true, // Always use market order
                investment_amount_override: None,
                is_individual_hedge: false,
                is_standard_hedge: false,
                dual_limit_shares: None,
            });
        }

        // BTC Down
        if let Some(btc_down) = snapshot.btc_market.down_token.as_ref() {
            let ask_f64 = btc_down.ask
                .and_then(|a| f64::try_from(a).ok())
                .unwrap_or(0.0);
            
            eprintln!("   BTC Down: ask=${:.6} -> MARKET order at ${:.6}", ask_f64, ask_f64);
            
            opportunities.push(BuyOpportunity {
                condition_id: snapshot.btc_market.condition_id.clone(),
                token_id: btc_down.token_id.clone(),
                token_type: TokenType::BtcDown,
                bid_price: ask_f64,
                period_timestamp: snapshot.period_timestamp,
                time_remaining_seconds: snapshot.time_remaining_seconds,
                time_elapsed_seconds,
                use_market_order: true, // Always use market order
                investment_amount_override: None,
                is_individual_hedge: false,
                is_standard_hedge: false,
                dual_limit_shares: None,
            });
        }

        // ETH Up/Down (if enabled)
        if self.enable_eth_trading {
            if let Some(eth_up) = snapshot.eth_market.up_token.as_ref() {
                let ask_f64 = eth_up.ask
                    .and_then(|a| f64::try_from(a).ok())
                    .unwrap_or(0.0);
                
                eprintln!("   ETH Up: ask=${:.6} -> MARKET order at ${:.6}", ask_f64, ask_f64);
                
                opportunities.push(BuyOpportunity {
                    condition_id: snapshot.eth_market.condition_id.clone(),
                    token_id: eth_up.token_id.clone(),
                    token_type: TokenType::EthUp,
                    bid_price: ask_f64,
                    period_timestamp: snapshot.period_timestamp,
                    time_remaining_seconds: snapshot.time_remaining_seconds,
                    time_elapsed_seconds,
                    use_market_order: true, // Always use market order
                    investment_amount_override: None,
                    is_individual_hedge: false,
                    is_standard_hedge: false,
                    dual_limit_shares: None,
                });
            }
            if let Some(eth_down) = snapshot.eth_market.down_token.as_ref() {
                let ask_f64 = eth_down.ask
                    .and_then(|a| f64::try_from(a).ok())
                    .unwrap_or(0.0);
                
                eprintln!("   ETH Down: ask=${:.6} -> MARKET order at ${:.6}", ask_f64, ask_f64);
                
                opportunities.push(BuyOpportunity {
                    condition_id: snapshot.eth_market.condition_id.clone(),
                    token_id: eth_down.token_id.clone(),
                    token_type: TokenType::EthDown,
                    bid_price: ask_f64,
                    period_timestamp: snapshot.period_timestamp,
                    time_remaining_seconds: snapshot.time_remaining_seconds,
                    time_elapsed_seconds,
                    use_market_order: true, // Always use market order
                    investment_amount_override: None,
                    is_individual_hedge: false,
                    is_standard_hedge: false,
                    dual_limit_shares: None,
                });
            }
        }

        // Solana Up/Down (if enabled)
        if self.enable_solana_trading {
            if let Some(solana_up) = snapshot.solana_market.up_token.as_ref() {
                let ask_f64 = solana_up.ask
                    .and_then(|a| f64::try_from(a).ok())
                    .unwrap_or(0.0);
                
                eprintln!("   SOL Up: ask=${:.6} -> MARKET order at ${:.6}", ask_f64, ask_f64);
                
                opportunities.push(BuyOpportunity {
                    condition_id: snapshot.solana_market.condition_id.clone(),
                    token_id: solana_up.token_id.clone(),
                    token_type: TokenType::SolanaUp,
                    bid_price: ask_f64,
                    period_timestamp: snapshot.period_timestamp,
                    time_remaining_seconds: snapshot.time_remaining_seconds,
                    time_elapsed_seconds,
                    use_market_order: true, // Always use market order
                    investment_amount_override: None,
                    is_individual_hedge: false,
                    is_standard_hedge: false,
                    dual_limit_shares: None,
                });
            }
            if let Some(solana_down) = snapshot.solana_market.down_token.as_ref() {
                let ask_f64 = solana_down.ask
                    .and_then(|a| f64::try_from(a).ok())
                    .unwrap_or(0.0);
                
                eprintln!("   SOL Down: ask=${:.6} -> MARKET order at ${:.6}", ask_f64, ask_f64);
                
                opportunities.push(BuyOpportunity {
                    condition_id: snapshot.solana_market.condition_id.clone(),
                    token_id: solana_down.token_id.clone(),
                    token_type: TokenType::SolanaDown,
                    bid_price: ask_f64,
                    period_timestamp: snapshot.period_timestamp,
                    time_remaining_seconds: snapshot.time_remaining_seconds,
                    time_elapsed_seconds,
                    use_market_order: true, // Always use market order
                    investment_amount_override: None,
                    is_individual_hedge: false,
                    is_standard_hedge: false,
                    dual_limit_shares: None,
                });
            }
        }

        eprintln!("📋 Found {} limit order opportunities", opportunities.len());
        opportunities
    }

    /// Mark that we bought a specific token in this period
    pub async fn mark_token_bought(&self, token_id: String) {
        let mut bought = self.current_period_bought.lock().await;
        bought.insert(token_id);
    }

    /// Reset when new period starts
    pub async fn reset_period(&self) {
        let mut bought = self.current_period_bought.lock().await;
        bought.clear();
        // Also reset reset states for new period
        let mut reset_states = self.reset_states.lock().await;
        reset_states.clear();
    }

    /// Clear limit order tracking for a specific period to allow re-entry after a sell
    /// This is called when a limit sell order fills, allowing the bot to place new limit buy orders
    pub async fn clear_limit_order_tracking(&self, period_timestamp: u64) {
        let mut bought = self.current_period_bought.lock().await;
        let period_key = format!("{}_limit_orders", period_timestamp);
        bought.remove(&period_key);
        eprintln!("🔄 Cleared limit order tracking for period {} - can re-enter if conditions met", period_timestamp);
    }

    /// Mark that a buy-sell cycle completed for a token type
    /// After this, the price must drop below trigger_price before allowing another buy
    pub async fn mark_cycle_completed(&self, token_type: TokenType) {
        let mut reset_states = self.reset_states.lock().await;
        reset_states.insert(token_type.clone(), ResetState::NeedsReset);
        crate::log_println!("🔄 {}: Buy-sell cycle completed. Will require price to drop below ${:.6} before next buy", 
            token_type.display_name(), self.trigger_price);
    }
}

// Helper function for Decimal to f64 conversion
fn decimal_to_f64(d: Decimal) -> f64 {
    d.to_string().parse().unwrap_or(0.0)
}

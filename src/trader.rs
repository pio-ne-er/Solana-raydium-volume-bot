use crate::api::PolymarketApi;
use crate::models::*;
use crate::detector::{BuyOpportunity, TokenType, PriceDetector};
use crate::config::TradingConfig;
use crate::monitor::MarketSnapshot;
use crate::simulation::SimulationTracker;
use anyhow::Result;
use log::{warn, debug};
use std::sync::Arc;
use tokio::sync::Mutex;
use std::collections::HashMap;
use std::str::FromStr;


pub struct Trader {
    api: Arc<PolymarketApi>,
    config: TradingConfig,
    simulation_mode: bool,
    total_profit: Arc<Mutex<f64>>,
    trades_executed: Arc<Mutex<u64>>,
    pending_trades: Arc<Mutex<HashMap<String, PendingTrade>>>, // Key: period_timestamp
    detector: Option<Arc<PriceDetector>>, // Optional detector reference for cycle tracking
    simulation_tracker: Option<Arc<SimulationTracker>>, // Simulation tracker for PnL and position tracking
}

impl Trader {
    pub fn get_simulation_tracker(&self) -> Option<Arc<SimulationTracker>> {
        self.simulation_tracker.clone()
    }

    pub fn new(api: Arc<PolymarketApi>, config: TradingConfig, simulation_mode: bool, detector: Option<Arc<PriceDetector>>) -> Result<Self> {
        let simulation_tracker = if simulation_mode {
            Some(Arc::new(SimulationTracker::new("simulation.toml")?))
        } else {
            None
        };
        
        Ok(Self {
            api,
            config,
            simulation_mode,
            total_profit: Arc::new(Mutex::new(0.0)),
            trades_executed: Arc::new(Mutex::new(0)),
            pending_trades: Arc::new(Mutex::new(HashMap::new())),
            detector,
            simulation_tracker,
        })
    }

    /// Helper function to place hedge sell orders with retry logic
    /// This is called from a background task after waiting 7 seconds
    async fn place_hedge_sell_orders_with_retry(
        api: &PolymarketApi,
        pending_trades: &Arc<tokio::sync::Mutex<std::collections::HashMap<String, PendingTrade>>>,
        trade_key: &str,
        token_id: &str,
        token_type: &crate::detector::TokenType,
        period_timestamp: u64,
        sell_size: f64,
        is_standard_hedge: bool,
    ) {
        const MAX_RETRIES: u32 = 3;
        const RETRY_DELAY_SECS: u64 = 2;
        
        let hedge_type = if is_standard_hedge { "STANDARD HEDGE" } else { "INDIVIDUAL HEDGE" };
        
        crate::log_println!("═══════════════════════════════════════════════════════════");
        crate::log_println!("📤 PLACING 2 LIMIT SELL ORDERS FOR {}", hedge_type);
        crate::log_println!("═══════════════════════════════════════════════════════════");
        crate::log_println!("📊 Order Details:");
        crate::log_println!("   Token: {}", token_type.display_name());
        crate::log_println!("   Token ID: {}", token_id);
        crate::log_println!("   Order 1: ${:.2} at $0.93, Size: {:.6} shares", sell_size * 0.93, sell_size);
        crate::log_println!("   Order 2: ${:.2} at $0.98, Size: {:.6} shares", sell_size * 0.98, sell_size);
        crate::log_println!("");
        
        use crate::models::OrderRequest;
        
        // Place first order at $0.93 with retry
        let mut order1_placed = false;
        for attempt in 1..=MAX_RETRIES {
            let sell_order_1 = OrderRequest {
                token_id: token_id.to_string(),
                side: "SELL".to_string(),
                size: format!("{:.2}", sell_size),
                price: format!("{:.2}", 0.93),
                order_type: "LIMIT".to_string(),
            };
            
            match api.place_order(&sell_order_1).await {
                Ok(response) => {
                    crate::log_println!("   ✅ LIMIT SELL ORDER 1 PLACED FOR {} (attempt {})", hedge_type, attempt);
                    crate::log_println!("      Token: {}", token_type.display_name());
                    crate::log_println!("      Order ID: {:?}", response.order_id);
                    crate::log_println!("      Limit Price: $0.93");
                    crate::log_println!("      Size: {:.6} shares", sell_size);
                    
                    let order_id_str = response.order_id.as_ref()
                        .map(|id| format!("{:?}", id))
                        .unwrap_or_else(|| "N/A".to_string());
                    let sell_event = format!(
                        "LIMIT SELL ORDER ({}) | Market: {} | Period: {} | Token: {} | Limit Price: $0.93 | Size: {:.6} | Order ID: {}",
                        hedge_type,
                        token_type.display_name(),
                        period_timestamp,
                        &token_id[..16],
                        sell_size,
                        order_id_str
                    );
                    crate::log_trading_event(&sell_event);
                    order1_placed = true;
                    break;
                }
                Err(e) => {
                    if attempt < MAX_RETRIES {
                        crate::log_println!("   ⚠️  Failed to place limit sell order 1 (attempt {}): {} - retrying in {} seconds...", attempt, e, RETRY_DELAY_SECS);
                        tokio::time::sleep(tokio::time::Duration::from_secs(RETRY_DELAY_SECS)).await;
                    } else {
                        eprintln!("   ❌ FAILED TO PLACE LIMIT SELL ORDER 1 FOR {} after {} attempts: {}", hedge_type, MAX_RETRIES, e);
                        warn!("Failed to place limit sell order 1 for {} after {} attempts: {}", hedge_type, MAX_RETRIES, e);
                    }
                }
            }
        }
        
        // Place second order at $0.98 with retry
        let mut order2_placed = false;
        for attempt in 1..=MAX_RETRIES {
            let sell_order_2 = OrderRequest {
                token_id: token_id.to_string(),
                side: "SELL".to_string(),
                size: format!("{:.2}", sell_size),
                price: format!("{:.2}", 0.98),
                order_type: "LIMIT".to_string(),
            };
            
            match api.place_order(&sell_order_2).await {
                Ok(response) => {
                    crate::log_println!("   ✅ LIMIT SELL ORDER 2 PLACED FOR {} (attempt {})", hedge_type, attempt);
                    crate::log_println!("      Token: {}", token_type.display_name());
                    crate::log_println!("      Order ID: {:?}", response.order_id);
                    crate::log_println!("      Limit Price: $0.98");
                    crate::log_println!("      Size: {:.6} shares", sell_size);
                    
                    let order_id_str = response.order_id.as_ref()
                        .map(|id| format!("{:?}", id))
                        .unwrap_or_else(|| "N/A".to_string());
                    let sell_event = format!(
                        "LIMIT SELL ORDER ({}) | Market: {} | Period: {} | Token: {} | Limit Price: $0.98 | Size: {:.6} | Order ID: {}",
                        hedge_type,
                        token_type.display_name(),
                        period_timestamp,
                        &token_id[..16],
                        sell_size,
                        order_id_str
                    );
                    crate::log_trading_event(&sell_event);
                    order2_placed = true;
                    break;
                }
                Err(e) => {
                    if attempt < MAX_RETRIES {
                        crate::log_println!("   ⚠️  Failed to place limit sell order 2 (attempt {}): {} - retrying in {} seconds...", attempt, e, RETRY_DELAY_SECS);
                        tokio::time::sleep(tokio::time::Duration::from_secs(RETRY_DELAY_SECS)).await;
                    } else {
                        eprintln!("   ❌ FAILED TO PLACE LIMIT SELL ORDER 2 FOR {} after {} attempts: {}", hedge_type, MAX_RETRIES, e);
                        warn!("Failed to place limit sell order 2 for {} after {} attempts: {}", hedge_type, MAX_RETRIES, e);
                    }
                }
            }
        }
        
        // Mark that sell orders have been placed (if at least one succeeded)
        if order1_placed || order2_placed {
            let mut pending = pending_trades.lock().await;
            if let Some(t) = pending.get_mut(trade_key) {
                t.limit_sell_orders_placed = true;
            }
        }
    }

    /// Get the opposite token ID for a given token type and condition ID
    /// Returns the token ID of the opposite token (Up <-> Down)
    async fn get_opposite_token_id(&self, token_type: &TokenType, condition_id: &str) -> Result<String> {
        // Get market details to find the opposite token
        let market_details = self.api.get_market(condition_id).await?;
        
        let opposite_type = token_type.opposite();
        let target_outcome = match opposite_type {
            TokenType::BtcUp | TokenType::EthUp | TokenType::SolanaUp | TokenType::XrpUp => "UP",
            TokenType::BtcDown | TokenType::EthDown | TokenType::SolanaDown | TokenType::XrpDown => "DOWN",
        };
        
        // Find the opposite token in the market
        for token in &market_details.tokens {
            let outcome_upper = token.outcome.to_uppercase();
            if outcome_upper.contains(target_outcome) || 
               (target_outcome == "UP" && outcome_upper == "1") ||
               (target_outcome == "DOWN" && outcome_upper == "0") {
                return Ok(token.token_id.clone());
            }
        }
        
        anyhow::bail!("Could not find opposite token for {} in market {}", token_type.display_name(), condition_id)
    }
    
    /// Returns true if we have an unsold position of the same type (BTC or ETH) in this period
    /// Note: 
    /// - Trades with failed redemptions (redemption_abandoned = true) don't block new positions
    /// - Only checks trades from the SAME period (different periods don't block each other)
    /// - BTC positions don't block ETH buys, and vice versa
    pub async fn has_active_position(&self, period_timestamp: u64, token_type: crate::detector::TokenType) -> bool {
        let pending = self.pending_trades.lock().await;
        for (_, trade) in pending.iter() {
            // Only count as active if:
            // 1. Same period (different periods don't block each other)
            // 2. Same token type (BTC vs ETH are separate)
            // 3. Not sold
            // 4. Not abandoned due to failed redemptions
            if trade.market_timestamp == period_timestamp 
                && trade.token_type == token_type
                && !trade.sold 
                && !trade.redemption_abandoned {
                return true;
            }
        }
        false
    }
    
    /// Clean up old abandoned trades (trades from previous periods that failed redemption)
    /// This prevents the pending_trades list from growing indefinitely
    pub async fn cleanup_old_abandoned_trades(&self, current_period_timestamp: u64) {
        let mut pending = self.pending_trades.lock().await;
        let mut to_remove = Vec::new();
        
        for (key, trade) in pending.iter() {
            // Remove trades that are:
            // 1. From a different (older) period AND
            // 2. Abandoned (redemption failed too many times)
            if trade.market_timestamp < current_period_timestamp && trade.redemption_abandoned {
                let old_period = trade.market_timestamp;
                to_remove.push((key.clone(), old_period));
            }
        }
        
        for (key, old_period) in to_remove {
            pending.remove(&key);
            crate::log_println!("🧹 Cleaned up old abandoned trade from period {} (redemption failed repeatedly - won't block new positions)", old_period);
        }
        
        drop(pending);
    }

    /// Get a pending limit trade (keyed by `"{period}_{token_id}_limit"`)
    pub async fn get_pending_limit_trade(&self, period_timestamp: u64, token_id: &str) -> Option<PendingTrade> {
        let key = format!("{}_{}_limit", period_timestamp, token_id);
        let pending = self.pending_trades.lock().await;
        pending.get(&key).cloned()
    }

    /// Cancel a pending LIMIT BUY order for the given token/period and remove it from tracking
    pub async fn cancel_pending_limit_buy(&self, period_timestamp: u64, token_id: &str) -> Result<()> {
        let key = format!("{}_{}_limit", period_timestamp, token_id);

        let trade = {
            let pending = self.pending_trades.lock().await;
            pending.get(&key).cloned()
        };

        let Some(trade) = trade else {
            return Ok(());
        };

        // Best-effort: if it's already confirmed, don't cancel
        if trade.buy_order_confirmed {
            return Ok(());
        }

        if self.simulation_mode {
            if let Some(tracker) = &self.simulation_tracker {
                tracker.cancel_limit_order(token_id, "BUY").await;
            }
        } else if let Some(order_id) = trade.order_id.as_deref() {
            self.api.cancel_order(order_id).await?;
        } else {
            warn!("Tried to cancel limit buy but no order_id was recorded (token_id={})", token_id);
        }

        let mut pending = self.pending_trades.lock().await;
        pending.remove(&key);
        Ok(())
    }
    
    /// Sync pending trades with actual portfolio balance
    /// Checks if tokens are still in portfolio - if balance is 0, mark as sold (already redeemed)
    /// This prevents the bot from trying to redeem already-redeemed tokens
    pub async fn sync_trades_with_portfolio(&self) -> Result<()> {
        let pending_trades: Vec<(String, PendingTrade)> = {
            let pending = self.pending_trades.lock().await;
            pending.iter()
                .map(|(key, trade)| (key.clone(), trade.clone()))
                .collect()
        };
        
        if pending_trades.is_empty() {
            return Ok(());
        }
        
        crate::log_println!("🔄 Syncing {} pending trade(s) with portfolio balance...", pending_trades.len());
        
        let mut updated_count = 0;
        let mut removed_count = 0;
        
        for (key, trade) in pending_trades {
            // Skip if already sold
            if trade.sold {
                continue;
            }
            
            // Check actual token balance
            match self.api.check_balance_allowance(&trade.token_id).await {
                Ok((balance, _)) => {
                    // Conditional tokens use 1e6 as base unit (like USDC)
                    // Convert from smallest unit to actual shares
                    let balance_decimal = balance / rust_decimal::Decimal::from(1_000_000u64);
                    let balance_f64 = f64::try_from(balance_decimal).unwrap_or(0.0);
                    
                    if balance_f64 == 0.0 {
                        // Balance is 0 - tokens were already redeemed
                        crate::log_println!("   ✅ Trade {}: Token balance is 0 - already redeemed, marking as sold", 
                            &trade.token_id[..16]);
                        
                        let mut pending = self.pending_trades.lock().await;
                        if let Some(t) = pending.get_mut(key.as_str()) {
                            t.sold = true;
                        }
                        pending.remove(&key);
                        drop(pending);
                        
                        removed_count += 1;
                    } else {
                        // Balance > 0 - update stored balance if different
                        if (balance_f64 - trade.units).abs() > 0.001 {
                            crate::log_println!("   🔄 Trade {}: Updating balance from {:.6} to {:.6} shares", 
                                &trade.token_id[..16], trade.units, balance_f64);
                            
                            let mut pending = self.pending_trades.lock().await;
                            if let Some(t) = pending.get_mut(key.as_str()) {
                                t.units = balance_f64;
                                t.confirmed_balance = Some(balance_f64);
                            }
                            drop(pending);
                            
                            updated_count += 1;
                        }
                    }
                }
                Err(e) => {
                    debug!("Failed to check balance for trade {}: {} - skipping sync for this trade", 
                        &trade.token_id[..16], e);
                }
            }
        }
        
        if updated_count > 0 || removed_count > 0 {
            crate::log_println!("✅ Portfolio sync complete: {} trade(s) updated, {} trade(s) removed (already redeemed)", 
                updated_count, removed_count);
        } else {
            crate::log_println!("✅ Portfolio sync complete: All trades match portfolio balance");
        }
        
        Ok(())
    }

    /// Mark position as closed (for stop-loss re-entry)
    pub async fn mark_position_closed(&self, period_timestamp: u64) {
        let mut pending = self.pending_trades.lock().await;
        for (_, trade) in pending.iter_mut() {
            if trade.market_timestamp == period_timestamp && !trade.sold {
                trade.sold = true;
                crate::log_println!("   ✅ Position marked as closed (period: {}) - can re-buy if price recovers", period_timestamp);
            }
        }
    }

    /// Execute buy when momentum opportunity is detected
    /// Buys any token (BTC Up/Down, ETH Up/Down) when price reaches trigger_price after 10 minutes
    pub async fn execute_buy(&self, opportunity: &BuyOpportunity) -> Result<()> {
        // Safety check: Verify time remaining is still sufficient before executing buy
        // This acts as a double-check in case market closed between detection and execution
        let min_time_remaining = self.config.min_time_remaining_seconds.unwrap_or(30);
        if opportunity.time_remaining_seconds < min_time_remaining {
            anyhow::bail!(
                "❌ SAFETY CHECK FAILED: Insufficient time remaining for buy\n\
                \n\
                Time remaining: {} seconds\n\
                Minimum required: {} seconds\n\
                \n\
                Buy was blocked to prevent risky trades near market closure.\n\
                This opportunity should not have been created - please check detector logic.",
                opportunity.time_remaining_seconds,
                min_time_remaining
            );
        }
        
        // Use investment amount override if provided (e.g., for individual hedges that need double amount)
        let fixed_amount = opportunity.investment_amount_override.unwrap_or(self.config.fixed_trade_amount);
        
        // Calculate units for the token
        let units = fixed_amount / opportunity.bid_price;
        let total_cost = units * opportunity.bid_price;
        let expected_profit_at_sell = (self.config.sell_price - opportunity.bid_price) * units;
        
        crate::log_println!("═══════════════════════════════════════════════════════════");
        crate::log_println!("💰 EXECUTING BUY ORDER");
        crate::log_println!("═══════════════════════════════════════════════════════════");
        crate::log_println!("📊 Order Details:");
        crate::log_println!("   Token Type: {}", opportunity.token_type.display_name());
        crate::log_println!("   Token ID: {}", opportunity.token_id);
        crate::log_println!("   Condition ID: {}", opportunity.condition_id);
        crate::log_println!("   Side: BUY");
        crate::log_println!("   Price: ${:.6}", opportunity.bid_price);
        crate::log_println!("   Units: {:.6}", units);
        crate::log_println!("   Total Cost: ${:.6}", total_cost);
        crate::log_println!("   Investment Amount: ${:.2}", fixed_amount);
        crate::log_println!("");
        crate::log_println!("📈 Trade Parameters:");
        crate::log_println!("   Target sell price: ${:.6}", self.config.sell_price);
        crate::log_println!("   Expected profit at sell: ${:.6}", expected_profit_at_sell);
        crate::log_println!("   Time elapsed: {}m {}s", 
              opportunity.time_elapsed_seconds / 60, opportunity.time_elapsed_seconds % 60);
        crate::log_println!("   Time remaining: {}s (minimum required: {}s)", 
              opportunity.time_remaining_seconds, min_time_remaining);
        crate::log_println!("   Period timestamp: {}", opportunity.period_timestamp);
        crate::log_println!("");
        
        if self.simulation_mode {
            crate::log_println!("🎮 SIMULATION MODE - Creating limit buy order");
            crate::log_println!("   ✅ SIMULATION: Limit buy order created:");
            crate::log_println!("      - Token Type: {}", opportunity.token_type.display_name());
            crate::log_println!("      - Token: {}", &opportunity.token_id[..16]);
            crate::log_println!("      - Units: {:.6}", units);
            crate::log_println!("      - Price: ${:.6}", opportunity.bid_price);
            crate::log_println!("      - Total: ${:.6}", total_cost);
            crate::log_println!("      - Strategy: Hold until market closure (claim $1.00 if winning, $0.00 if losing)");
            
            // In simulation mode, create a limit order that will be checked for fills
            if let Some(tracker) = &self.simulation_tracker {
                tracker.add_limit_order(
                    opportunity.token_id.clone(),
                    opportunity.token_type.clone(),
                    opportunity.condition_id.clone(),
                    opportunity.bid_price,
                    units,
                    "BUY".to_string(),
                    opportunity.period_timestamp,
                ).await;
                
                // Also track as pending trade for consistency
                // In simulation mode, we hold positions until market closure (no selling)
                let trade_key = format!("{}_{}_market", opportunity.period_timestamp, opportunity.token_id);
                let mut pending = self.pending_trades.lock().await;
                let trade = PendingTrade {
                    token_id: opportunity.token_id.clone(),
                    condition_id: opportunity.condition_id.clone(),
                    token_type: opportunity.token_type.clone(),
                    order_id: None,
                    investment_amount: fixed_amount,
                    units,
                    purchase_price: opportunity.bid_price,
                    sell_price: self.config.sell_price, // Not used in simulation - positions held until closure
                    timestamp: std::time::Instant::now(),
                    market_timestamp: opportunity.period_timestamp,
                    sold: false,
                    confirmed_balance: None,
                    buy_order_confirmed: false,
                    limit_sell_orders_placed: false, // No sell orders in simulation - hold until closure
                    no_sell: true, // Mark as no_sell since we hold until market closure
                    claim_on_closure: true, // Will claim at market closure
                    sell_attempts: 0,
                    redemption_attempts: 0,
                    redemption_abandoned: false,
                };
                pending.insert(trade_key, trade);
            }
            
            return Ok(());
        } else {
            // Place real market order
            // For BUY market orders, we pass USD value (not units)
            // The exchange will determine how many units we get at market price
            crate::log_println!("🚀 PRODUCTION MODE - Placing order on exchange");
            crate::log_println!("   Order parameters:");
            crate::log_println!("      Token Type: {}", opportunity.token_type.display_name());
            crate::log_println!("      Token ID: {}", opportunity.token_id);
            crate::log_println!("      Side: BUY");
            crate::log_println!("      Amount: ${:.6} (market order - units determined by market price)", fixed_amount);
            crate::log_println!("      Type: FOK (Fill-or-Kill)");
            
            match self.api.place_market_order(
                &opportunity.token_id,
                fixed_amount,  // USD value for BUY market orders
                "BUY",
                Some("FOK"),
            ).await {
                Ok(response) => {
                    crate::log_println!("   ✅ ORDER PLACED SUCCESSFULLY");
                    crate::log_println!("      Order ID: {:?}", response.order_id);
                    crate::log_println!("      Status: {}", response.status);
                    if let Some(msg) = &response.message {
                        crate::log_println!("      Message: {}", msg);
                    }
                    
                    // For hedge trades, spawn background task to wait 7 seconds then place limit sell orders
                    // This is non-blocking so monitoring continues
                    let is_individual_hedge = opportunity.is_individual_hedge;
                    let is_standard_hedge = opportunity.is_standard_hedge;
                    let is_hedge_trade = is_individual_hedge || is_standard_hedge;
                    
                    // Use expected units for hedge trades (no balance checking)
                    let balance_f64 = if is_hedge_trade {
                        units // Use expected units
                    } else {
                        // For non-hedge trades, still check balance (backward compatibility)
                        let mut balance_f64 = 0.0;
                        match self.api.check_balance_allowance(&opportunity.token_id).await {
                            Ok((balance, _)) => {
                                let balance_decimal = balance / rust_decimal::Decimal::from(1_000_000u64);
                                balance_f64 = f64::try_from(balance_decimal).unwrap_or(0.0);
                            }
                            Err(_) => {
                                balance_f64 = units; // Fallback to expected
                            }
                        }
                        balance_f64
                    };
                    
                    let balance_confirmed = is_hedge_trade || balance_f64 > 0.0;
                    
                    crate::log_println!("   ✅ BUY ORDER CONFIRMED");
                    crate::log_println!("      Expected Units: {:.6} shares", units);
                    if is_hedge_trade {
                        crate::log_println!("      Using expected units for limit sell order placement");
                    } else {
                        crate::log_println!("      Portfolio Balance: {:.6} shares", balance_f64);
                    }
                    
                    // Log successful buy order to history.toml
                    let market_name = opportunity.token_type.display_name();
                    let order_id_str = response.order_id.as_ref()
                        .map(|id| format!("{:?}", id))
                        .unwrap_or_else(|| "N/A".to_string());
                    
                    if balance_confirmed {
                        // Balance of portfolio is confirmed exactly - use this balance_f64 for all downstream logic
                        crate::log_println!("      ✅ Balance confirmed - tokens received successfully");
                        crate::log_println!("      📊 Confirmed portfolio balance: {:.6} shares", balance_f64);
                        
                        // Track the trade with confirmed balance
                        // For dual limit bot: 
                        // - If is_individual_hedge is true, this is an individual hedge that should place a limit sell order at config.sell_price
                        // - If is_standard_hedge is true, this is a standard hedge (after dual_limit_hedge_after_minutes) that should place a limit sell order at $0.98
                        // - If use_market_order is true but neither is_individual_hedge nor is_standard_hedge, this is a multi-market hedge that should be held until closure
                        let is_individual_hedge = opportunity.is_individual_hedge;
                        let is_standard_hedge = opportunity.is_standard_hedge;
                        let is_dual_limit_hedge = opportunity.use_market_order && !is_individual_hedge && !is_standard_hedge;
                        let trade = PendingTrade {
                            token_id: opportunity.token_id.clone(),
                            condition_id: opportunity.condition_id.clone(),
                            token_type: opportunity.token_type.clone(),
                            order_id: response.order_id.clone(),
                            investment_amount: fixed_amount,
                            units: balance_f64, // Use actual confirmed balance
                            purchase_price: opportunity.bid_price,
                            sell_price: self.config.sell_price,
                            timestamp: std::time::Instant::now(),
                            market_timestamp: opportunity.period_timestamp,
                            sold: false,
                            confirmed_balance: Some(balance_f64),
                            buy_order_confirmed: true,
                            limit_sell_orders_placed: false, // Will be set to true after placing sell orders
                            no_sell: is_dual_limit_hedge, // Don't sell multi-market hedge trades - hold until closure. Individual hedges can sell.
                            claim_on_closure: is_dual_limit_hedge, // Will claim at market closure
                            sell_attempts: 0,
                            redemption_attempts: 0,
                            redemption_abandoned: false,
                        };
                        
                        // Use token_id as key to track individual tokens
                        // Add marker for different hedge types
                        let trade_key = if is_individual_hedge {
                            format!("{}_{}_individual_hedge", opportunity.period_timestamp, opportunity.token_id)
                        } else if is_standard_hedge {
                            format!("{}_{}_standard_hedge", opportunity.period_timestamp, opportunity.token_id)
                        } else if is_dual_limit_hedge {
                            format!("{}_{}_dual_limit_hedge", opportunity.period_timestamp, opportunity.token_id)
                        } else {
                            format!("{}_{}", opportunity.period_timestamp, opportunity.token_id)
                        };
                        let mut pending = self.pending_trades.lock().await;
                        if is_dual_limit_hedge {
                            debug!("Created hedge trade with no_sell=true, key: {}", trade_key);
                        } else if is_individual_hedge {
                            debug!("Created individual hedge trade with no_sell=false, key: {}", trade_key);
                        }
                        pending.insert(trade_key.clone(), trade.clone());
                        drop(pending);
                        
                        // For standard hedges, spawn background task to place 2 limit sell orders at $0.93 and $0.98 after 7 seconds
                        if is_standard_hedge && !self.simulation_mode {
                            let api_clone = self.api.clone();
                            let trade_key_clone = trade_key.clone();
                            let token_id_clone = opportunity.token_id.clone();
                            let token_type_clone = opportunity.token_type.clone();
                            let period_timestamp = opportunity.period_timestamp;
                            let pending_trades_clone = self.pending_trades.clone();
                            let sell_size = opportunity.dual_limit_shares.unwrap_or(units);
                            
                            // Spawn non-blocking background task
                            tokio::spawn(async move {
                                crate::log_println!("   ⏳ Waiting 7 seconds before placing limit sell orders for standard hedge (non-blocking)...");
                                tokio::time::sleep(tokio::time::Duration::from_secs(7)).await;
                                
                                Self::place_hedge_sell_orders_with_retry(
                                    &api_clone,
                                    &pending_trades_clone,
                                    &trade_key_clone,
                                    &token_id_clone,
                                    &token_type_clone,
                                    period_timestamp,
                                    sell_size,
                                    true, // is_standard_hedge
                                ).await;
                            });
                        }
                        
                        // For individual hedges, spawn background task to place 2 limit sell orders at $0.93 and $0.98 after 7 seconds
                        if is_individual_hedge && !self.simulation_mode {
                            let api_clone = self.api.clone();
                            let trade_key_clone = trade_key.clone();
                            let token_id_clone = opportunity.token_id.clone();
                            let token_type_clone = opportunity.token_type.clone();
                            let period_timestamp = opportunity.period_timestamp;
                            let pending_trades_clone = self.pending_trades.clone();
                            let sell_size = opportunity.dual_limit_shares.unwrap_or(units);
                            
                            // Spawn non-blocking background task
                            tokio::spawn(async move {
                                crate::log_println!("   ⏳ Waiting 7 seconds before placing limit sell orders for individual hedge (non-blocking)...");
                                tokio::time::sleep(tokio::time::Duration::from_secs(7)).await;
                                
                                Self::place_hedge_sell_orders_with_retry(
                                    &api_clone,
                                    &pending_trades_clone,
                                    &trade_key_clone,
                                    &token_id_clone,
                                    &token_type_clone,
                                    period_timestamp,
                                    sell_size,
                                    false, // is_standard_hedge (false = individual hedge)
                                ).await;
                            });
                        }
                        
                        // Log structured buy order event to history.toml with allowance status
                        let market_name = opportunity.token_type.display_name();
                        let order_id_str = response.order_id.as_ref()
                            .map(|id| format!("{:?}", id))
                            .unwrap_or_else(|| "N/A".to_string());
                        let buy_event = format!(
                            "BUY ORDER | Market: {} | Period: {} | Token: {} | Price: ${:.6} | Units: {:.6} | Cost: ${:.6} | Order ID: {} | Status: CONFIRMED",
                            market_name,
                            opportunity.period_timestamp,
                            &opportunity.token_id[..16],
                            opportunity.bid_price,
                            balance_f64,
                            balance_f64 * opportunity.bid_price,
                            order_id_str
                        );
                        crate::log_trading_event(&buy_event);
                        
                        crate::log_println!("   ✅ Trade stored successfully. Will monitor price and sell at ${:.6} or market close.", 
                              self.config.sell_price);
                        
                        let mut trades = self.trades_executed.lock().await;
                        *trades += 1;
                        drop(trades);
                    } else {
                        // Balance confirmation failed or timeout - still log the successful buy
                        let status_note = if !balance_confirmed {
                            "Balance check timeout/failed"
                        } else if balance_f64 == 0.0 {
                            "Balance is 0 (may still be settling)"
                        } else {
                            "Balance mismatch"
                        };
                        
                        // Log successful buy even if balance confirmation failed
                        let buy_event = format!(
                            "BUY ORDER | Market: {} | Period: {} | Token: {} | Price: ${:.6} | Units: {:.6} | Cost: ${:.6} | Order ID: {} | Status: SUCCESS | Note: {}",
                            market_name,
                            opportunity.period_timestamp,
                            &opportunity.token_id[..16],
                            opportunity.bid_price,
                            balance_f64,
                            balance_f64 * opportunity.bid_price,
                            order_id_str,
                            status_note
                        );
                        crate::log_trading_event(&buy_event);
                        
                        warn!("⚠️  Balance mismatch: Expected ~{:.6} shares, but only have {:.6} shares", units, balance_f64);
                        warn!("   The buy order may not have executed fully, or price changed significantly");
                        // Still store the trade but mark as unconfirmed
                        // For dual limit bot: 
                        // - If is_individual_hedge is true, this is an individual hedge that should place a limit sell order at config.sell_price
                        // - If is_standard_hedge is true, this is a standard hedge (after dual_limit_hedge_after_minutes) that should place a limit sell order at $0.98
                        // - If use_market_order is true but neither is_individual_hedge nor is_standard_hedge, this is a multi-market hedge that should be held until closure
                        let is_individual_hedge = opportunity.is_individual_hedge;
                        let is_standard_hedge = opportunity.is_standard_hedge;
                        let is_dual_limit_hedge = opportunity.use_market_order && !is_individual_hedge && !is_standard_hedge;
                        let trade = PendingTrade {
                            token_id: opportunity.token_id.clone(),
                            condition_id: opportunity.condition_id.clone(),
                            token_type: opportunity.token_type.clone(),
                            order_id: response.order_id.clone(),
                            investment_amount: fixed_amount,
                            units: balance_f64, // Use actual balance
                            purchase_price: opportunity.bid_price,
                            sell_price: self.config.sell_price,
                            timestamp: std::time::Instant::now(),
                            market_timestamp: opportunity.period_timestamp,
                            sold: false,
                            confirmed_balance: Some(balance_f64),
                            buy_order_confirmed: balance_f64 > 0.0, // Confirmed if we have any tokens
                            limit_sell_orders_placed: false, // Will be set to true after placing sell orders
                            no_sell: is_dual_limit_hedge, // Don't sell multi-market hedge trades - hold until closure. Individual hedges can sell.
                            claim_on_closure: is_dual_limit_hedge, // Will claim at market closure
                            sell_attempts: 0,
                            redemption_attempts: 0,
                            redemption_abandoned: false,
                        };
                        
                        let trade_key = if is_individual_hedge {
                            format!("{}_{}_individual_hedge", opportunity.period_timestamp, opportunity.token_id)
                        } else if is_standard_hedge {
                            format!("{}_{}_standard_hedge", opportunity.period_timestamp, opportunity.token_id)
                        } else if is_dual_limit_hedge {
                            format!("{}_{}_dual_limit_hedge", opportunity.period_timestamp, opportunity.token_id)
                        } else {
                            format!("{}_{}", opportunity.period_timestamp, opportunity.token_id)
                        };
                        let mut pending = self.pending_trades.lock().await;
                        if is_dual_limit_hedge {
                            debug!("Created hedge trade with no_sell=true (balance mismatch), key: {}", trade_key);
                        } else if is_individual_hedge {
                            debug!("Created individual hedge trade with no_sell=false (balance mismatch), key: {}", trade_key);
                        }
                        pending.insert(trade_key.clone(), trade.clone());
                        drop(pending);
                        
                        // IMPORTANT: Limit sell orders for hedges are ONLY placed after balance confirmation
                        // If balance confirmation failed or timed out, we skip placing limit sell orders
                        // The trade will be tracked, and limit sell orders will be placed later once balance is confirmed
                        if is_standard_hedge {
                            warn!("⚠️  Balance confirmation failed or timed out for standard hedge - skipping limit sell order placement");
                            warn!("   Limit sell order will be placed once balance is confirmed in next check");
                        }
                        
                        // For individual hedges, also skip if balance not confirmed
                        if is_individual_hedge {
                            warn!("⚠️  Balance confirmation failed or timed out for individual hedge - skipping limit sell order placement");
                            warn!("   Limit sell order will be placed once balance is confirmed in next check");
                        }
                        
                        let mut trades = self.trades_executed.lock().await;
                        *trades += 1;
                        drop(trades);
                    }
                    
                    return Ok(());
                }
                Err(e) => {
                crate::log_println!("   ❌ ORDER FAILED");
                crate::log_println!("      Error: {}", e);
                warn!("   ⚠️  Failed to place {} token order: {}", opportunity.token_type.display_name(), e);
                
                // Log structured buy order failure to history.toml (simplified error message)
                let market_name = opportunity.token_type.display_name();
                let error_msg = format!("{:?}", e);
                let simple_error = if error_msg.contains("order couldn't be fully filled") || error_msg.contains("FOK orders") {
                    "order couldn't be fully filled (FOK)".to_string()
                } else if error_msg.contains("not enough balance") {
                    "not enough balance / allowance".to_string()
                } else if error_msg.contains("Insufficient") {
                    "Insufficient balance or allowance".to_string()
                } else {
                    // Extract first meaningful line from error
                    error_msg.lines()
                        .find(|line| !line.trim().is_empty() && !line.contains("Troubleshooting") && !line.contains("Order details"))
                        .unwrap_or("Order failed")
                        .to_string()
                };
                
                let buy_event = format!(
                    "BUY ORDER | Market: {} | Period: {} | Token: {} | Price: ${:.6} | Units: {:.6} | Cost: ${:.6} | Status: FAILED | Error: {}",
                    market_name,
                    opportunity.period_timestamp,
                    &opportunity.token_id[..16],
                    opportunity.bid_price,
                    units,
                    total_cost,
                    simple_error
                );
                crate::log_trading_event(&buy_event);
                
                    return Err(e);
                }
            }
        }
        
        // For simulation mode, store the trade
        if self.simulation_mode {
            crate::log_println!("   📝 Storing trade: {} token ID: {}...", 
                  opportunity.token_type.display_name(), &opportunity.token_id[..16]);
        
        let trade = PendingTrade {
                token_id: opportunity.token_id.clone(),
                condition_id: opportunity.condition_id.clone(),
                token_type: opportunity.token_type.clone(),
                order_id: None,
            investment_amount: fixed_amount,
            units,
                purchase_price: opportunity.bid_price,
            sell_price: self.config.sell_price,
            timestamp: std::time::Instant::now(),
            market_timestamp: opportunity.period_timestamp,
            sold: false,
                confirmed_balance: None,
                buy_order_confirmed: false,
                limit_sell_orders_placed: false, // Will be set to true after placing sell orders
                no_sell: false,
                claim_on_closure: false,
                sell_attempts: 0,
                redemption_attempts: 0,
                redemption_abandoned: false,
            };
            
            // Use token_id as key to track individual tokens
            let trade_key = format!("{}_{}", opportunity.period_timestamp, opportunity.token_id);
        let mut pending = self.pending_trades.lock().await;
        pending.insert(trade_key, trade);
        drop(pending);
        
            crate::log_println!("   ✅ Trade stored successfully. Will monitor price and sell at ${:.6} or market close.", 
              self.config.sell_price);
        
        let mut trades = self.trades_executed.lock().await;
        *trades += 1;
        drop(trades);
        }
        
        Ok(())
    }

    /// Execute limit buy order - places limit buy for both Up and Down tokens
    /// When a buy order fills, immediately places ONE limit sell order at sell_price (profit target)
    /// Stop-loss is disabled for limit order version
    pub async fn execute_limit_buy(
        &self,
        opportunity: &BuyOpportunity,
        place_sell_orders: bool,
        size_override: Option<f64>,
    ) -> Result<()> {
        let fixed_amount = self.config.fixed_trade_amount;
        let units = size_override.unwrap_or_else(|| fixed_amount / opportunity.bid_price);
        let investment_amount = units * opportunity.bid_price;
        
        // Only profit target sell price (stop-loss disabled for limit order version)
        let sell_price = self.config.sell_price;
        
        crate::log_println!("═══════════════════════════════════════════════════════════");
        crate::log_println!("📋 PLACING LIMIT BUY ORDER");
        crate::log_println!("═══════════════════════════════════════════════════════════");
        crate::log_println!("📊 Order Details:");
        crate::log_println!("   Token Type: {}", opportunity.token_type.display_name());
        crate::log_println!("   Token ID: {}", opportunity.token_id);
        crate::log_println!("   Condition ID: {}", opportunity.condition_id);
        crate::log_println!("   Side: BUY (LIMIT)");
        crate::log_println!("   Limit Price: ${:.6}", opportunity.bid_price);
        crate::log_println!("   Size: {:.6} shares", units);
        crate::log_println!("   Investment Amount: ${:.2}", investment_amount);
        if place_sell_orders {
            crate::log_println!("   When filled, will place limit sell order:");
            crate::log_println!("      - Sell at ${:.6} (profit target)", sell_price);
            crate::log_println!("      - Stop-loss disabled for limit order version");
        } else {
            crate::log_println!("   When filled, no sell orders will be placed (log confirmation only)");
        }
        crate::log_println!("   Time elapsed: {}m {}s", 
            opportunity.time_elapsed_seconds / 60, opportunity.time_elapsed_seconds % 60);
        crate::log_println!("");
        
        if self.simulation_mode {
            crate::log_println!("🎮 SIMULATION MODE - Limit order NOT placed");
            crate::log_println!("   ✅ SIMULATION: Limit buy order would be placed:");
            crate::log_println!("      - Token: {}", opportunity.token_type.display_name());
            crate::log_println!("      - Price: ${:.6}", opportunity.bid_price);
            crate::log_println!("      - Size: {:.6} shares", units);
            crate::log_println!("      - Strategy: Hold until market closure (claim $1.00 if winning, $0.00 if losing)");
            
            // Add to simulation tracker
            if let Some(tracker) = &self.simulation_tracker {
                tracker.add_limit_order(
                    opportunity.token_id.clone(),
                    opportunity.token_type.clone(),
                    opportunity.condition_id.clone(),
                    opportunity.bid_price,
                    units,
                    "BUY".to_string(),
                    opportunity.period_timestamp,
                ).await;
                
                // Also track as pending trade for consistency
                // In simulation mode, we hold positions until market closure (no selling)
                let trade_key = format!("{}_{}_limit", opportunity.period_timestamp, opportunity.token_id);
                let mut pending = self.pending_trades.lock().await;
                let trade = PendingTrade {
                    token_id: opportunity.token_id.clone(),
                    condition_id: opportunity.condition_id.clone(),
                    token_type: opportunity.token_type.clone(),
                    order_id: None,
                    investment_amount,
                    units,
                    purchase_price: opportunity.bid_price,
                    sell_price: self.config.sell_price, // Not used in simulation - positions held until closure
                    timestamp: std::time::Instant::now(),
                    market_timestamp: opportunity.period_timestamp,
                    sold: false,
                    confirmed_balance: None,
                    buy_order_confirmed: false,
                    limit_sell_orders_placed: false, // No sell orders in simulation - hold until closure
                    no_sell: true, // Mark as no_sell since we hold until market closure
                    claim_on_closure: true, // Will claim at market closure
                    sell_attempts: 0,
                    redemption_attempts: 0,
                    redemption_abandoned: false,
                };
                pending.insert(trade_key, trade);
            }
            
            return Ok(());
        }
        
        // Place limit buy order
        use rust_decimal::Decimal;
        use crate::models::OrderRequest;
        
        // Format size to 2 decimal places (API requirement: maximum 2 decimal places)
        let size_formatted = format!("{:.2}", units);
        
        let order = OrderRequest {
            token_id: opportunity.token_id.clone(),
            side: "BUY".to_string(),
            size: size_formatted,
            price: format!("{:.2}", opportunity.bid_price), // Also format price to 2 decimal places
            order_type: "LIMIT".to_string(),
        };
        
        crate::log_println!("🚀 Placing limit buy order on exchange...");
        match self.api.place_order(&order).await {
            Ok(response) => {
                crate::log_println!("   ✅ LIMIT BUY ORDER PLACED");
                crate::log_println!("      Order ID: {:?}", response.order_id);
                crate::log_println!("      Status: {}", response.status);
                if let Some(msg) = &response.message {
                    crate::log_println!("      Message: {}", msg);
                }
                
                // Track the limit order - we'll check for fills in check_pending_trades
                let trade_key = format!("{}_{}_limit", opportunity.period_timestamp, opportunity.token_id);
                let mut pending = self.pending_trades.lock().await;
                
                // Store initial balance to detect fills
                let initial_balance = match self.api.check_balance_only(&opportunity.token_id).await {
                    Ok(balance) => {
                        let balance_decimal = balance / Decimal::from(1_000_000u64);
                        f64::try_from(balance_decimal).unwrap_or(0.0)
                    }
                    Err(_) => 0.0,
                };
                
                // Store both sell prices in the trade (we'll use sell_price as the primary one for tracking)
                let trade = PendingTrade {
                    token_id: opportunity.token_id.clone(),
                    condition_id: opportunity.condition_id.clone(),
                    token_type: opportunity.token_type.clone(),
                    order_id: response.order_id.clone(),
                    investment_amount: investment_amount,
                    units: units, // Expected units
                    purchase_price: opportunity.bid_price,
                    sell_price, // Primary sell price (profit target)
                    timestamp: std::time::Instant::now(),
                    market_timestamp: opportunity.period_timestamp,
                    sold: false,
                    confirmed_balance: Some(initial_balance),
                    buy_order_confirmed: false, // Will be true when limit order fills
                    limit_sell_orders_placed: false, // Will be set to true after placing sell orders
                    no_sell: !place_sell_orders,
                    claim_on_closure: false,
                    sell_attempts: 0,
                    redemption_attempts: 0,
                    redemption_abandoned: false,
                };
                
                pending.insert(trade_key, trade);
                drop(pending);
                
                if place_sell_orders {
                    crate::log_println!("   📝 Order tracked - will monitor for fill and place limit sell order:");
                    crate::log_println!("      - Sell at ${:.6} (profit target)", sell_price);
                    crate::log_println!("      - Stop-loss disabled for limit order version");
                } else {
                    crate::log_println!("   📝 Order tracked - will monitor for fill and log confirmation only");
                }
                crate::log_println!("   💡 Initial balance: {:.6} shares", initial_balance);
                
                // Log the limit order event
                let order_id_str = response.order_id.as_ref()
                    .map(|id| format!("{:?}", id))
                    .unwrap_or_else(|| "N/A".to_string());
                let buy_event = format!(
                    "LIMIT BUY ORDER | Market: {} | Period: {} | Token: {} | Limit Price: ${:.6} | Size: {:.6} | Order ID: {} | Target Sell: ${:.6} (profit only, stop-loss disabled)",
                    opportunity.token_type.display_name(),
                    opportunity.period_timestamp,
                    &opportunity.token_id[..16],
                    opportunity.bid_price,
                    units,
                    order_id_str,
                    sell_price
                );
                crate::log_trading_event(&buy_event);
            }
            Err(e) => {
                eprintln!("   ❌ FAILED TO PLACE LIMIT BUY ORDER: {}", e);
                return Err(e);
            }
        }
        
        Ok(())
    }

    /// Check pending trades and sell when price reaches sell_price (0.99 or 1.0)
    /// Also handles limit order fills: detects when limit buy orders fill and places limit sell orders
    pub async fn check_pending_trades(&self) -> Result<()> {
        // In simulation mode, check limit orders against current prices
        if self.simulation_mode {
            if let Some(tracker) = &self.simulation_tracker {
                // Log that we're checking
                tracker.log_to_file("🔄 SIMULATION: check_pending_trades called").await;
                
                // Get current prices for all tokens we're tracking
                let mut current_prices = HashMap::new();
                
                // In simulation mode, use simulation tracker's pending orders as source of truth
                // Get token IDs from pending limit orders in simulation tracker
                let pending_order_token_ids = tracker.get_pending_order_token_ids().await;
                let pending_order_count = tracker.get_pending_order_count().await;
                
                tracker.log_to_file(&format!(
                    "📊 SIMULATION: Found {} pending limit order(s) waiting for fills\n",
                    pending_order_count
                )).await;
                
                // Collect all unique token IDs we need prices for (from simulation tracker only)
                let mut token_ids_to_fetch = std::collections::HashSet::new();
                for token_id in &pending_order_token_ids {
                    token_ids_to_fetch.insert(token_id.clone());
                }
                
                if token_ids_to_fetch.is_empty() {
                    tracker.log_to_file("⚠️  SIMULATION: No token IDs to fetch prices for").await;
                    return Ok(());
                }
                
                tracker.log_to_file(&format!(
                    "🔍 SIMULATION: Fetching prices for {} unique token(s)\n",
                    token_ids_to_fetch.len()
                )).await;
                
                // Fetch prices for all tokens
                for token_id in token_ids_to_fetch {
                    if !current_prices.contains_key(&token_id) {
                        // Fetch current price for this token using orderbook
                        match self.api.get_orderbook(&token_id).await {
                            Ok(orderbook) => {
                                let bid = orderbook.bids.first().map(|e| e.price);
                                let ask = orderbook.asks.first().map(|e| e.price);
                                let token_price = TokenPrice {
                                    token_id: token_id.clone(),
                                    bid,
                                    ask,
                                };
                                current_prices.insert(token_id.clone(), token_price);
                                
                                // Log if ask is missing (for BUY orders) or bid is missing (for SELL orders)
                                if ask.is_none() {
                                    if let Some(tracker) = &self.simulation_tracker {
                                        let pending_order_token_ids = tracker.get_pending_order_token_ids().await;
                                        if pending_order_token_ids.contains(&token_id) {
                                            tracker.log_to_file(&format!(
                                                "⚠️  SIMULATION: No ask price available for token {} (BUY orders may not fill)",
                                                &token_id[..16]
                                            )).await;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                // Log API errors in simulation mode
                                if let Some(tracker) = &self.simulation_tracker {
                                    tracker.log_to_file(&format!(
                                        "⚠️  SIMULATION: Failed to fetch orderbook for token {}: {}",
                                        &token_id[..16],
                                        e
                                    )).await;
                                }
                            }
                        }
                    }
                }
                
                // Log how many prices we fetched
                if current_prices.is_empty() {
                    tracker.log_to_file("⚠️  SIMULATION: No prices fetched for any tokens").await;
                } else {
                    tracker.log_to_file(&format!(
                        "✅ SIMULATION: Fetched prices for {} token(s)\n",
                        current_prices.len()
                    )).await;
                }
                
                // Check limit orders against current prices
                tracker.check_limit_orders(&current_prices).await;
                
                // Log pending orders summary every check (to see what's happening)
                tracker.log_pending_orders_summary(&current_prices).await;
                
                // Check if any simulated fills created new positions that need sell orders
                let pending_trades_after: Vec<(String, PendingTrade)> = {
                    let pending = self.pending_trades.lock().await;
                    pending.iter()
                        .map(|(key, trade)| (key.clone(), trade.clone()))
                        .collect()
                };
                
                for (key, trade) in &pending_trades_after {
                    // Check if this trade has a filled position (for both limit and market orders)
                    if !trade.buy_order_confirmed && !trade.sold {
                        // Check if the position exists in simulation tracker (order was filled)
                        if tracker.has_position(&trade.token_id).await {
                            // Order was filled in simulation - update trade status
                            // In simulation mode, we hold positions until market closure (no selling)
                            let mut pending = self.pending_trades.lock().await;
                            if let Some(t) = pending.get_mut(key.as_str()) {
                                t.buy_order_confirmed = true;
                                t.confirmed_balance = Some(t.units);
                                
                                tracker.log_to_file(&format!(
                                    "✅ SIMULATION: Position confirmed for {} - holding until market closure (will claim at $1.00 if winning, $0.00 if losing)",
                                    trade.token_type.display_name()
                                )).await;
                            }
                        }
                    }
                }
                
                // Note: In simulation mode, we don't create sell orders
                // Positions will be resolved at market closure ($1 for winning tokens, $0 for losing tokens)
            }
        }
        
        let pending_trades: Vec<(String, PendingTrade)> = {
            let pending = self.pending_trades.lock().await;
            pending.iter()
                .map(|(key, trade)| (key.clone(), trade.clone()))
                .collect()
        };
        
        if pending_trades.is_empty() {
            return Ok(());
        }
        
        // First, check for limit buy order fills
        for (key, trade) in &pending_trades {
            // Skip if already sold or already confirmed
            if trade.sold || trade.buy_order_confirmed {
                continue;
            }
            
            // Check if this is a limit order (key contains "_limit")
            if !key.contains("_limit") {
                continue;
            }
            
            // Check current balance to detect fill
            use rust_decimal::Decimal;
            let current_balance = match self.api.check_balance_only(&trade.token_id).await {
                Ok(balance) => {
                    let balance_decimal = balance / Decimal::from(1_000_000u64);
                    f64::try_from(balance_decimal).unwrap_or(0.0)
                }
                Err(_) => continue,
            };
            
            // Get initial balance from trade
            let initial_balance = trade.confirmed_balance.unwrap_or(0.0);
            
            // If balance increased, limit buy order filled
            if current_balance > initial_balance + 0.000001 { // Small threshold to account for rounding
                crate::log_println!("═══════════════════════════════════════════════════════════");
                crate::log_println!("✅ LIMIT BUY ORDER FILLED");
                crate::log_println!("═══════════════════════════════════════════════════════════");
                crate::log_println!("📊 Fill Details:");
                crate::log_println!("   Token Type: {}", trade.token_type.display_name());
                crate::log_println!("   Token ID: {}", trade.token_id);
                crate::log_println!("   Initial Balance: {:.6} shares", initial_balance);
                crate::log_println!("   Current Balance: {:.6} shares", current_balance);
                crate::log_println!("   Filled Amount: {:.6} shares", current_balance - initial_balance);
                crate::log_println!("   Purchase Price: ${:.6}", trade.purchase_price);
                crate::log_println!("   Target Sell Price: ${:.6}", trade.sell_price);
                crate::log_println!("");
                
                // Update trade with confirmed balance
                {
                    let mut pending = self.pending_trades.lock().await;
                    if let Some(t) = pending.get_mut(key.as_str()) {
                        t.confirmed_balance = Some(current_balance);
                        t.units = current_balance; // Update units to actual filled amount
                        t.buy_order_confirmed = true;
                    }
                }
                
                // For no-sell mode, log confirmation only and skip sell placement
                if trade.no_sell {
                    crate::log_println!("✅ No-sell mode: confirmation logged, no sell orders will be placed.");
                    let mut pending = self.pending_trades.lock().await;
                    if let Some(t) = pending.get_mut(key.as_str()) {
                        t.limit_sell_orders_placed = true;
                    }
                    continue;
                }

                // Place limit sell order immediately
                if !self.simulation_mode {
                    crate::log_println!("📤 Placing limit sell order at ${:.6}...", trade.sell_price);
                    
                    use crate::models::OrderRequest;
                    let sell_order = OrderRequest {
                        token_id: trade.token_id.clone(),
                        side: "SELL".to_string(),
                        size: current_balance.to_string(),
                        price: trade.sell_price.to_string(),
                        order_type: "LIMIT".to_string(),
                    };
                    
                    match self.api.place_order(&sell_order).await {
                        Ok(response) => {
                            crate::log_println!("   ✅ LIMIT SELL ORDER PLACED");
                            crate::log_println!("      Order ID: {:?}", response.order_id);
                            crate::log_println!("      Limit Price: ${:.6}", trade.sell_price);
                            crate::log_println!("      Size: {:.6} shares", current_balance);
                            
                            let order_id_str = response.order_id.as_ref()
                                .map(|id| format!("{:?}", id))
                                .unwrap_or_else(|| "N/A".to_string());
                            let sell_event = format!(
                                "LIMIT SELL ORDER | Market: {} | Period: {} | Token: {} | Limit Price: ${:.6} | Size: {:.6} | Order ID: {}",
                                trade.token_type.display_name(),
                                trade.market_timestamp,
                                &trade.token_id[..16],
                                trade.sell_price,
                                current_balance,
                                order_id_str
                            );
                            crate::log_trading_event(&sell_event);
                        }
                        Err(e) => {
                            eprintln!("   ❌ FAILED TO PLACE LIMIT SELL ORDER: {}", e);
                            warn!("Failed to place limit sell order after buy fill: {}", e);
                        }
                    }
                } else {
                    crate::log_println!("🎮 SIMULATION: Limit sell order would be placed at ${:.6}", trade.sell_price);
                }
            }
        }
        
        // Second: Check for market buys that are confirmed and place limit sell orders
        // Market buys are stored with buy_order_confirmed: true immediately after confirmation
        // We need to place TWO limit sell orders (profit target and stop-loss) for them
        for (key, trade) in &pending_trades {
            // Skip if already sold, not confirmed, or sell orders already placed
            if trade.sold || !trade.buy_order_confirmed || trade.limit_sell_orders_placed {
                continue;
            }
            
            // Skip if no_sell flag is set (e.g., dual limit bot trades should be held until closure)
            if trade.no_sell {
                debug!("Skipping sell order for {} - no_sell flag is set (key: {})", 
                    trade.token_type.display_name(), key);
                continue;
            }
            
            // Skip limit orders (they're handled above) - only process market buys
            // Also explicitly skip all hedge trades (individual, standard, and multi-market)
            // Individual and standard hedges already have limit sell orders placed, multi-market hedges are held until closure
            if key.contains("_limit") || key.contains("_individual_hedge") || key.contains("_standard_hedge") || key.contains("_dual_limit_hedge") {
                debug!("Skipping sell order for {} - limit order or hedge trade (key: {})", 
                    trade.token_type.display_name(), key);
                continue;
            }
            
            // Get current balance
            use rust_decimal::Decimal;
            let current_balance = match self.api.check_balance_only(&trade.token_id).await {
                Ok(balance) => {
                    let balance_decimal = balance / Decimal::from(1_000_000u64);
                    f64::try_from(balance_decimal).unwrap_or(0.0)
                }
                Err(_) => continue,
            };
            
            // Only proceed if we have tokens
            if current_balance < 0.000001 {
                continue;
            }
            
            // Place limit sell order for bought token at sell_price (no hedge limit buy)
            if !self.simulation_mode {
                let sell_price = self.config.sell_price;
                
                crate::log_println!("═══════════════════════════════════════════════════════════");
                crate::log_println!("📤 PLACING ORDER AFTER MARKET BUY");
                crate::log_println!("═══════════════════════════════════════════════════════════");
                crate::log_println!("📊 Order Details:");
                crate::log_println!("   Bought Token: {}", trade.token_type.display_name());
                crate::log_println!("   Token ID: {}", trade.token_id);
                crate::log_println!("   Current Balance: {:.6} shares", current_balance);
                crate::log_println!("");
                crate::log_println!("   📋 Placing limit SELL for {} at ${:.6} (profit target)", trade.token_type.display_name(), sell_price);
                crate::log_println!("");
                
                use crate::models::OrderRequest;
                
                // Place limit sell order for bought token at sell_price
                let sell_order_profit = OrderRequest {
                    token_id: trade.token_id.clone(),
                    side: "SELL".to_string(),
                    size: format!("{:.2}", current_balance), // Format to 2 decimal places
                    price: format!("{:.2}", sell_price), // Format to 2 decimal places
                    order_type: "LIMIT".to_string(),
                };
                
                match self.api.place_order(&sell_order_profit).await {
                    Ok(response) => {
                        crate::log_println!("   ✅ LIMIT SELL ORDER #1 PLACED (Profit Target)");
                        crate::log_println!("      Token: {}", trade.token_type.display_name());
                        crate::log_println!("      Order ID: {:?}", response.order_id);
                        crate::log_println!("      Limit Price: ${:.6}", sell_price);
                        crate::log_println!("      Size: {:.6} shares", current_balance);
                        
                        let order_id_str = response.order_id.as_ref()
                            .map(|id| format!("{:?}", id))
                            .unwrap_or_else(|| "N/A".to_string());
                        let sell_event = format!(
                            "LIMIT SELL ORDER (PROFIT) | Market: {} | Period: {} | Token: {} | Limit Price: ${:.6} | Size: {:.6} | Order ID: {}",
                            trade.token_type.display_name(),
                            trade.market_timestamp,
                            &trade.token_id[..16],
                            sell_price,
                            current_balance,
                            order_id_str
                        );
                        crate::log_trading_event(&sell_event);
                    }
                    Err(e) => {
                        eprintln!("   ❌ FAILED TO PLACE LIMIT SELL ORDER: {}", e);
                        warn!("Failed to place limit sell order (profit) after market buy: {}", e);
                    }
                }
                
                // Mark that sell order has been placed
                {
                    let mut pending = self.pending_trades.lock().await;
                    if let Some(t) = pending.get_mut(key.as_str()) {
                        t.limit_sell_orders_placed = true;
                    }
                }
            } else {
                crate::log_println!("🎮 SIMULATION: One order would be placed for market buy:");
                crate::log_println!("   Limit SELL for {} at ${:.6} (profit target) - {:.6} shares", trade.token_type.display_name(), self.config.sell_price, current_balance);
            }
        }
        
        // Continue with regular sell checks for filled orders
        for (key, mut trade) in pending_trades {
            
            // Check current balance to detect fill
            use rust_decimal::Decimal;
            let current_balance = match self.api.check_balance_only(&trade.token_id).await {
                Ok(balance) => {
                    let balance_decimal = balance / Decimal::from(1_000_000u64);
                    f64::try_from(balance_decimal).unwrap_or(0.0)
                }
                Err(_) => continue,
            };
            
            // Get initial balance from trade
            let initial_balance = trade.confirmed_balance.unwrap_or(0.0);
            
            // If balance increased, limit buy order filled
            if current_balance > initial_balance + 0.000001 { // Small threshold to account for rounding
                crate::log_println!("═══════════════════════════════════════════════════════════");
                crate::log_println!("✅ LIMIT BUY ORDER FILLED");
                crate::log_println!("═══════════════════════════════════════════════════════════");
                crate::log_println!("📊 Fill Details:");
                crate::log_println!("   Token Type: {}", trade.token_type.display_name());
                crate::log_println!("   Token ID: {}", trade.token_id);
                crate::log_println!("   Initial Balance: {:.6} shares", initial_balance);
                crate::log_println!("   Current Balance: {:.6} shares", current_balance);
                crate::log_println!("   Filled Amount: {:.6} shares", current_balance - initial_balance);
                crate::log_println!("   Purchase Price: ${:.6}", trade.purchase_price);
                crate::log_println!("   Will place limit sell order:");
                crate::log_println!("      - Sell at ${:.6} (profit target)", self.config.sell_price);
                crate::log_println!("      - Stop-loss disabled for limit order version");
                crate::log_println!("");
                
                // Update trade with confirmed balance
                {
                    let mut pending = self.pending_trades.lock().await;
                    if let Some(t) = pending.get_mut(key.as_str()) {
                        t.confirmed_balance = Some(current_balance);
                        t.units = current_balance; // Update units to actual filled amount
                        t.buy_order_confirmed = true;
                    }
                }
                
                // For no-sell mode (dual limit bot), log confirmation only and skip sell placement
                if trade.no_sell {
                    crate::log_println!("✅ No-sell mode: confirmation logged, no sell orders will be placed.");
                    let mut pending = self.pending_trades.lock().await;
                    if let Some(t) = pending.get_mut(key.as_str()) {
                        t.limit_sell_orders_placed = true;
                    }
                    continue;
                }
                
                // Place ONE limit sell order at profit target (stop-loss disabled for limit order version)
                if !self.simulation_mode {
                    let sell_price = self.config.sell_price;
                    
                    crate::log_println!("📤 Placing limit sell order:");
                    crate::log_println!("   Profit target: ${:.6}", sell_price);
                    crate::log_println!("   (Stop-loss disabled for limit order version)");
                    
                    use crate::models::OrderRequest;
                    
                    // Place sell order at profit target (sell_price)
                    let sell_order_profit = OrderRequest {
                        token_id: trade.token_id.clone(),
                        side: "SELL".to_string(),
                        size: format!("{:.2}", current_balance), // Format to 2 decimal places
                        price: format!("{:.2}", sell_price), // Format to 2 decimal places
                        order_type: "LIMIT".to_string(),
                    };
                    
                    match self.api.place_order(&sell_order_profit).await {
                        Ok(response) => {
                            crate::log_println!("   ✅ LIMIT SELL ORDER PLACED (Profit Target)");
                            crate::log_println!("      Order ID: {:?}", response.order_id);
                            crate::log_println!("      Limit Price: ${:.6}", sell_price);
                            crate::log_println!("      Size: {:.6} shares", current_balance);
                            
                            let order_id_str = response.order_id.as_ref()
                                .map(|id| format!("{:?}", id))
                                .unwrap_or_else(|| "N/A".to_string());
                            let sell_event = format!(
                                "LIMIT SELL ORDER (PROFIT) | Market: {} | Period: {} | Token: {} | Limit Price: ${:.6} | Size: {:.6} | Order ID: {}",
                                trade.token_type.display_name(),
                                trade.market_timestamp,
                                &trade.token_id[..16],
                                sell_price,
                                current_balance,
                                order_id_str
                            );
                            crate::log_trading_event(&sell_event);
                        }
                        Err(e) => {
                            eprintln!("   ❌ FAILED TO PLACE LIMIT SELL ORDER (Profit): {}", e);
                            warn!("Failed to place profit target limit sell order: {}", e);
                        }
                    }
                    
                    // Mark that sell orders have been placed
                    {
                        let mut pending = self.pending_trades.lock().await;
                        if let Some(t) = pending.get_mut(key.as_str()) {
                            t.limit_sell_orders_placed = true;
                        }
                    }
                } else {
                    crate::log_println!("🎮 SIMULATION: Limit sell order would be placed:");
                    crate::log_println!("   - Sell at ${:.6} (profit target)", self.config.sell_price);
                    crate::log_println!("   (Stop-loss disabled for limit order version)");
                }
            }
        }
        
        // Check for limit sell order fills (detect when balance drops to 0 after placing sell orders)
        // This allows re-entry if price recovers after a stop-loss fill
        // Clone again for this check since we already used pending_trades above
        let pending_trades_sell_check: Vec<(String, PendingTrade)> = {
            let pending = self.pending_trades.lock().await;
            pending.iter()
                .map(|(key, trade)| (key.clone(), trade.clone()))
                .collect()
        };
        
        for (key, trade) in &pending_trades_sell_check {
            // Skip if already marked as sold or if buy order not confirmed yet
            if trade.sold || !trade.buy_order_confirmed {
                continue;
            }
            
            // Skip dual limit bot trades and all hedge trades (identified by markers in key or no_sell flag)
            // Individual and standard hedges already have limit sell orders placed, multi-market hedges are held until closure
            if key.contains("_limit") || key.contains("_individual_hedge") || key.contains("_standard_hedge") || key.contains("_dual_limit_hedge") || trade.no_sell {
                continue;
            }
            
            // Only check trades that have limit sell orders placed
            // This includes both regular limit sell orders and opposite token limit sell orders
            if !trade.limit_sell_orders_placed {
                continue;
            }
            
            // Check current balance - if it dropped to 0, a sell order filled
            use rust_decimal::Decimal;
            let current_balance = match self.api.check_balance_only(&trade.token_id).await {
                Ok(balance) => {
                    let balance_decimal = balance / Decimal::from(1_000_000u64);
                    f64::try_from(balance_decimal).unwrap_or(0.0)
                }
                Err(_) => continue,
            };
            
            // Get last known balance from trade
            let last_balance = trade.confirmed_balance.unwrap_or(0.0);
            
            // If balance dropped to 0 (or near 0), a sell order filled
            if last_balance > 0.000001 && current_balance < 0.000001 {
                // Determine if this is an opposite token trade
                let is_opposite_token = key.contains("_opposite_");
                let trade_description = if is_opposite_token {
                    "OPPOSITE TOKEN LIMIT SELL ORDER"
                } else {
                    "LIMIT SELL ORDER"
                };
                
                crate::log_println!("═══════════════════════════════════════════════════════════");
                crate::log_println!("✅ {} FILLED", trade_description);
                crate::log_println!("═══════════════════════════════════════════════════════════");
                crate::log_println!("📊 Sell Fill Details:");
                crate::log_println!("   Token Type: {}", trade.token_type.display_name());
                crate::log_println!("   Token ID: {}", trade.token_id);
                crate::log_println!("   Last Balance: {:.6} shares", last_balance);
                crate::log_println!("   Current Balance: {:.6} shares", current_balance);
                crate::log_println!("   Purchase Price: ${:.6}", trade.purchase_price);
                crate::log_println!("   Sell Price: ${:.6}", trade.sell_price);
                crate::log_println!("   Shares Sold: {:.6}", last_balance);
                crate::log_println!("   Revenue: ${:.6}", last_balance * trade.sell_price);
                crate::log_println!("   Status: FILLED");
                crate::log_println!("");
                
                // Mark trade as sold
                {
                    let mut pending = self.pending_trades.lock().await;
                    if let Some(t) = pending.get_mut(key.as_str()) {
                        t.sold = true;
                        t.confirmed_balance = Some(0.0);
                    }
                }
                
                // Log the sell fill event
                let sell_event = format!(
                    "{} FILLED | Market: {} | Period: {} | Token: {} | Purchase Price: ${:.6} | Sell Price: ${:.6} | Shares Sold: {:.6} | Revenue: ${:.6} | Status: FILLED",
                    trade_description,
                    trade.token_type.display_name(),
                    trade.market_timestamp,
                    &trade.token_id[..16],
                    trade.purchase_price,
                    trade.sell_price,
                    last_balance,
                    last_balance * trade.sell_price
                );
                crate::log_trading_event(&sell_event);
            }
        }
        
        // Continue with regular sell checks for filled orders (re-clone for second iteration)
        let pending_trades_2: Vec<(String, PendingTrade)> = {
            let pending = self.pending_trades.lock().await;
            pending.iter()
                .map(|(key, trade)| (key.clone(), trade.clone()))
                .collect()
        };
        
        for (key, mut trade) in pending_trades_2 {
            // Skip if already sold
            if trade.sold {
                continue;
            }
            
            // Get current ASK price (what we receive when selling)
            // Also check if there are actual buyers in the orderbook before attempting to sell
            let price_result = self.api.get_price(&trade.token_id, "SELL").await;
            let current_ask_price = match price_result {
                Ok(p) => {
                    let price_f64 = f64::try_from(p).unwrap_or(0.0);
                    // Log price check every 10th time to avoid spam (or use debug level)
                    debug!("Checking {} token {} ASK price: ${:.6} (target: ${:.6}, purchased at: ${:.6})", 
                           trade.token_type.display_name(), &trade.token_id[..16], price_f64, trade.sell_price, trade.purchase_price);
                    price_f64
                },
                Err(e) => {
                    debug!("Failed to get ASK price for {} token {}: {}", trade.token_type.display_name(), &trade.token_id[..16], e);
                    continue; // Skip if can't get price
                }
            };
            
            // Check orderbook to verify there are actual buyers before attempting to sell
            // This prevents "No opposing orders" errors when there's no liquidity
            let has_liquidity = match self.api.get_best_price(&trade.token_id).await {
                Ok(Some(token_price)) => {
                    // Check if there are actual bids (buyers) in the orderbook
                    token_price.bid.is_some()
                },
                Ok(None) => {
                    debug!("No orderbook data for {} token {} - skipping sell attempt", 
                           trade.token_type.display_name(), &trade.token_id[..16]);
                    false
                },
                Err(e) => {
                    debug!("Failed to check orderbook for {} token {}: {} - will try anyway", 
                           trade.token_type.display_name(), &trade.token_id[..16], e);
                    true // Assume liquidity exists if we can't check
                }
            };
            
            if !has_liquidity {
                debug!("Skipping sell for {} token {} - no buyers in orderbook (no liquidity)", 
                       trade.token_type.display_name(), &trade.token_id[..16]);
                continue; // Skip this trade - no liquidity to sell into
            }
            
            // Skip if already marked for claim (will be handled at market closure)
            if trade.claim_on_closure {
                continue;
            }
            
            // Skip stop-loss for dual limit bot trades (identified by hedge markers in key)
            // All hedge trades (individual, standard, and multi-market) should not trigger stop-loss
            // They use limit sell orders or hold until market closure
            if key.contains("_limit") || key.contains("_individual_hedge") || key.contains("_standard_hedge") || key.contains("_dual_limit_hedge") {
                continue;
            }
            
            // OPPOSITE TOKEN STOP-LOSS: Check if opposite token price drops below (1 - stop_loss_price - 0.1)
            // This protects against losses if the opposite token price crashes
            if key.contains("_opposite_") {
                if let Some(stop_loss_price) = self.config.stop_loss_price {
                    let opposite_stop_loss_price = (1.0 - stop_loss_price) - 0.1; // e.g., (1.0 - 0.80) - 0.1 = 0.10
                    
                    // Check if price dropped below opposite token stop-loss threshold
                    if current_ask_price <= opposite_stop_loss_price {
                        // CRITICAL: Re-check actual balance before selling
                        let actual_balance = match self.api.check_balance_allowance(&trade.token_id).await {
                            Ok((balance, _)) => {
                                let balance_decimal = balance / rust_decimal::Decimal::from(1_000_000u64);
                                let balance_f64 = f64::try_from(balance_decimal).unwrap_or(0.0);
                                if balance_f64 > 0.0 && (trade.units == 0.0 || balance_f64 > trade.units) {
                                    balance_f64
                                } else {
                                    trade.units
                                }
                            }
                            Err(e) => {
                                warn!("⚠️  Failed to re-check balance before opposite token stop-loss sell: {}. Using stored units: {:.6}", e, trade.units);
                                trade.units
                            }
                        };
                        
                        if actual_balance == 0.0 {
                            warn!("⚠️  Skipping opposite token stop-loss sell - actual balance is 0.000000 shares.");
                            continue;
                        }
                        
                        let units_to_sell = actual_balance;
                        
                        crate::log_println!("═══════════════════════════════════════════════════════════");
                        crate::log_println!("🛑 OPPOSITE TOKEN STOP-LOSS TRIGGERED");
                        crate::log_println!("═══════════════════════════════════════════════════════════");
                        crate::log_println!("⚠️  Opposite token price dropped below stop-loss threshold!");
                        crate::log_println!("💰 Current ASK Price: ${:.6}", current_ask_price);
                        crate::log_println!("🛑 Opposite Token Stop-Loss Price: ${:.6}", opposite_stop_loss_price);
                        crate::log_println!("📊 Purchase Price: ${:.6}", trade.purchase_price);
                        crate::log_println!("   Condition: {} <= {}", current_ask_price, opposite_stop_loss_price);
                        crate::log_println!("");
                        crate::log_println!("📊 Trade Details:");
                        crate::log_println!("   Token Type: {}", trade.token_type.display_name());
                        crate::log_println!("   Token ID: {}", trade.token_id);
                        crate::log_println!("   Units to sell: {:.6}", units_to_sell);
                        crate::log_println!("");
                        crate::log_println!("🔄 Executing stop-loss sell for opposite token...");
                        
                        // Execute stop-loss sell for opposite token
                        match self.execute_sell(&key, &mut trade, units_to_sell, current_ask_price, Some("FAK"), true).await {
                            Ok(_) => {
                                crate::log_println!("   ✅ OPPOSITE TOKEN STOP-LOSS SELL EXECUTED");
                                
                                // Mark trade as sold
                                {
                                    let mut pending = self.pending_trades.lock().await;
                                    if let Some(t) = pending.get_mut(key.as_str()) {
                                        t.sold = true;
                                        t.confirmed_balance = Some(0.0);
                                    }
                                }
                                
                                // Log the sell event
                                let sell_event = format!(
                                    "OPPOSITE TOKEN STOP-LOSS SELL | Market: {} | Period: {} | Token: {} | Purchase Price: ${:.6} | Sell Price: ${:.6} | Shares Sold: {:.6} | Status: FILLED",
                                    trade.token_type.display_name(),
                                    trade.market_timestamp,
                                    &trade.token_id[..16],
                                    trade.purchase_price,
                                    current_ask_price,
                                    units_to_sell
                                );
                                crate::log_trading_event(&sell_event);
                                
                                continue; // Move to next trade after handling opposite token stop-loss
                            }
                            Err(e) => {
                                eprintln!("   ❌ FAILED TO EXECUTE OPPOSITE TOKEN STOP-LOSS SELL: {}", e);
                                warn!("Failed to execute opposite token stop-loss sell: {}", e);
                                continue; // Move to next trade
                            }
                        }
                    }
                }
            }
            
            // NEW STRATEGY: Check for stop-loss condition for market buy trades
            // If price drops to stop_loss_price, sell the bought token and place limit sell for opposite token
            // Only apply to trades that have limit_sell_orders_placed (new strategy) and are NOT hedge trades
            // Skip all hedge trades (individual, standard, and multi-market) - they use limit sell orders or hold until closure
            if trade.limit_sell_orders_placed && !key.contains("_limit") && !key.contains("_individual_hedge") && !key.contains("_standard_hedge") && !key.contains("_dual_limit_hedge") {
                if let Some(stop_loss_price) = self.config.stop_loss_price {
                    // Only trigger stop-loss if price is at or below threshold
                    if current_ask_price <= stop_loss_price {
                        // CRITICAL: Re-check actual balance before selling
                        let actual_balance = match self.api.check_balance_allowance(&trade.token_id).await {
                            Ok((balance, _)) => {
                                let balance_decimal = balance / rust_decimal::Decimal::from(1_000_000u64);
                                let balance_f64 = f64::try_from(balance_decimal).unwrap_or(0.0);
                                if balance_f64 > 0.0 && (trade.units == 0.0 || balance_f64 > trade.units) {
                                    balance_f64
                                } else {
                                    trade.units
                                }
                            }
                            Err(e) => {
                                warn!("⚠️  Failed to re-check balance before stop-loss sell: {}. Using stored units: {:.6}", e, trade.units);
                                trade.units
                            }
                        };
                        
                        if actual_balance == 0.0 {
                            warn!("⚠️  Skipping stop-loss sell - actual balance is 0.000000 shares.");
                            continue;
                        }
                        
                        let units_to_sell = actual_balance;
                        
                        crate::log_println!("═══════════════════════════════════════════════════════════");
                        crate::log_println!("🛑 STOP-LOSS TRIGGERED - NEW STRATEGY");
                        crate::log_println!("═══════════════════════════════════════════════════════════");
                        crate::log_println!("⚠️  Price dropped to stop-loss threshold!");
                        crate::log_println!("💰 Current ASK Price: ${:.6}", current_ask_price);
                        crate::log_println!("🛑 Stop-Loss Price: ${:.6}", stop_loss_price);
                        crate::log_println!("📊 Purchase Price: ${:.6}", trade.purchase_price);
                        crate::log_println!("   Condition: {} <= {}", current_ask_price, stop_loss_price);
                        crate::log_println!("");
                        crate::log_println!("📊 Trade Details:");
                        crate::log_println!("   Token Type: {}", trade.token_type.display_name());
                        crate::log_println!("   Token ID: {}", trade.token_id);
                        crate::log_println!("   Units to sell: {:.6}", units_to_sell);
                        crate::log_println!("");
                        crate::log_println!("🔄 Executing stop-loss sell and placing opposite token order...");
                        
                        // Execute stop-loss sell
                        match self.execute_sell(&key, &mut trade, units_to_sell, current_ask_price, Some("FAK"), true).await {
                            Ok(_) => {
                                crate::log_println!("   ✅ STOP-LOSS SELL EXECUTED");
                                
                                // Place limit buy order for opposite token at (1 - stop_loss_price)
                                // This ensures we have the hedge even if the earlier limit buy didn't fill
                                let opposite_token_type = trade.token_type.opposite();
                                let opposite_buy_price = 1.0 - stop_loss_price; // e.g., 1.0 - 0.80 = 0.20
                                
                                match self.get_opposite_token_id(&trade.token_type, &trade.condition_id).await {
                                    Ok(opposite_token_id) => {
                                        // Check if we already have the opposite token
                                        let opposite_balance = match self.api.check_balance_only(&opposite_token_id).await {
                                            Ok(balance) => {
                                                let balance_decimal = balance / rust_decimal::Decimal::from(1_000_000u64);
                                                f64::try_from(balance_decimal).unwrap_or(0.0)
                                            }
                                            Err(_) => 0.0,
                                        };
                                        
                                        if opposite_balance > 0.000001 {
                                            // We already have the opposite token - place limit sell order
                                            let opposite_sell_price = (1.0 - stop_loss_price) + 0.1; // e.g., (1.0 - 0.80) + 0.1 = 0.30
                                            
                                            crate::log_println!("   📤 Placing limit sell for opposite token (already have it):");
                                            crate::log_println!("      Token: {}", opposite_token_type.display_name());
                                            crate::log_println!("      Limit Price: ${:.6}", opposite_sell_price);
                                            crate::log_println!("      Balance: {:.6} shares", opposite_balance);
                                            
                                            use crate::models::OrderRequest;
                                            let opposite_sell_order = OrderRequest {
                                                token_id: opposite_token_id.clone(),
                                                side: "SELL".to_string(),
                                                size: format!("{:.2}", opposite_balance),
                                                price: format!("{:.2}", opposite_sell_price),
                                                order_type: "LIMIT".to_string(),
                                            };
                                            
                                            match self.api.place_order(&opposite_sell_order).await {
                                                Ok(response) => {
                                                    crate::log_println!("   ✅ LIMIT SELL ORDER PLACED FOR OPPOSITE TOKEN");
                                                    crate::log_println!("      Order ID: {:?}", response.order_id);
                                                    crate::log_println!("      Limit Price: ${:.6}", opposite_sell_price);
                                                    
                                                    let order_id_str = response.order_id.as_ref()
                                                        .map(|id| format!("{:?}", id))
                                                        .unwrap_or_else(|| "N/A".to_string());
                                                    let sell_event = format!(
                                                        "LIMIT SELL ORDER (OPPOSITE AFTER STOP-LOSS) | Market: {} | Period: {} | Token: {} | Limit Price: ${:.6} | Size: {:.6} | Order ID: {}",
                                                        opposite_token_type.display_name(),
                                                        trade.market_timestamp,
                                                        &opposite_token_id[..16],
                                                        opposite_sell_price,
                                                        opposite_balance,
                                                        order_id_str
                                                    );
                                                    crate::log_trading_event(&sell_event);
                                                    
                                                    // Create a PendingTrade entry to track this opposite token limit sell order
                                                    let opposite_trade = PendingTrade {
                                                        token_id: opposite_token_id.clone(),
                                                        condition_id: trade.condition_id.clone(),
                                                        token_type: opposite_token_type.clone(),
                                                        order_id: response.order_id.clone(),
                                                        investment_amount: opposite_balance * opposite_buy_price,
                                                        units: opposite_balance,
                                                        purchase_price: opposite_buy_price,
                                                        sell_price: opposite_sell_price,
                                                        timestamp: std::time::Instant::now(),
                                                        market_timestamp: trade.market_timestamp,
                                                        sold: false,
                                                        confirmed_balance: Some(opposite_balance),
                                                        buy_order_confirmed: true,
                                                        limit_sell_orders_placed: true,
                                                        no_sell: false,
                                                        claim_on_closure: false,
                                                        sell_attempts: 0,
                                                        redemption_attempts: 0,
                                                        redemption_abandoned: false,
                                                    };
                                                    
                                                    let opposite_trade_key = format!("{}_opposite_{}", trade.market_timestamp, opposite_token_id);
                                                    let mut pending = self.pending_trades.lock().await;
                                                    pending.insert(opposite_trade_key, opposite_trade);
                                                    drop(pending);
                                                    
                                                    crate::log_println!("   📊 Tracking opposite token limit sell order (will monitor for fill)");
                                                }
                                                Err(e) => {
                                                    eprintln!("   ❌ FAILED TO PLACE LIMIT SELL FOR OPPOSITE TOKEN: {}", e);
                                                    warn!("Failed to place limit sell for opposite token: {}", e);
                                                }
                                            }
                                        } else {
                                            // We don't have the opposite token yet - place limit buy order
                                            // Use the same number of shares as the bought token we just sold
                                            let opposite_buy_size = units_to_sell; // Same number of shares
                                            
                                            crate::log_println!("   📥 Placing limit buy for opposite token (hedge):");
                                            crate::log_println!("      Token: {}", opposite_token_type.display_name());
                                            crate::log_println!("      Limit Price: ${:.6}", opposite_buy_price);
                                            crate::log_println!("      Size: {:.6} shares (same as sold token)", opposite_buy_size);
                                            
                                            use crate::models::OrderRequest;
                                            let opposite_buy_order = OrderRequest {
                                                token_id: opposite_token_id.clone(),
                                                side: "BUY".to_string(),
                                                size: format!("{:.2}", opposite_buy_size),
                                                price: format!("{:.2}", opposite_buy_price),
                                                order_type: "LIMIT".to_string(),
                                            };
                                            
                                            match self.api.place_order(&opposite_buy_order).await {
                                                Ok(response) => {
                                                    crate::log_println!("   ✅ LIMIT BUY ORDER PLACED FOR OPPOSITE TOKEN");
                                                    crate::log_println!("      Order ID: {:?}", response.order_id);
                                                    crate::log_println!("      Limit Price: ${:.6}", opposite_buy_price);
                                                    crate::log_println!("      Size: {:.6} shares", opposite_buy_size);
                                                    
                                                    let order_id_str = response.order_id.as_ref()
                                                        .map(|id| format!("{:?}", id))
                                                        .unwrap_or_else(|| "N/A".to_string());
                                                    let buy_event = format!(
                                                        "LIMIT BUY ORDER (OPPOSITE AFTER STOP-LOSS) | Market: {} | Period: {} | Token: {} | Limit Price: ${:.6} | Size: {:.6} | Order ID: {}",
                                                        opposite_token_type.display_name(),
                                                        trade.market_timestamp,
                                                        &opposite_token_id[..16],
                                                        opposite_buy_price,
                                                        opposite_buy_size,
                                                        order_id_str
                                                    );
                                                    crate::log_trading_event(&buy_event);
                                                    
                                                    // Create a PendingTrade entry to track this opposite token limit buy order
                                                    let opposite_trade = PendingTrade {
                                                        token_id: opposite_token_id.clone(),
                                                        condition_id: trade.condition_id.clone(),
                                                        token_type: opposite_token_type.clone(),
                                                        order_id: response.order_id.clone(),
                                                        investment_amount: opposite_buy_size * opposite_buy_price,
                                                        units: opposite_buy_size,
                                                        purchase_price: opposite_buy_price,
                                                        sell_price: (1.0 - stop_loss_price) + 0.1, // Will sell at 0.30 when filled
                                                        timestamp: std::time::Instant::now(),
                                                        market_timestamp: trade.market_timestamp,
                                                        sold: false,
                                                        confirmed_balance: Some(0.0), // Not filled yet
                                                        buy_order_confirmed: false, // Limit buy not confirmed yet
                                                        limit_sell_orders_placed: false, // Will place sell order after buy fills
                                                        no_sell: false,
                                                        claim_on_closure: false,
                                                        sell_attempts: 0,
                                                        redemption_attempts: 0,
                                                        redemption_abandoned: false,
                                                    };
                                                    
                                                    let opposite_trade_key = format!("{}_opposite_limit_{}", trade.market_timestamp, opposite_token_id);
                                                    let mut pending = self.pending_trades.lock().await;
                                                    pending.insert(opposite_trade_key, opposite_trade);
                                                    drop(pending);
                                                    
                                                    crate::log_println!("   📊 Tracking opposite token limit buy order (will monitor for fill and place sell order)");
                                                }
                                                Err(e) => {
                                                    eprintln!("   ❌ FAILED TO PLACE LIMIT BUY ORDER FOR OPPOSITE TOKEN: {}", e);
                                                    warn!("Failed to place limit buy order for opposite token: {}", e);
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("   ❌ FAILED TO GET OPPOSITE TOKEN ID: {}", e);
                                        warn!("Failed to get opposite token ID: {}", e);
                                    }
                                }
                                
                                // Mark trade as sold
                                {
                                    let mut pending = self.pending_trades.lock().await;
                                    if let Some(t) = pending.get_mut(key.as_str()) {
                                        t.sold = true;
                                    }
                                }
                                
                                continue; // Move to next trade after handling stop-loss
                            }
                            Err(e) => {
                                eprintln!("   ❌ FAILED TO EXECUTE STOP-LOSS SELL: {}", e);
                                warn!("Failed to execute stop-loss sell: {}", e);
                                continue; // Move to next trade
                            }
                        }
                    }
                }
                
                // Skip old stop-loss logic for new strategy trades
                continue;
            }
            
            // OLD STOP-LOSS LOGIC (for non-new-strategy trades) - keep for backward compatibility
            // Skip stop-loss for limit order version trades (identified by limit_sell_orders_placed flag)
            // Also skip stop-loss for all hedge trades (individual, standard, and multi-market)
            if trade.limit_sell_orders_placed || key.contains("_limit") || key.contains("_individual_hedge") || key.contains("_standard_hedge") || key.contains("_dual_limit_hedge") {
                // This is a limit order version trade or hedge trade - skip stop-loss monitoring
                continue;
            }
            
            // Check for stop-loss condition first (before checking for profit sell)
            // Stop-loss: sell if price drops below stop_loss_price to limit losses
            // Note: If stop-loss sell fails, keep retrying until sold OR price recovers above stop_loss_price
            if let Some(stop_loss_price) = self.config.stop_loss_price {
                // Only trigger stop-loss if price is below threshold
                // If price recovers above stop_loss_price, cancel stop-loss attempt
                if current_ask_price < stop_loss_price {
                    // CRITICAL: Re-check actual balance before selling
                    let actual_balance = match self.api.check_balance_allowance(&trade.token_id).await {
                        Ok((balance, _)) => {
                            // Conditional tokens use 1e6 as base unit (like USDC)
                            // Convert from smallest unit to actual shares
                            let balance_decimal = balance / rust_decimal::Decimal::from(1_000_000u64);
                            let balance_f64 = f64::try_from(balance_decimal).unwrap_or(0.0);
                            if balance_f64 > 0.0 && (trade.units == 0.0 || balance_f64 > trade.units) {
                                crate::log_println!("   🔄 Updating units from stored {:.6} to actual balance {:.6} shares", trade.units, balance_f64);
                                balance_f64
                            } else {
                                trade.units
                            }
                        }
                        Err(e) => {
                            warn!("⚠️  Failed to re-check balance before stop-loss sell: {}. Using stored units: {:.6}", e, trade.units);
                            trade.units
                        }
                    };
                    
                    if actual_balance == 0.0 {
                        warn!("⚠️  Skipping stop-loss sell - actual balance is 0.000000 shares.");
                        continue;
                    }
                    
                    let units_to_sell = actual_balance;
                    
                    crate::log_println!("═══════════════════════════════════════════════════════════");
                    crate::log_println!("🛑 STOP-LOSS TRIGGERED - NEW STRATEGY");
                    crate::log_println!("═══════════════════════════════════════════════════════════");
                    crate::log_println!("⚠️  Price dropped to stop-loss threshold!");
                    crate::log_println!("💰 Current ASK Price: ${:.6}", current_ask_price);
                    crate::log_println!("🛑 Stop-Loss Price: ${:.6}", stop_loss_price);
                    crate::log_println!("📊 Purchase Price: ${:.6}", trade.purchase_price);
                    crate::log_println!("   Condition: {} <= {}", current_ask_price, stop_loss_price);
                    crate::log_println!("");
                    crate::log_println!("📊 Trade Details:");
                    crate::log_println!("   Token Type: {}", trade.token_type.display_name());
                    crate::log_println!("   Token ID: {}", trade.token_id);
                    crate::log_println!("   Units to sell: {:.6}", units_to_sell);
                    crate::log_println!("");
                    crate::log_println!("🔄 Executing stop-loss sell and placing opposite token order...");
                    
                    // Execute stop-loss sell
                    match self.execute_sell(&key, &mut trade, units_to_sell, current_ask_price, Some("FAK"), true).await {
                        Ok(_) => {
                            crate::log_println!("   ✅ STOP-LOSS SELL EXECUTED");
                            
                            // Place limit buy order for opposite token at (1 - stop_loss_price)
                            // This ensures we have the hedge even if the earlier limit buy didn't fill
                            let opposite_token_type = trade.token_type.opposite();
                            let opposite_buy_price = 1.0 - stop_loss_price; // e.g., 1.0 - 0.80 = 0.20
                            
                            match self.get_opposite_token_id(&trade.token_type, &trade.condition_id).await {
                                Ok(opposite_token_id) => {
                                    // Check if we already have the opposite token
                                    let opposite_balance = match self.api.check_balance_only(&opposite_token_id).await {
                                        Ok(balance) => {
                                            let balance_decimal = balance / rust_decimal::Decimal::from(1_000_000u64);
                                            f64::try_from(balance_decimal).unwrap_or(0.0)
                                        }
                                        Err(_) => 0.0,
                                    };
                                    
                                    if opposite_balance > 0.000001 {
                                        // We already have the opposite token - place limit sell order
                                        let opposite_sell_price = (1.0 - stop_loss_price) + 0.1; // e.g., (1.0 - 0.80) + 0.1 = 0.30
                                        
                                        crate::log_println!("   📤 Placing limit sell for opposite token (already have it):");
                                        crate::log_println!("      Token: {}", opposite_token_type.display_name());
                                        crate::log_println!("      Limit Price: ${:.6}", opposite_sell_price);
                                        crate::log_println!("      Balance: {:.6} shares", opposite_balance);
                                        
                                        use crate::models::OrderRequest;
                                        let opposite_sell_order = OrderRequest {
                                            token_id: opposite_token_id.clone(),
                                            side: "SELL".to_string(),
                                            size: format!("{:.2}", opposite_balance),
                                            price: format!("{:.2}", opposite_sell_price),
                                            order_type: "LIMIT".to_string(),
                                        };
                                        
                                        match self.api.place_order(&opposite_sell_order).await {
                                            Ok(response) => {
                                                crate::log_println!("   ✅ LIMIT SELL ORDER PLACED FOR OPPOSITE TOKEN");
                                                crate::log_println!("      Order ID: {:?}", response.order_id);
                                                crate::log_println!("      Limit Price: ${:.6}", opposite_sell_price);
                                                
                                                let order_id_str = response.order_id.as_ref()
                                                    .map(|id| format!("{:?}", id))
                                                    .unwrap_or_else(|| "N/A".to_string());
                                                let sell_event = format!(
                                                    "LIMIT SELL ORDER (OPPOSITE AFTER STOP-LOSS) | Market: {} | Period: {} | Token: {} | Limit Price: ${:.6} | Size: {:.6} | Order ID: {}",
                                                    opposite_token_type.display_name(),
                                                    trade.market_timestamp,
                                                    &opposite_token_id[..16],
                                                    opposite_sell_price,
                                                    opposite_balance,
                                                    order_id_str
                                                );
                                                crate::log_trading_event(&sell_event);
                                                
                                                // Create a PendingTrade entry to track this opposite token limit sell order
                                                let opposite_trade = PendingTrade {
                                                    token_id: opposite_token_id.clone(),
                                                    condition_id: trade.condition_id.clone(),
                                                    token_type: opposite_token_type.clone(),
                                                    order_id: response.order_id.clone(),
                                                    investment_amount: opposite_balance * opposite_buy_price,
                                                    units: opposite_balance,
                                                    purchase_price: opposite_buy_price,
                                                    sell_price: opposite_sell_price,
                                                    timestamp: std::time::Instant::now(),
                                                    market_timestamp: trade.market_timestamp,
                                                    sold: false,
                                                    confirmed_balance: Some(opposite_balance),
                                                    buy_order_confirmed: true,
                                                    limit_sell_orders_placed: true,
                                                    no_sell: false,
                                                    claim_on_closure: false,
                                                    sell_attempts: 0,
                                                    redemption_attempts: 0,
                                                    redemption_abandoned: false,
                                                };
                                                
                                                let opposite_trade_key = format!("{}_opposite_{}", trade.market_timestamp, opposite_token_id);
                                                let mut pending = self.pending_trades.lock().await;
                                                pending.insert(opposite_trade_key, opposite_trade);
                                                drop(pending);
                                                
                                                crate::log_println!("   📊 Tracking opposite token limit sell order (will monitor for fill)");
                                            }
                                            Err(e) => {
                                                eprintln!("   ❌ FAILED TO PLACE LIMIT SELL FOR OPPOSITE TOKEN: {}", e);
                                                warn!("Failed to place limit sell for opposite token: {}", e);
                                            }
                                        }
                                    } else {
                                        // We don't have the opposite token yet - place limit buy order
                                        // Use the same number of shares as the bought token we just sold
                                        let opposite_buy_size = units_to_sell; // Same number of shares
                                        
                                        crate::log_println!("   📥 Placing limit buy for opposite token (hedge):");
                                        crate::log_println!("      Token: {}", opposite_token_type.display_name());
                                        crate::log_println!("      Limit Price: ${:.6}", opposite_buy_price);
                                        crate::log_println!("      Size: {:.6} shares (same as sold token)", opposite_buy_size);
                                        
                                        use crate::models::OrderRequest;
                                        let opposite_buy_order = OrderRequest {
                                            token_id: opposite_token_id.clone(),
                                            side: "BUY".to_string(),
                                            size: format!("{:.2}", opposite_buy_size),
                                            price: format!("{:.2}", opposite_buy_price),
                                            order_type: "LIMIT".to_string(),
                                        };
                                        
                                        match self.api.place_order(&opposite_buy_order).await {
                                            Ok(response) => {
                                                crate::log_println!("   ✅ LIMIT BUY ORDER PLACED FOR OPPOSITE TOKEN");
                                                crate::log_println!("      Order ID: {:?}", response.order_id);
                                                crate::log_println!("      Limit Price: ${:.6}", opposite_buy_price);
                                                crate::log_println!("      Size: {:.6} shares", opposite_buy_size);
                                                
                                                let order_id_str = response.order_id.as_ref()
                                                    .map(|id| format!("{:?}", id))
                                                    .unwrap_or_else(|| "N/A".to_string());
                                                let buy_event = format!(
                                                    "LIMIT BUY ORDER (OPPOSITE AFTER STOP-LOSS) | Market: {} | Period: {} | Token: {} | Limit Price: ${:.6} | Size: {:.6} | Order ID: {}",
                                                    opposite_token_type.display_name(),
                                                    trade.market_timestamp,
                                                    &opposite_token_id[..16],
                                                    opposite_buy_price,
                                                    opposite_buy_size,
                                                    order_id_str
                                                );
                                                crate::log_trading_event(&buy_event);
                                                
                                                // Create a PendingTrade entry to track this opposite token limit buy order
                                                let opposite_trade = PendingTrade {
                                                    token_id: opposite_token_id.clone(),
                                                    condition_id: trade.condition_id.clone(),
                                                    token_type: opposite_token_type.clone(),
                                                    order_id: response.order_id.clone(),
                                                    investment_amount: opposite_buy_size * opposite_buy_price,
                                                    units: opposite_buy_size,
                                                    purchase_price: opposite_buy_price,
                                                    sell_price: (1.0 - stop_loss_price) + 0.1, // Will sell at 0.30 when filled
                                                    timestamp: std::time::Instant::now(),
                                                    market_timestamp: trade.market_timestamp,
                                                    sold: false,
                                                    confirmed_balance: Some(0.0), // Not filled yet
                                                    buy_order_confirmed: false, // Limit buy not confirmed yet
                                                    limit_sell_orders_placed: false, // Will place sell order after buy fills
                                                    no_sell: false,
                                                    claim_on_closure: false,
                                                    sell_attempts: 0,
                                                    redemption_attempts: 0,
                                                    redemption_abandoned: false,
                                                };
                                                
                                                let opposite_trade_key = format!("{}_opposite_limit_{}", trade.market_timestamp, opposite_token_id);
                                                let mut pending = self.pending_trades.lock().await;
                                                pending.insert(opposite_trade_key, opposite_trade);
                                                drop(pending);
                                                
                                                crate::log_println!("   📊 Tracking opposite token limit buy order (will monitor for fill and place sell order)");
                                            }
                                            Err(e) => {
                                                eprintln!("   ❌ FAILED TO PLACE LIMIT BUY ORDER FOR OPPOSITE TOKEN: {}", e);
                                                warn!("Failed to place limit buy order for opposite token: {}", e);
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    eprintln!("   ❌ FAILED TO GET OPPOSITE TOKEN ID: {}", e);
                                    warn!("Failed to get opposite token ID: {}", e);
                                }
                            }
                            
                            // Mark trade as sold
                            {
                                let mut pending = self.pending_trades.lock().await;
                                if let Some(t) = pending.get_mut(key.as_str()) {
                                    t.sold = true;
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("   ❌ FAILED TO EXECUTE STOP-LOSS SELL: {}", e);
                            warn!("Failed to execute stop-loss sell: {}", e);
                        }
                    }
                    
                    continue; // Move to next trade after handling stop-loss
                }
            }
            
            // OLD STOP-LOSS LOGIC (for non-limit-order-version trades) - keep for backward compatibility
            // Skip stop-loss for limit order version trades (identified by limit_sell_orders_placed flag)
            // Also skip stop-loss for all hedge trades (individual, standard, and multi-market)
            if trade.limit_sell_orders_placed || key.contains("_limit") || key.contains("_individual_hedge") || key.contains("_standard_hedge") || key.contains("_dual_limit_hedge") {
                // This is a limit order version trade or hedge trade - skip old stop-loss monitoring
                continue;
            }
            
            // Check for stop-loss condition first (before checking for profit sell)
            // Stop-loss: sell if price drops below stop_loss_price to limit losses
            // Note: If stop-loss sell fails, keep retrying until sold OR price recovers above stop_loss_price
            if let Some(stop_loss_price) = self.config.stop_loss_price {
                // Only trigger stop-loss if price is below threshold
                // If price recovers above stop_loss_price, cancel stop-loss attempt
                if current_ask_price < stop_loss_price {
                    // CRITICAL: Re-check actual balance before selling
                    let actual_balance = match self.api.check_balance_allowance(&trade.token_id).await {
                        Ok((balance, _)) => {
                            // Conditional tokens use 1e6 as base unit (like USDC)
                            // Convert from smallest unit to actual shares
                            let balance_decimal = balance / rust_decimal::Decimal::from(1_000_000u64);
                            let balance_f64 = f64::try_from(balance_decimal).unwrap_or(0.0);
                            if balance_f64 > 0.0 && (trade.units == 0.0 || balance_f64 > trade.units) {
                                crate::log_println!("   🔄 Updating units from stored {:.6} to actual balance {:.6} shares", trade.units, balance_f64);
                                balance_f64
                            } else {
                                trade.units
                            }
                        }
                        Err(e) => {
                            warn!("⚠️  Failed to re-check balance before stop-loss sell: {}. Using stored units: {:.6}", e, trade.units);
                            trade.units
                        }
                    };
                    
                    if actual_balance == 0.0 {
                        warn!("⚠️  Skipping stop-loss sell - actual balance is 0.000000 shares.");
                        continue;
                    }
                    
                    let units_to_sell = actual_balance;
                    let loss = (current_ask_price - trade.purchase_price) * units_to_sell;
                    
                    crate::log_println!("═══════════════════════════════════════════════════════════");
                    crate::log_println!("🛑 STOP-LOSS TRIGGERED");
                    crate::log_println!("═══════════════════════════════════════════════════════════");
                    crate::log_println!("⚠️  Price dropped below stop-loss threshold!");
                    crate::log_println!("💰 Current ASK Price: ${:.6}", current_ask_price);
                    crate::log_println!("🛑 Stop-Loss Price: ${:.6}", stop_loss_price);
                    crate::log_println!("📊 Purchase Price: ${:.6}", trade.purchase_price);
                    crate::log_println!("   Condition: {} < {}", current_ask_price, stop_loss_price);
                    crate::log_println!("");
                    crate::log_println!("📊 Trade Details:");
                    crate::log_println!("   Token Type: {}", trade.token_type.display_name());
                    crate::log_println!("   Token ID: {}", trade.token_id);
                    crate::log_println!("   Units to sell: {:.6}", units_to_sell);
                    crate::log_println!("   Purchase price: ${:.6}", trade.purchase_price);
                    crate::log_println!("   Current ASK price: ${:.6}", current_ask_price);
                    crate::log_println!("   Expected revenue: ${:.6}", current_ask_price * units_to_sell);
                    crate::log_println!("   Original cost: ${:.6}", trade.purchase_price * units_to_sell);
                    crate::log_println!("   Loss: ${:.6}", loss);
                    crate::log_println!("");
                    crate::log_println!("   🔄 Executing stop-loss sell to limit losses...");
                    
                    // CRITICAL: Refresh backend's cached allowance before selling
                    // Even though setApprovalForAll is set on-chain, the backend cache might be stale
                    // The API checks the cached allowance, not the on-chain approval directly
                    crate::log_println!("   🔄 Refreshing backend's cached allowance (required for API to recognize approval)...");
                    if let Err(e) = self.api.update_balance_allowance_for_sell(&trade.token_id).await {
                        crate::log_println!("   ⚠️  Failed to refresh allowance cache: {} (proceeding anyway)", e);
                    } else {
                        crate::log_println!("   ✅ Allowance cache refreshed - waiting 500ms for backend to process...");
                        // Give backend a moment to process the cache update
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    }
                    
                    // Optimized retry loop for stop-loss: try selling ASAP, retry immediately on failure
                    // Stop if price recovers above stop-loss threshold (safe level)
                    let max_retry_attempts = 20; // Maximum retry attempts
                    let retry_delay_ms = 1500; // 1.5 seconds between retries
                    let mut sell_succeeded = false;
                    let mut last_price = current_ask_price;
                    
                    for attempt in 1..=max_retry_attempts {
                        crate::log_println!("   🔄 Stop-loss sell attempt #{}/{} (price: ${:.6})", attempt, max_retry_attempts, last_price);
                        
                        // CRITICAL: Refresh backend's cached allowance before each retry attempt
                        // The backend checks cached allowance, not on-chain approval directly
                        if attempt > 1 {
                            crate::log_println!("   🔄 Refreshing backend's cached allowance before retry...");
                            if let Err(e) = self.api.update_balance_allowance_for_sell(&trade.token_id).await {
                                crate::log_println!("   ⚠️  Failed to refresh allowance cache: {} (retrying anyway)", e);
                            } else {
                                crate::log_println!("   ✅ Allowance cache refreshed - waiting 500ms for backend to process...");
                                // Give backend a moment to process the cache update
                                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                            }
                        }
                        
                        // Execute stop-loss sell with FAK (Fill-and-Kill) to allow partial fills
                        let sell_result = self.execute_sell(&key, &mut trade, units_to_sell, last_price, Some("FAK"), true).await;
                        
                        match sell_result {
                            Ok(_) => {
                                sell_succeeded = true;
                                break; // Success - exit retry loop
                            }
                            Err(e) => {
                                let error_str = e.to_string();
                                
                                // Check if price recovered above stop-loss threshold (safe level)
                                // If price recovers, stop retrying
                                let current_price_check = match self.api.get_price(&trade.token_id, "SELL").await {
                                    Ok(p) => f64::try_from(p).unwrap_or(last_price),
                                    Err(_) => last_price, // Use last known price if fetch fails
                                };
                                
                                // Stop retrying if price recovered above stop-loss threshold
                                if let Some(stop_loss_threshold) = self.config.stop_loss_price {
                                    if current_price_check >= stop_loss_threshold {
                                        crate::log_println!("   ⏸️  Price recovered above stop-loss threshold (${:.6} >= ${:.6}) - stopping retry", 
                                            current_price_check, stop_loss_threshold);
                                        crate::log_println!("   💡 Position is safe - will monitor for profit sell or stop-loss again");
                                        
                                        // Update trade (don't mark as sold, keep monitoring)
                                        let mut pending = self.pending_trades.lock().await;
                                        if let Some(t) = pending.get_mut(key.as_str()) {
                                            *t = trade.clone();
                                        }
                                        drop(pending);
                                        
                                        // Log the failure
                                        let market_name = trade.token_type.display_name();
                                        let error_msg = format!("{:?}", e);
                                        let simple_error = if error_msg.contains("not enough balance") || error_msg.contains("allowance") {
                                            "not enough balance / allowance".to_string()
                                        } else if error_msg.contains("No opposing orders") || error_msg.contains("no orders found") {
                                            "no liquidity / no buyers".to_string()
                                        } else if error_msg.contains("couldn't be fully filled") {
                                            "insufficient liquidity".to_string()
                                        } else {
                                            error_msg.lines().next().unwrap_or("Sell failed").to_string()
                                        };
                                        
                                        let sell_event = format!(
                                            "SELL ORDER (STOP-LOSS) | Market: {} | Period: {} | Price: ${:.6} | Units: {:.6} | Revenue: ${:.6} | Loss: ${:.6} | Status: FAILED | Attempt: {} | Error: {} | Stopped: Price recovered",
                                            market_name,
                                            trade.market_timestamp,
                                            current_price_check,
                                            units_to_sell,
                                            current_price_check * units_to_sell,
                                            loss,
                                            attempt,
                                            simple_error
                                        );
                                        crate::log_trading_event(&sell_event);
                                        
                                        break; // Stop retrying - price recovered
                                    }
                                }
                                
                                last_price = current_price_check;
                                
                                // Log failure but continue retrying
                                if attempt % 5 == 0 || attempt == 1 {
                                    crate::log_println!("   ❌ Stop-loss sell attempt {} failed: {}", attempt, error_str);
                                    if attempt < max_retry_attempts {
                                        crate::log_println!("   ⏳ Retrying in {}ms... (price: ${:.6})", retry_delay_ms, last_price);
                                    }
                                }
                                
                                // Wait before next retry (except on last attempt)
                                if attempt < max_retry_attempts {
                                    tokio::time::sleep(tokio::time::Duration::from_millis(retry_delay_ms)).await;
                                }
                            }
                        }
                    }
                    
                    // Handle final result
                    if !sell_succeeded {
                        // Max retries reached or price recovered - continue monitoring
                        let final_attempt = trade.sell_attempts;
                        if final_attempt >= max_retry_attempts {
                            crate::log_println!("   ⚠️  Maximum stop-loss sell attempts ({}) reached - will retry on next check", max_retry_attempts);
                        }
                        continue; // Continue to next trade - will retry on next check cycle
                    }
                    
                    // Stop-loss sell succeeded - mark as sold and remove from pending trades
                    trade.sold = true;
                    
                    // Mark cycle completed for this token type (requires reset before next buy)
                    if let Some(ref detector) = self.detector {
                        detector.mark_cycle_completed(trade.token_type.clone()).await;
                    }
                    
                    // Remove from HashMap since trade is sold
                    let mut pending = self.pending_trades.lock().await;
                    pending.remove(&key);
                    drop(pending);
                    
                    crate::log_println!("   ✅ Stop-loss sell executed successfully");
                    crate::log_println!("   💡 Position closed - can re-buy if price goes back up over ${:.6}", self.config.trigger_price);
                    continue; // Move to next trade
                }
            }
            
            // Check if we've reached the sell price (0.99 or 1.0)
            // Also check if price is >= 1.0 (market resolution - token is worth $1)
            if current_ask_price >= trade.sell_price || current_ask_price >= 1.0 {
                // CRITICAL: Re-check actual balance before selling
                // The stored units might be 0 if balance check failed initially, but tokens may have arrived later
                let actual_balance = match self.api.check_balance_allowance(&trade.token_id).await {
                    Ok((balance, _)) => {
                        // Conditional tokens use 1e6 as base unit (like USDC)
                        // Convert from smallest unit to actual shares
                        let balance_decimal = balance / rust_decimal::Decimal::from(1_000_000u64);
                        let balance_f64 = f64::try_from(balance_decimal).unwrap_or(0.0);
                        if balance_f64 > 0.0 && (trade.units == 0.0 || balance_f64 > trade.units) {
                            crate::log_println!("   🔄 Updating units from stored {:.6} to actual balance {:.6} shares", trade.units, balance_f64);
                            balance_f64
                        } else {
                            trade.units // Use stored units if they're valid
                        }
                    }
                    Err(e) => {
                        warn!("⚠️  Failed to re-check balance before selling: {}. Using stored units: {:.6}", e, trade.units);
                        trade.units
                    }
                };
                
                // Skip if we still have 0 units after re-checking
                if actual_balance == 0.0 {
                    warn!("⚠️  Skipping sell - actual balance is 0.000000 shares. Trade may not have executed.");
                    continue;
                }
                
                let units_to_sell = actual_balance; // Use actual balance, not stored units
                
                crate::log_println!("═══════════════════════════════════════════════════════════");
                crate::log_println!("📈 SELL CONDITION MET");
                crate::log_println!("═══════════════════════════════════════════════════════════");
                crate::log_println!("💰 Current Price: ${:.6}", current_ask_price);
                crate::log_println!("🎯 Target Price: ${:.6}", trade.sell_price);
                crate::log_println!("   Condition: {} >= {}", current_ask_price, trade.sell_price);
                crate::log_println!("");
                crate::log_println!("📊 Trade Details:");
                crate::log_println!("   Token Type: {}", trade.token_type.display_name());
                crate::log_println!("   Token ID: {}", trade.token_id);
                crate::log_println!("   Units to sell: {:.6}", units_to_sell);
                crate::log_println!("   Purchase price: ${:.6}", trade.purchase_price);
                crate::log_println!("   Current ASK price: ${:.6}", current_ask_price);
                crate::log_println!("   Expected revenue: ${:.6}", current_ask_price * units_to_sell);
                crate::log_println!("   Original cost: ${:.6}", trade.purchase_price * units_to_sell);
                let profit = (current_ask_price - trade.purchase_price) * units_to_sell;
                crate::log_println!("   Expected profit: ${:.6}", profit);
                crate::log_println!("");
                
                // Optimized retry loop: try selling ASAP, retry immediately on failure
                // Stop if price recovers to safe level (drops below sell_price)
                let max_retry_attempts = 20; // Maximum retry attempts
                let retry_delay_ms = 1500; // 1.5 seconds between retries
                let mut sell_succeeded = false;
                let mut last_price = current_ask_price;
                
                for attempt in 1..=max_retry_attempts {
                    trade.sell_attempts = attempt;
                    crate::log_println!("   🔄 Sell attempt #{}/{} (price: ${:.6})", attempt, max_retry_attempts, last_price);
                    
                    // CRITICAL: Refresh backend's cached allowance before each retry attempt
                    // The backend checks cached allowance, not on-chain approval directly
                    if attempt > 1 {
                        crate::log_println!("   🔄 Refreshing backend's cached allowance before retry...");
                        if let Err(e) = self.api.update_balance_allowance_for_sell(&trade.token_id).await {
                            crate::log_println!("   ⚠️  Failed to refresh allowance cache: {} (retrying anyway)", e);
                        } else {
                            crate::log_println!("   ✅ Allowance cache refreshed - waiting 500ms for backend to process...");
                            // Give backend a moment to process the cache update
                            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                        }
                    }
                    
                    // Execute sell with FAK (Fill-and-Kill) to allow partial fills
                    let sell_result = self.execute_sell(&key, &mut trade, units_to_sell, last_price, Some("FAK"), false).await;
                    
                    match sell_result {
                        Ok(_) => {
                            sell_succeeded = true;
                            break; // Success - exit retry loop
                        }
                        Err(e) => {
                            let error_str = e.to_string();
                            
                            // Check if price recovered to safe level (dropped below sell_price)
                            // If price drops significantly, stop retrying
                            let current_price_check = match self.api.get_price(&trade.token_id, "SELL").await {
                                Ok(p) => f64::try_from(p).unwrap_or(last_price),
                                Err(_) => last_price, // Use last known price if fetch fails
                            };
                            
                            // Stop retrying if price dropped below sell_price (recovered to safe level)
                            if current_price_check < trade.sell_price {
                                crate::log_println!("   ⏸️  Price recovered to safe level (${:.6} < ${:.6}) - stopping retry", 
                                    current_price_check, trade.sell_price);
                                crate::log_println!("   💡 Will retry when price reaches ${:.6} again", trade.sell_price);
                                
                                // Update trade with current attempt count
                let mut pending = self.pending_trades.lock().await;
                if let Some(t) = pending.get_mut(&key) {
                                    t.sell_attempts = attempt;
                }
                drop(pending);
                                
                                // Log the failure
                                let market_name = trade.token_type.display_name();
                                let error_msg = format!("{:?}", e);
                                let simple_error = if error_msg.contains("not enough balance") || error_msg.contains("allowance") {
                                    "not enough balance / allowance".to_string()
                                } else if error_msg.contains("No opposing orders") || error_msg.contains("no orders found") {
                                    "no liquidity / no buyers".to_string()
                                } else if error_msg.contains("couldn't be fully filled") {
                                    "insufficient liquidity".to_string()
                                } else {
                                    error_msg.lines()
                                        .find(|line| !line.trim().is_empty() && !line.contains("Troubleshooting"))
                                        .unwrap_or("Sell failed")
                                        .to_string()
                                };
                                
                                let sell_event = format!(
                                    "SELL ORDER (PROFIT) | Market: {} | Period: {} | Price: ${:.6} | Units: {:.6} | Revenue: ${:.6} | Profit: ${:.6} | Status: FAILED | Attempt: {}/{} | Error: {} | Stopped: Price recovered",
                                    market_name,
                                    trade.market_timestamp,
                                    current_price_check,
                                    units_to_sell,
                                    current_price_check * units_to_sell,
                                    (current_price_check - trade.purchase_price) * units_to_sell,
                                    attempt,
                                    max_retry_attempts,
                                    simple_error
                                );
                                crate::log_trading_event(&sell_event);
                                
                                break; // Stop retrying - price recovered
                            }
                            
                            last_price = current_price_check;
                            
                            // Log failure but continue retrying
                            if attempt % 5 == 0 || attempt == 1 {
                                crate::log_println!("   ❌ Sell attempt {} failed: {}", attempt, error_str);
                                if attempt < max_retry_attempts {
                                    crate::log_println!("   ⏳ Retrying in {}ms... (price: ${:.6})", retry_delay_ms, last_price);
                                }
                            }
                            
                            // Wait before next retry (except on last attempt)
                            if attempt < max_retry_attempts {
                                tokio::time::sleep(tokio::time::Duration::from_millis(retry_delay_ms)).await;
                            }
                        }
                    }
                }
                
                // Handle final result
                if !sell_succeeded {
                    // Max retries reached or price recovered
                    if trade.sell_attempts >= max_retry_attempts {
                        crate::log_println!("   ⚠️  Maximum sell attempts ({}) reached", max_retry_attempts);
                        crate::log_println!("   📋 Marking trade for claim at market closure");
                        
                        trade.claim_on_closure = true;
                        let mut pending = self.pending_trades.lock().await;
                        if let Some(t) = pending.get_mut(key.as_str()) {
                            *t = trade.clone();
                        }
                        drop(pending);
                        
                        crate::log_println!("   ✅ Trade will be claimed/redeemed when market closes");
                    }
                    continue; // Move to next trade
                }
                
                // Mark as sold and remove from pending trades - execute_sell already logged the order with order ID
                trade.sold = true;
                
                // Mark cycle completed for this token type (requires reset before next buy)
                if let Some(ref detector) = self.detector {
                    detector.mark_cycle_completed(trade.token_type.clone()).await;
                }
                
                // Remove from HashMap since trade is sold
                let mut pending = self.pending_trades.lock().await;
                pending.remove(&key);
                drop(pending);
            }
        }
        
        Ok(())
    }

    /// Execute sell order with configurable order type
    /// order_type: None or "FAK" for Fill-and-Kill (allows partial fills), "FOK" for Fill-or-Kill
    /// Default: FAK (allows partial fills, better for limited liquidity situations)
    /// is_stop_loss: true if this is a stop-loss sell, false if it's a profit sell
    async fn execute_sell(
        &self,
        _trade_key: &str,
        trade: &PendingTrade,
        units_to_sell: f64,
        current_price: f64,
        order_type: Option<&str>,
        is_stop_loss: bool,
    ) -> Result<()> {
        let order_type_str = order_type.unwrap_or("FAK");
        let order_type_display = match order_type_str {
            "FAK" => "FAK (Fill-and-Kill - allows partial fills)",
            "FOK" => "FOK (Fill-or-Kill)",
            _ => "FAK (Fill-and-Kill - allows partial fills)",
        };
        if self.simulation_mode {
            let sell_value = current_price * units_to_sell;
            let profit = sell_value - (trade.purchase_price * units_to_sell);
            
            let mut total = self.total_profit.lock().await;
            *total += profit;
            let total_profit = *total;
            drop(total);
            
            crate::log_println!("🎮 SIMULATION MODE - Order NOT placed on exchange");
            crate::log_println!("   ✅ SIMULATION: Sell order would execute:");
            crate::log_println!("      - Token Type: {}", trade.token_type.display_name());
            crate::log_println!("      - Token: {}", &trade.token_id[..16]);
            crate::log_println!("      - Units: {:.6}", units_to_sell);
            crate::log_println!("      - Price: ${:.6}", current_price);
            crate::log_println!("      - Revenue: ${:.6}", sell_value);
            crate::log_println!("      - Cost: ${:.6}", trade.purchase_price * units_to_sell);
            crate::log_println!("      - Profit: ${:.6}", profit);
            crate::log_println!("      - Total Profit (all trades): ${:.6}", total_profit);
        } else {
            crate::log_println!("🚀 PRODUCTION MODE - Placing order on exchange");
            crate::log_println!("   Order parameters:");
            crate::log_println!("      Token Type: {}", trade.token_type.display_name());
            crate::log_println!("      Token ID: {}", trade.token_id);
            crate::log_println!("      Side: SELL");
            crate::log_println!("      Shares: {:.6} units (market order - price determined by market)", units_to_sell);
            crate::log_println!("      Type: {}", order_type_display);
            
            // Place real market sell order
            // For SELL market orders, we pass number of shares/units
            // Note: If you get "not enough balance / allowance" error, it may be because:
            // 1. Token allowance needs to be set for proxy wallet (SDK should handle this automatically)
            // 2. The shares amount might need to be converted to USD value instead
            // 
            // Calculate expected USD value for better error messages
            let expected_usd_value = current_price * units_to_sell;
            crate::log_println!("   Expected USD value: ${:.6} ({} shares × ${:.6})", 
                expected_usd_value, units_to_sell, current_price);
            
            // CRITICAL: Refresh backend's cached allowance before selling
            // Even though setApprovalForAll is set on-chain, the backend cache might be stale
            // The API checks the cached allowance, not the on-chain approval directly
            crate::log_println!("   🔄 Refreshing backend's cached allowance (required for API to recognize approval)...");
            if let Err(e) = self.api.update_balance_allowance_for_sell(&trade.token_id).await {
                crate::log_println!("   ⚠️  Failed to refresh allowance cache: {} (proceeding anyway - backend might still work)", e);
            } else {
                crate::log_println!("   ✅ Allowance cache refreshed - waiting 500ms for backend to process...");
                // Give backend a moment to process the cache update
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
            
            crate::log_println!("\n   📤 Placing SELL order...");
            match self.api.place_market_order(
                &trade.token_id,
                units_to_sell,  // Number of shares/units for SELL market orders
                "SELL",
                Some(order_type_str),
            ).await {
                Ok(response) => {
                    crate::log_println!("   ✅ ORDER PLACED SUCCESSFULLY");
                    crate::log_println!("      Order ID: {:?}", response.order_id);
                    crate::log_println!("      Status: {}", response.status);
                    if let Some(msg) = &response.message {
                        crate::log_println!("      Message: {}", msg);
                    }
                    
                    // Calculate profit/loss
                    let sell_value = current_price * units_to_sell;
                    let pnl = sell_value - (trade.purchase_price * units_to_sell);
                    let mut total = self.total_profit.lock().await;
                    *total += pnl;
                    let total_profit = *total;
                    drop(total);
                    
                    // Log structured sell order to history.toml (profit or stop-loss)
                    let market_name = trade.token_type.display_name();
                    let order_id_str = response.order_id.as_ref()
                        .map(|id| format!("{:?}", id))
                        .unwrap_or_else(|| "N/A".to_string());
                    
                    // Determine sell type
                    let sell_type = if is_stop_loss { "STOP-LOSS" } else { "PROFIT" };
                    
                    let sell_event = format!(
                        "SELL ORDER ({}) | Market: {} | Period: {} | Price: ${:.6} | Units: {:.6} | Revenue: ${:.6} | {}: ${:.6} | Order ID: {} | Status: SUCCESS",
                        sell_type,
                        market_name,
                        trade.market_timestamp,
                        current_price,
                        units_to_sell,
                        sell_value,
                        if pnl >= 0.0 { "Profit" } else { "Loss" },
                        pnl,
                        order_id_str
                    );
                    crate::log_trading_event(&sell_event);
                    
                    crate::log_println!("   📊 Trade Results:");
                    crate::log_println!("      Revenue: ${:.6}", sell_value);
                    crate::log_println!("      Cost: ${:.6}", trade.purchase_price * units_to_sell);
                    crate::log_println!("      {}: ${:.6}", if pnl >= 0.0 { "Profit" } else { "Loss" }, pnl);
                    crate::log_println!("      Total Profit (all trades): ${:.6}", total_profit);
                }
                Err(e) => {
                    // Enhanced error logging for sell failures
                    let error_str = format!("{:?}", e);
                    let error_msg = format!("{}", e);
                    
                    crate::log_println!("═══════════════════════════════════════════════════════════");
                    crate::log_println!("❌ SELL ORDER FAILED - DETAILED ERROR ANALYSIS");
                    crate::log_println!("═══════════════════════════════════════════════════════════");
                    crate::log_println!("📊 Order Details:");
                    crate::log_println!("   Token Type: {}", trade.token_type.display_name());
                    crate::log_println!("   Token ID: {}", &trade.token_id[..16]);
                    crate::log_println!("   Condition ID: {}", &trade.condition_id[..16]);
                    crate::log_println!("   Period: {}", trade.market_timestamp);
                    crate::log_println!("   Side: SELL");
                    crate::log_println!("   Units to Sell: {:.6}", units_to_sell);
                    crate::log_println!("   Current Price: ${:.6}", current_price);
                    crate::log_println!("   Expected Revenue: ${:.6}", current_price * units_to_sell);
                    crate::log_println!("   Sell Attempt: {}/{}", trade.sell_attempts + 1, 3);
                    
                    crate::log_println!("\n🔍 Error Details:");
                    crate::log_println!("   Full Error: {}", error_msg);
                    crate::log_println!("   Error String: {}", error_str);
                    
                    // Check on-chain approval status
                    crate::log_println!("\n🔐 Approval Status Check:");
                    match self.api.check_is_approved_for_all().await {
                        Ok(true) => {
                            crate::log_println!("   ✅ setApprovalForAll: SET (Exchange is approved on-chain)");
                        }
                        Ok(false) => {
                            crate::log_println!("   ❌ setApprovalForAll: NOT SET (Exchange is NOT approved on-chain)");
                            crate::log_println!("   💡 This is likely the root cause - no on-chain approval means allowance will always be 0");
                            crate::log_println!("   💡 Solution: Run: cargo run --bin test_allowance -- --approve-only");
                        }
                        Err(e) => {
                            crate::log_println!("   ⚠️  Could not check setApprovalForAll status: {}", e);
                        }
                    }
                    
                    // Re-check balance and allowance for detailed diagnostics
                    crate::log_println!("\n📊 Current Balance & Allowance:");
                    match self.api.check_balance_allowance(&trade.token_id).await {
                        Ok((balance, allowance)) => {
                            let balance_decimal = balance / rust_decimal::Decimal::from(1_000_000u64);
                            let allowance_decimal = allowance / rust_decimal::Decimal::from(1_000_000u64);
                            let balance_f64 = f64::try_from(balance_decimal).unwrap_or(0.0);
                            let allowance_f64 = f64::try_from(allowance_decimal).unwrap_or(0.0);
                            
                            crate::log_println!("   Token Balance: {:.6} shares", balance_f64);
                            crate::log_println!("   Token Allowance: {:.6} shares", allowance_f64);
                            crate::log_println!("   Required: {:.6} shares", units_to_sell);
                            
                            if balance_f64 < units_to_sell {
                                crate::log_println!("   ❌ INSUFFICIENT BALANCE: Balance ({:.6}) < Required ({:.6})", balance_f64, units_to_sell);
                            } else {
                                crate::log_println!("   ✅ Balance sufficient: {:.6} >= {:.6}", balance_f64, units_to_sell);
                            }
                            
                            if allowance_f64 < units_to_sell {
                                crate::log_println!("   ❌ INSUFFICIENT ALLOWANCE: Allowance ({:.6}) < Required ({:.6})", allowance_f64, units_to_sell);
                                crate::log_println!("   💡 This is the root cause - Exchange contract cannot spend your tokens");
                                crate::log_println!("   💡 update_balance_allowance only refreshes cache - it doesn't set on-chain approval");
                            } else {
                                crate::log_println!("   ✅ Allowance sufficient: {:.6} >= {:.6}", allowance_f64, units_to_sell);
                            }
                        }
                        Err(e) => {
                            crate::log_println!("   ⚠️  Could not check balance/allowance: {}", e);
                        }
                    }
                    
                    // Analyze error type
                    crate::log_println!("\n🔍 Error Analysis:");
                    let is_allowance_error = error_str.contains("allowance") || 
                                           (error_str.contains("not enough") && error_str.contains("allowance"));
                    let is_balance_error = error_str.contains("balance") && !error_str.contains("allowance");
                    let is_fill_error = error_str.contains("couldn't be fully filled") || error_str.contains("FOK");
                    let is_no_orders_error = error_str.contains("No opposing orders") || error_str.contains("no orders found");
                    
                    if is_allowance_error {
                        crate::log_println!("   Error Type: ALLOWANCE ERROR");
                        crate::log_println!("   Root Cause: Exchange contract is not approved to spend your tokens");
                        crate::log_println!("   Solution: Set setApprovalForAll on-chain:");
                        crate::log_println!("      cargo run --bin test_allowance -- --approve-only");
                    } else if is_balance_error {
                        crate::log_println!("   Error Type: BALANCE ERROR");
                        crate::log_println!("   Root Cause: You don't have enough tokens in your portfolio");
                        crate::log_println!("   Solution: Check your Polymarket portfolio - tokens may have been sold/redeemed");
                    } else if is_fill_error {
                        crate::log_println!("   Error Type: FILL ERROR");
                        crate::log_println!("   Root Cause: Order couldn't be fully filled (FOK/FAK order)");
                        crate::log_println!("   Solution: This is normal for market orders - partial fills may occur");
                    } else if is_no_orders_error {
                        crate::log_println!("   Error Type: NO ORDERS ERROR");
                        crate::log_println!("   Root Cause: No opposing orders in the order book");
                        crate::log_println!("   Solution: Wait for market liquidity or try again later");
                    } else {
                        crate::log_println!("   Error Type: UNKNOWN");
                        crate::log_println!("   Root Cause: Unknown error - see full error message above");
                    }
                    
                    crate::log_println!("\n═══════════════════════════════════════════════════════════");
                    
                    // Return error - the retry logic in check_pending_trades will handle it
                    // After max attempts, it will mark for claim
                    return Err(e);
                }
            }
        }
        
        crate::log_println!("═══════════════════════════════════════════════════════════");
        crate::log_println!("");
        
        Ok(())
    }

    /// Check and settle trades when markets close
    /// For momentum strategy: If token wasn't sold, it will be worth $1 if Up won, $0 if Down won
    pub async fn check_market_closure(&self) -> Result<()> {
        // In simulation mode, check simulation tracker positions for market closure
        if self.simulation_mode {
            if let Some(tracker) = &self.simulation_tracker {
                // Get all positions from simulation tracker
                let positions = tracker.get_all_positions().await;
                
                if positions.is_empty() {
                    return Ok(());
                }
                
                let current_timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                
                // Check each position for market closure
                for position in positions {
                    // Market closes at period_timestamp + 900 seconds
                    let market_end_timestamp = position.period_timestamp + 900;
                    let seconds_until_close = market_end_timestamp.saturating_sub(current_timestamp);
                    
                    if current_timestamp < market_end_timestamp - 30 {
                        // Market hasn't closed yet
                        continue;
                    }
                    
                    // Market has closed - check resolution
                    let condition_id = position.condition_id.clone();
                    let token_id = position.token_id.clone();
                    let token_type = position.token_type.clone();
                    
                    tracker.log_to_file(&format!(
                        "🔍 Market closed - checking resolution for {} (period: {})",
                        token_type.display_name(),
                        position.period_timestamp
                    )).await;
                    
                    let (market_closed, token_winner) = match self.check_market_result(&condition_id, &token_id).await {
                        Ok(result) => result,
                        Err(e) => {
                            tracker.log_to_file(&format!(
                                "⚠️  Failed to check market result: {} - will retry on next check",
                                e
                            )).await;
                            continue; // Retry on next check
                        }
                    };
                    
                    if market_closed {
                        // Log market end event
                        tracker.log_market_end(
                            &token_type.display_name(),
                            position.period_timestamp,
                            &condition_id
                        ).await;
                        
                        // Determine if market resolved Up or Down
                        let market_resolved_up = match token_type {
                            crate::detector::TokenType::BtcUp | crate::detector::TokenType::EthUp | 
                            crate::detector::TokenType::SolanaUp | crate::detector::TokenType::XrpUp => {
                                token_winner
                            }
                            crate::detector::TokenType::BtcDown | crate::detector::TokenType::EthDown | 
                            crate::detector::TokenType::SolanaDown | crate::detector::TokenType::XrpDown => {
                                !token_winner
                            }
                        };
                        
                        // Resolve all positions for this market
                        let (spent, earned, pnl) = tracker.resolve_market_positions(&condition_id, market_resolved_up).await;
                        
                        // Get total spending and earnings
                        let (total_spent, total_earned, total_realized_pnl) = tracker.get_total_spending_and_earnings().await;
                        
                        // Log market resolution summary
                        tracker.log_to_file(&format!(
                            "═══════════════════════════════════════════════════════════\n\
                             🏁 MARKET RESOLUTION SUMMARY\n\
                             ═══════════════════════════════════════════════════════════\n\
                             Market: {} | Period: {} | Condition: {}\n\
                             Resolution: {} {}\n\
                             \n\
                             This Market:\n\
                             - Total Spent: ${:.2}\n\
                             - Total Earned: ${:.2}\n\
                             - Net PnL: ${:.2} {}\n\
                             \n\
                             Overall Totals:\n\
                             - Total Spent (All Markets): ${:.2}\n\
                             - Total Earned (All Markets): ${:.2}\n\
                             - Total Realized PnL: ${:.2} {}\n\
                             ═══════════════════════════════════════════════════════════",
                            token_type.display_name(),
                            position.period_timestamp,
                            &condition_id[..16],
                            if market_resolved_up { "UP" } else { "DOWN" },
                            if market_resolved_up { "✅" } else { "❌" },
                            spent,
                            earned,
                            pnl,
                            if pnl >= 0.0 { "✅" } else { "❌" },
                            total_spent,
                            total_earned,
                            total_realized_pnl,
                            if total_realized_pnl >= 0.0 { "✅" } else { "❌" }
                        )).await;
                    }
                }
                
                return Ok(());
            }
        }
        
        let pending_trades: Vec<(String, PendingTrade)> = {
            let pending = self.pending_trades.lock().await;
            pending.iter()
                .map(|(key, trade)| (key.clone(), trade.clone()))
                .collect()
        };
        
        if pending_trades.is_empty() {
            // No pending trades - nothing to check
            return Ok(());
        }
        
        let current_timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        // Log that we're checking for market closure
        let unsold_count = pending_trades.iter().filter(|(_, t)| !t.sold).count();
        if unsold_count > 0 {
            debug!("🔍 Checking market closure for {} unsold trade(s) (checking every 10 seconds)", unsold_count);
        }
        
        for (key, trade) in pending_trades {
            // Skip if already sold
            if trade.sold {
                continue;
            }
            
            // Market closes at market_timestamp + 900 seconds
            let market_end_timestamp = trade.market_timestamp + 900;
            let seconds_until_close = market_end_timestamp.saturating_sub(current_timestamp);
            
            if current_timestamp < market_end_timestamp - 30 {
                // Market hasn't closed yet - log periodically (every 30 seconds) to show we're monitoring
                if seconds_until_close % 30 == 0 || seconds_until_close < 60 {
                    crate::log_println!("⏳ Monitoring trade for market closure: {} token (period: {}), market closes in {}s", 
                        trade.token_type.display_name(), trade.market_timestamp, seconds_until_close);
                }
                continue; // Market hasn't closed yet
            }
            
            // Market has closed (or is about to close) - log that we're checking resolution
            crate::log_println!("🔍 Market closed - checking resolution and attempting redemption for {} token (period: {})", 
                trade.token_type.display_name(), trade.market_timestamp);
            
            // Check if token won
            crate::log_println!("   📊 Checking market resolution for condition {}...", &trade.condition_id[..16]);
            let (market_closed, token_winner) = match self.check_market_result(&trade.condition_id, &trade.token_id).await {
                Ok(result) => result,
                Err(e) => {
                    crate::log_println!("   ⚠️  Failed to check market result: {} - will retry on next check", e);
                    continue; // Retry on next check
                }
            };
            
            if market_closed {
                crate::log_println!("   ✅ Market is closed and resolved");
                crate::log_println!("   📊 Token outcome: {} token {}", trade.token_type.display_name(), if token_winner { "WON (worth $1.00)" } else { "LOST (worth $0.00)" });
                
                // In simulation mode, log market end and resolve all positions for this market
                if self.simulation_mode {
                    if let Some(tracker) = &self.simulation_tracker {
                        // Log market end event
                        tracker.log_market_end(
                            &trade.token_type.display_name(),
                            trade.market_timestamp,
                            &trade.condition_id
                        ).await;
                        
                        // Determine if market resolved Up or Down based on the token type
                        // If we have an Up token and it won, market resolved Up
                        // If we have a Down token and it won, market resolved Down
                        let market_resolved_up = match trade.token_type {
                            crate::detector::TokenType::BtcUp | crate::detector::TokenType::EthUp | 
                            crate::detector::TokenType::SolanaUp | crate::detector::TokenType::XrpUp => {
                                token_winner // Up token won = market resolved Up
                            }
                            crate::detector::TokenType::BtcDown | crate::detector::TokenType::EthDown | 
                            crate::detector::TokenType::SolanaDown | crate::detector::TokenType::XrpDown => {
                                !token_winner // Down token won = market resolved Down (so Up = false)
                            }
                        };
                        
                        // Resolve all positions for this market
                        let (spent, earned, pnl) = tracker.resolve_market_positions(&trade.condition_id, market_resolved_up).await;
                        
                        // Get total spending and earnings
                        let (total_spent, total_earned, total_realized_pnl) = tracker.get_total_spending_and_earnings().await;
                        
                        // Log market resolution summary
                        tracker.log_to_file(&format!(
                            "═══════════════════════════════════════════════════════════\n\
                             🏁 MARKET RESOLUTION SUMMARY\n\
                             ═══════════════════════════════════════════════════════════\n\
                             Market: {} | Period: {} | Condition: {}\n\
                             Resolution: {}\n\
                             \n\
                             This Market:\n\
                             - Total Spent: ${:.2}\n\
                             - Total Earned: ${:.2}\n\
                             - Net PnL: ${:.2}\n\
                             \n\
                             Overall Totals:\n\
                             - Total Spent (All Markets): ${:.2}\n\
                             - Total Earned (All Markets): ${:.2}\n\
                             - Total Realized PnL: ${:.2}\n\
                             ═══════════════════════════════════════════════════════════",
                            trade.token_type.display_name(),
                            trade.market_timestamp,
                            &trade.condition_id[..16],
                            if market_resolved_up { "UP" } else { "DOWN" },
                            spent,
                            earned,
                            pnl,
                            total_spent,
                            total_earned,
                            total_realized_pnl
                        )).await;
                    }
                }
                
                // Log MARKET ENDED event to history.toml
                let market_name = trade.token_type.display_name();
                let market_end_event = format!(
                    "MARKET ENDED | Market: {} | Period: {} | Condition: {}",
                    market_name,
                    trade.market_timestamp,
                    trade.condition_id
                );
                crate::log_trading_event(&market_end_event);
                
                // Determine token value at resolution
                let token_value = if token_winner {
                    1.0 // Token won, worth $1
                } else {
                    0.0 // Token lost, worth $0
                };
                
                let total_value = trade.units * token_value;
                let total_cost = trade.units * trade.purchase_price;
                let profit = total_value - total_cost;
                
                // Log structured market result to history.toml
                let result_event = format!(
                    "MARKET RESULT | Market: {} | Period: {} | Outcome: {} | Token Value: ${:.6} | Cost: ${:.6} | Value: ${:.6} | Profit: ${:.6}",
                    market_name,
                    trade.market_timestamp,
                    if token_winner { "WON" } else { "LOST" },
                    token_value,
                    total_cost,
                    total_value,
                    profit
                );
                crate::log_trading_event(&result_event);
                
                // Automatically redeem ALL unsold tokens after market resolution
                // This replaces manual redemption - the bot will redeem all positions after market closes
                let should_redeem = !self.simulation_mode;
                
                // Track if redemption was successful (declared outside if block for use after)
                let mut redemption_successful = false;
                
                if should_redeem {
                    // CRITICAL: Check actual token balance before attempting redemption
                    // If balance is 0, tokens were already redeemed (manually or by bot) - mark as sold and skip
                    crate::log_println!("   🔍 Checking token balance before redemption...");
                    let current_balance = match self.api.check_balance_allowance(&trade.token_id).await {
                        Ok((balance, _)) => {
                            // Conditional tokens use 1e6 as base unit (like USDC)
                            // Convert from smallest unit to actual shares
                            let balance_decimal = balance / rust_decimal::Decimal::from(1_000_000u64);
                            let balance_f64 = f64::try_from(balance_decimal).unwrap_or(0.0);
                            crate::log_println!("   📊 Current token balance: {:.6} shares", balance_f64);
                            balance_f64
                        }
                        Err(e) => {
                            crate::log_println!("   ⚠️  Failed to check balance: {} - will attempt redemption anyway", e);
                            trade.units // Use stored units as fallback
                        }
                    };
                    
                    // If balance is 0, tokens are already redeemed - mark as sold and skip redemption
                    if current_balance == 0.0 {
                        crate::log_println!("   ✅ Token balance is 0 - tokens already redeemed (manually or by bot)");
                        crate::log_println!("   📋 Marking trade as sold - no redemption needed");
                        
                        // Log structured redemption status (already redeemed) to history.toml
                        let market_name = trade.token_type.display_name();
                        let redeem_event = format!(
                            "REDEMPTION STATUS | Market: {} | Period: {} | Status: ALREADY_REDEEMED",
                            market_name,
                            trade.market_timestamp
                        );
                        crate::log_trading_event(&redeem_event);
                        
                        // Mark as sold and remove trade
                        let mut pending = self.pending_trades.lock().await;
                        if let Some(t) = pending.get_mut(key.as_str()) {
                            t.sold = true;
                        }
                        pending.remove(&key);
                        drop(pending);
                        
                        // Update profit calculation
                        let mut total = self.total_profit.lock().await;
                        *total += profit;
                        drop(total);
                        
                        crate::log_println!("💰 Market Closed - Trade Already Redeemed");
                        crate::log_println!("   Token Type: {}", trade.token_type.display_name());
                        crate::log_println!("   Outcome: {} token {}", trade.token_type.display_name(), if token_winner { "won" } else { "lost" });
                        crate::log_println!("   Token value at resolution: ${:.6}", token_value);
                        crate::log_println!("   Total cost: ${:.6} | Total value: ${:.6} | Profit: ${:.6}", 
                              total_cost, total_value, profit);
                        crate::log_println!("   ✅ Trade closed (tokens were already redeemed)");
                        continue; // Move to next trade
                    }
                    
                    // Balance > 0 - proceed with redemption
                    // Increment redemption attempts counter
                    let mut trade_mut = trade.clone();
                    trade_mut.redemption_attempts += 1;
                    let max_redemption_attempts = 20; // Max 20 attempts (20 * 10 seconds = 200 seconds = ~3.3 minutes)
                    
                    if trade.claim_on_closure {
                        crate::log_println!("   📋 Auto-redeeming tokens (marked due to insufficient liquidity) - attempt {} (balance: {:.6})", 
                            trade_mut.redemption_attempts, current_balance);
                    } else if token_winner {
                        crate::log_println!("   📋 Auto-redeeming winning tokens (token won - worth $1.00) - attempt {} (balance: {:.6})", 
                            trade_mut.redemption_attempts, current_balance);
                    } else {
                        crate::log_println!("   📋 Auto-redeeming losing tokens (token lost - worth $0.00, but redeeming to close position) - attempt {} (balance: {:.6})", 
                            trade_mut.redemption_attempts, current_balance);
                    }
                    
                    // Redeem tokens - pass trade data directly to avoid lookup issues
                    // Keep retrying until successful (similar to stop-loss retry logic)
                    match self.redeem_token_by_id_with_trade(&trade_mut).await {
                        Ok(_) => {
                            crate::log_println!("   ✅ Tokens redeemed successfully (attempt {})", trade_mut.redemption_attempts);
                            
                            // Log structured redemption success to history.toml
                            let market_name = trade.token_type.display_name();
                            let redeem_event = format!(
                                "REDEMPTION SUCCESS | Market: {} | Period: {} | Attempt: {} | Status: SUCCESS",
                                market_name,
                                trade.market_timestamp,
                                trade_mut.redemption_attempts
                            );
                            crate::log_trading_event(&redeem_event);
                            
                            redemption_successful = true;
                        }
                        Err(e) => {
                            crate::log_println!("   ❌ Failed to redeem tokens (attempt {}/{}): {}", 
                                trade_mut.redemption_attempts, max_redemption_attempts, e);
                            
                            // Check if we've exceeded max attempts
                            if trade_mut.redemption_attempts >= max_redemption_attempts {
                                crate::log_println!("   ⚠️  Maximum redemption attempts ({}) reached", max_redemption_attempts);
                                crate::log_println!("   📋 Marking trade as abandoned - will not block new positions");
                                trade_mut.redemption_abandoned = true;
                                
                                // Log structured redemption failure to history.toml
                                let market_name = trade.token_type.display_name();
                                let redeem_event = format!(
                                    "REDEMPTION FAILED | Market: {} | Period: {} | Attempts: {} | Status: ABANDONED | Error: {}",
                                    market_name,
                                    trade.market_timestamp,
                                    trade_mut.redemption_attempts,
                                    e.to_string().chars().take(100).collect::<String>() // Truncate long errors
                                );
                                crate::log_trading_event(&redeem_event);
                                
                                // Update trade in HashMap
                                let mut pending = self.pending_trades.lock().await;
                                if let Some(t) = pending.get_mut(key.as_str()) {
                                    *t = trade_mut.clone();
                }
                                drop(pending);
                                
                                crate::log_println!("   ✅ Trade abandoned - you can manually redeem later if needed");
                                crate::log_println!("   💡 New positions in new markets will NOT be blocked by this trade");
                            } else {
                                warn!("⚠️  Token redemption failed: {} - will retry on next check (attempt {}/{})", 
                                    e, trade_mut.redemption_attempts, max_redemption_attempts);
                                
                                // Log structured redemption retry to history.toml
                                let market_name = trade.token_type.display_name();
                                let redeem_event = format!(
                                    "REDEMPTION RETRY | Market: {} | Period: {} | Attempt: {}/{} | Status: RETRYING",
                                    market_name,
                                    trade.market_timestamp,
                                    trade_mut.redemption_attempts,
                                    max_redemption_attempts
                                );
                                crate::log_trading_event(&redeem_event);
                                
                                // Update trade with incremented attempts counter
                                let mut pending = self.pending_trades.lock().await;
                                if let Some(t) = pending.get_mut(key.as_str()) {
                                    *t = trade_mut;
                                }
                                drop(pending);
                            }
                            
                            // Don't remove trade - keep it so we can retry redemption on next check_market_closure call
                            // Redemption might fail if:
                            // 1. Market hasn't fully resolved yet (wait a bit longer)
                            // 2. Tokens were already redeemed (will fail but that's okay - we'll mark as sold next time)
                            // 3. Network/API error (will retry)
                            // Continue to next trade - this one will be retried
                            continue;
                }
                    }
                } else {
                    // Simulation mode - just log what would happen
                    if token_winner {
                        crate::log_println!("   🎮 SIMULATION: Would redeem winning tokens (worth $1.00)");
                    } else {
                        crate::log_println!("   🎮 SIMULATION: Would redeem losing tokens (worth $0.00)");
                    }
                    redemption_successful = true; // In simulation, consider it successful
                }
                
                // Update profit calculation
                let mut total = self.total_profit.lock().await;
                *total += profit;
                let total_profit = *total;
                drop(total);
                
                // Only log settlement and remove trade if redemption was successful
                // If redemption failed, the trade remains for retry
                if redemption_successful {
                crate::log_println!("💰 Market Closed - Momentum Trade Settled");
                    crate::log_println!("   Token Type: {}", trade.token_type.display_name());
                    crate::log_println!("   Outcome: {} token {}", trade.token_type.display_name(), if token_winner { "won" } else { "lost" });
                    crate::log_println!("   Token value at resolution: ${:.6}", token_value);
                    crate::log_println!("   Total cost: ${:.6} | Total value: ${:.6} | Profit: ${:.6}", 
                      total_cost, total_value, profit);
                    crate::log_println!("   Total profit (all trades): ${:.6}", total_profit);
                    crate::log_println!("   ✅ Trade closed and removed from pending trades");
                
                    // Mark as sold and remove trade
                let mut pending = self.pending_trades.lock().await;
                    if let Some(t) = pending.get_mut(key.as_str()) {
                        t.sold = true;
                    }
                pending.remove(&key);
                    drop(pending);
                }
            }
        }
        
        Ok(())
    }

    async fn check_market_result(&self, condition_id: &str, token_id: &str) -> Result<(bool, bool)> {
        let market = self.api.get_market(condition_id).await?;
        
        let is_closed = market.closed;
        let is_winner = market.tokens.iter()
            .any(|t| t.token_id == token_id && t.winner);
        
        Ok((is_closed, is_winner))
    }

    /// Redeem tokens using trade data directly (avoids lookup issues)
    async fn redeem_token_by_id_with_trade(&self, trade: &PendingTrade) -> Result<()> {
        // Determine outcome string based on token type
        // For Up/Down markets: Up = "Up", Down = "Down"
        let outcome = match trade.token_type {
            crate::detector::TokenType::BtcUp | crate::detector::TokenType::EthUp | crate::detector::TokenType::SolanaUp | crate::detector::TokenType::XrpUp => "Up",
            crate::detector::TokenType::BtcDown | crate::detector::TokenType::EthDown | crate::detector::TokenType::SolanaDown | crate::detector::TokenType::XrpDown => "Down",
        };
        
        crate::log_println!("═══════════════════════════════════════════════════════════");
        crate::log_println!("🔄 ATTEMPTING TOKEN REDEMPTION");
        crate::log_println!("═══════════════════════════════════════════════════════════");
        crate::log_println!("📊 Redemption Details:");
        crate::log_println!("   Token Type: {}", trade.token_type.display_name());
        crate::log_println!("   Token ID: {}...", &trade.token_id[..16]);
        crate::log_println!("   Condition ID: {}...", &trade.condition_id[..16]);
        crate::log_println!("   Outcome: {}", outcome);
        crate::log_println!("   Units to redeem: {:.6}", trade.units);
        crate::log_println!("   Purchase price: ${:.6}", trade.purchase_price);
        crate::log_println!("");
        crate::log_println!("   🔄 Calling Polymarket API to redeem tokens...");
        
        // Call the API to redeem tokens
        match self.api.redeem_tokens(
            &trade.condition_id,
            &trade.token_id,
            outcome,
        ).await {
            Ok(_) => {
                crate::log_println!("   ✅ Redemption API call successful");
        Ok(())
            }
            Err(e) => {
                crate::log_println!("   ❌ Redemption API call failed: {}", e);
                Err(e)
            }
        }
    }
    
    /// Legacy method - kept for compatibility but uses trade lookup
    async fn redeem_token_by_id(&self, token_id: &str, _units: f64) -> Result<()> {
        // Get the condition ID and outcome from the token
        // We need to find which trade this token belongs to
        let trade = {
            let pending = self.pending_trades.lock().await;
            pending.values()
                .find(|t| t.token_id == token_id)
                .ok_or_else(|| anyhow::anyhow!("Trade not found for token {}", &token_id[..16]))?
                .clone()
        };
        
        self.redeem_token_by_id_with_trade(&trade).await
    }

    /// Reset for new period
    pub async fn reset_period(&self, old_period: u64) {
        let mut pending = self.pending_trades.lock().await;
        // Remove trades from old period
        pending.retain(|_, trade| trade.market_timestamp != old_period);
        drop(pending);
    }

    /// Print summary of all trades (for testing/verification)
    pub async fn print_trade_summary(&self) {
        // In simulation mode, print simulation position summary
        if self.simulation_mode {
            if let Some(tracker) = &self.simulation_tracker {
                // Get list of token IDs from positions and pending trades
                let mut token_ids: Vec<String> = tracker.get_position_token_ids().await;
                
                // Also add token IDs from pending trades that might have limit orders
                {
                    let pending = self.pending_trades.lock().await;
                    for trade in pending.values() {
                        if !token_ids.contains(&trade.token_id) {
                            token_ids.push(trade.token_id.clone());
                        }
                    }
                }
                
                // Get current prices for all positions
                let mut current_prices: HashMap<String, TokenPrice> = HashMap::new();
                for token_id in &token_ids {
                    if !current_prices.contains_key(token_id) {
                        if let Ok(orderbook) = self.api.get_orderbook(token_id).await {
                            let bid = orderbook.bids.first().map(|e| e.price);
                            let ask = orderbook.asks.first().map(|e| e.price);
                            let token_price = TokenPrice {
                                token_id: token_id.clone(),
                                bid,
                                ask,
                            };
                            current_prices.insert(token_id.clone(), token_price);
                        }
                    }
                }
                
                let summary = tracker.get_position_summary(&current_prices).await;
                tracker.log_position_summary(&current_prices).await;
                crate::log_to_history(&summary);
            }
            return;
        }
        
        // Copy needed data under lock, then release to minimize hold time
        let (n, profit, pending_count, pending_list): (u64, f64, usize, Vec<(String, crate::models::PendingTrade)>) = {
            let pending = self.pending_trades.lock().await;
            let tc = self.trades_executed.lock().await;
            let tp = self.total_profit.lock().await;
            let pc = pending.values().filter(|t| !t.sold).count();
            let list = pending
                .iter()
                .filter(|(_, t)| !t.sold)
                .map(|(k, t)| (k.clone(), t.clone()))
                .collect();
            (*tc, *tp, pc, list)
        };

        let ts = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
        let p = format!("[{}] ", ts);

        // Build entire block in one buffer, single write to stderr + history file
        let mut out = String::with_capacity(1024);
        out.push_str(&format!("{}═══════════════════════════════════════════════════════════\n", p));
        out.push_str(&format!("{}📊 TRADE SUMMARY\n", p));
        out.push_str(&format!("{}═══════════════════════════════════════════════════════════\n", p));
        out.push_str(&format!("{}Total Trades Executed: {}\n", p, n));
        out.push_str(&format!("{}Total Profit: ${:.6}\n", p, profit));
        out.push_str(&format!("{}Pending Trades: {}\n", p, pending_count));
        out.push_str(&format!("{} \n", p));

        if pending_count == 0 {
            out.push_str(&format!("{}No pending trades.\n", p));
        } else {
            for (key, trade) in &pending_list {
                out.push_str(&format!("{}Trade #{}:\n", p, key));
                out.push_str(&format!("{}   Token Type: {}\n", p, trade.token_type.display_name()));
                out.push_str(&format!("{}   Token ID: {}\n", p, trade.token_id));
                out.push_str(&format!("{}   Condition ID: {}\n", p, trade.condition_id));
                out.push_str(&format!("{}   Units: {:.6}\n", p, trade.units));
                out.push_str(&format!("{}   Purchase Price: ${:.6}\n", p, trade.purchase_price));
                out.push_str(&format!("{}   Target Sell Price: ${:.6}\n", p, trade.sell_price));
                out.push_str(&format!("{}   Market Timestamp: {}\n", p, trade.market_timestamp));
                out.push_str(&format!("{}   Status: PENDING\n", p));
                out.push_str(&format!("{}   Investment: ${:.6}\n", p, trade.investment_amount));
                out.push_str(&format!("{} \n", p));
            }
        }
        out.push_str(&format!("{}═══════════════════════════════════════════════════════════\n", p));
        out.push_str(&format!("{} \n", p));

        crate::log_to_history(&out);
    }
}

// Helper trait for Decimal to f64 conversion
trait ToF64 {
    fn to_f64(&self) -> Option<f64>;
}

impl ToF64 for rust_decimal::Decimal {
    fn to_f64(&self) -> Option<f64> {
        self.to_string().parse().ok()
    }
}



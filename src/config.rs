use clap::Parser;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Run in simulation mode (no real trades)
    /// Default: simulation mode is enabled (true)
    #[arg(short, long, default_value_t = true)]
    pub simulation: bool,

    /// Run in production mode (execute real trades)
    /// This sets simulation to false
    #[arg(long)]
    pub no_simulation: bool,

    /// Run in backtest mode (use historical price data)
    /// This overrides simulation and production modes
    #[arg(long)]
    pub backtest: bool,

    /// Configuration file path
    #[arg(short, long, default_value = "config.json")]
    pub config: PathBuf,
}

impl Args {
    /// Get the effective simulation mode
    /// If --no-simulation is used, it overrides the default
    /// If --backtest is used, it overrides everything
    pub fn is_simulation(&self) -> bool {
        if self.backtest {
            false // Backtest is not simulation (it's a separate mode)
        } else if self.no_simulation {
            false
        } else {
            self.simulation
        }
    }

    /// Check if we're in backtest mode
    pub fn is_backtest(&self) -> bool {
        self.backtest
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub polymarket: PolymarketConfig,
    pub trading: TradingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolymarketConfig {
    pub gamma_api_url: String,
    pub clob_api_url: String,
    pub api_key: Option<String>,
    pub api_secret: Option<String>,
    pub api_passphrase: Option<String>,
    /// Private key for signing orders (optional, but may be required for order placement)
    /// Format: hex string (with or without 0x prefix) or raw private key
    pub private_key: Option<String>,
    /// Proxy wallet address (Polymarket proxy wallet address where your balance is)
    /// If set, the bot will trade using this proxy wallet instead of the EOA (private key account)
    /// Format: Ethereum address (with or without 0x prefix)
    pub proxy_wallet_address: Option<String>,
    /// Signature type for authentication (optional, defaults to EOA if not set)
    /// 0 = EOA (Externally Owned Account - private key account)
    /// 1 = Proxy (Polymarket proxy wallet)
    /// 2 = GnosisSafe (Gnosis Safe wallet)
    /// If proxy_wallet_address is set, this should be 1 (Proxy)
    pub signature_type: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingConfig {
    pub eth_condition_id: Option<String>,
    pub btc_condition_id: Option<String>,
    pub solana_condition_id: Option<String>,
    pub xrp_condition_id: Option<String>,
    pub check_interval_ms: u64,
    /// Fixed trade amount in USD for BTC Up token purchase
    /// Default: 1.0 ($1.00)
    pub fixed_trade_amount: f64,
    /// Price threshold to trigger buy (BTC Up token must reach this price)
    /// Default: 0.9 ($0.90)
    pub trigger_price: f64,
    /// Minimum minutes that must have elapsed before buying (after this time, check trigger_price)
    /// Default: 10 (10 minutes)
    pub min_elapsed_minutes: u64,
    /// Price at which to sell the token (sell when price reaches this)
    /// Default: 0.99 ($0.99)
    pub sell_price: f64,
    /// Maximum price to buy at (don't buy if price exceeds this)
    /// Default: 0.95 ($0.95)
    /// Buy when: trigger_price <= bid_price <= max_buy_price
    pub max_buy_price: Option<f64>,
    /// Stop-loss price - sell if price drops below this (stop-loss protection)
    /// Default: 0.85 ($0.85) - sell if price drops 5% below purchase price
    /// If None, stop-loss is disabled
    pub stop_loss_price: Option<f64>,
    /// Hedge price - limit buy price for opposite token when buying at $0.9+ (hedging strategy)
    /// Default: 0.5 ($0.50) - place limit buy order for opposite token at this price
    /// If None, hedging is disabled
    /// Strategy: When buying a token at $0.9+, also place a limit buy for the opposite token at $0.5
    /// This creates a hedge - if market reverses, you'll have both tokens at favorable prices
    pub hedge_price: Option<f64>,
    /// Interval for checking market closure and redemption retries after period ends
    /// Default: 10 (10 seconds) - faster retries for redemption
    pub market_closure_check_interval_seconds: u64,
    /// Minimum time remaining in seconds before allowing a buy
    /// Default: 30 (30 seconds) - don't buy if less than 30 seconds remain
    /// This prevents buying too close to market close when prices can be volatile
    pub min_time_remaining_seconds: Option<u64>,
    /// Enable ETH market trading
    /// Default: true (ETH trading enabled)
    /// If false, only BTC markets will be traded
    pub enable_eth_trading: bool,
    /// Enable Solana market trading
    /// Default: false (Solana trading disabled)
    /// If true, Solana Up/Down markets will also be traded
    pub enable_solana_trading: bool,
    /// Enable XRP market trading
    /// Default: false (XRP trading disabled)
    /// If true, XRP Up/Down markets will also be traded
    pub enable_xrp_trading: bool,
    /// Dual limit-start bot: fixed limit price for Up/Down orders
    pub dual_limit_price: Option<f64>,
    /// Dual limit-start bot: fixed number of shares per order
    pub dual_limit_shares: Option<f64>,
    /// Dual limit-start bot: after this many minutes, if only one side filled, begin hedging the unfilled side
    /// Default: 10 (minutes)
    pub dual_limit_hedge_after_minutes: Option<u64>,
    /// Dual limit-start bot: hedge trigger/limit price for buying the unfilled side
    /// Default: 0.85 ($0.85)
    pub dual_limit_hedge_price: Option<f64>,
    /// Dual limit-start bot: early hedge check time (minutes) - check earlier if multiple markets unfilled or uptrending
    /// Default: 5 (minutes)
    pub dual_limit_early_hedge_minutes: Option<u64>,
    /// Dual limit-start bot: minimum trend strength (0.0-1.0) to trigger early hedge
    /// Default: 0.3
    pub dual_limit_trend_strength_threshold: Option<f64>,
    /// Dual limit-start bot: price buffer for early hedge (hedge if price >= hedge_price - buffer and uptrending)
    /// Default: 0.05 ($0.05)
    pub dual_limit_trend_price_buffer: Option<f64>,
    /// Dual limit-start bot: number of price snapshots to track for trend analysis
    /// Default: 20
    pub dual_limit_trend_history_size: Option<usize>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            polymarket: PolymarketConfig {
                gamma_api_url: "https://gamma-api.polymarket.com".to_string(),
                clob_api_url: "https://clob.polymarket.com".to_string(),
                api_key: None,
                api_secret: None,
                api_passphrase: None,
                private_key: None,
                proxy_wallet_address: None,
                signature_type: None,
            },
            trading: TradingConfig {
                eth_condition_id: None,
                btc_condition_id: None,
                solana_condition_id: None,
                xrp_condition_id: None,
                check_interval_ms: 1000,
                fixed_trade_amount: 1.0, // $1.00
                trigger_price: 0.9, // $0.90 trigger price
                min_elapsed_minutes: 10, // 10 minutes must have elapsed
                sell_price: 0.99, // Sell at $0.99
                max_buy_price: Some(0.95), // Maximum price to buy at ($0.95)
                stop_loss_price: Some(0.85), // Stop-loss at $0.85 (sell if price drops below this)
                hedge_price: Some(0.5), // Hedge price at $0.5 (limit buy for opposite token)
                market_closure_check_interval_seconds: 10, // 10 seconds - faster redemption retries
                min_time_remaining_seconds: Some(30), // 30 seconds - don't buy if less time remains
                enable_eth_trading: true, // ETH trading enabled by default
                enable_solana_trading: false, // Solana trading disabled by default
                enable_xrp_trading: false, // XRP trading disabled by default
                dual_limit_price: None,
                dual_limit_shares: None,
                dual_limit_hedge_after_minutes: Some(10),
                dual_limit_hedge_price: Some(0.85),
                dual_limit_early_hedge_minutes: Some(5),
                dual_limit_trend_strength_threshold: Some(0.3),
                dual_limit_trend_price_buffer: Some(0.05),
                dual_limit_trend_history_size: Some(60),
            },
        }
    }
}

impl Config {
    pub fn load(path: &PathBuf) -> anyhow::Result<Self> {
        if path.exists() {
            let content = std::fs::read_to_string(path)?;
            Ok(serde_json::from_str(&content)?)
        } else {
            let config = Config::default();
            let content = serde_json::to_string_pretty(&config)?;
            std::fs::write(path, content)?;
            Ok(config)
        }
    }
}


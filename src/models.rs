use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Market {
    #[serde(rename = "conditionId")]
    pub condition_id: String,
    #[serde(rename = "id")]
    pub market_id: Option<String>, // Market ID (numeric string)
    pub question: String,
    pub slug: String,
    #[serde(rename = "resolutionSource")]
    pub resolution_source: Option<String>,
    #[serde(rename = "endDateISO")]
    pub end_date_iso: Option<String>,
    #[serde(rename = "endDateIso")]
    pub end_date_iso_alt: Option<String>,
    pub active: bool,
    pub closed: bool,
    pub tokens: Option<Vec<Token>>,
    #[serde(rename = "clobTokenIds")]
    pub clob_token_ids: Option<String>, // JSON string array
    pub outcomes: Option<String>, // JSON string array like "[\"Up\", \"Down\"]"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    #[serde(rename = "tokenId")]
    pub token_id: String,
    pub outcome: String,
    pub price: Option<Decimal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBook {
    pub bids: Vec<OrderBookEntry>,
    pub asks: Vec<OrderBookEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookEntry {
    pub price: Decimal,
    pub size: Decimal,
}

#[derive(Debug, Clone)]
pub struct TokenPrice {
    pub token_id: String,
    pub bid: Option<Decimal>,
    pub ask: Option<Decimal>,
}

impl TokenPrice {
    pub fn mid_price(&self) -> Option<Decimal> {
        match (self.bid, self.ask) {
            (Some(bid), Some(ask)) => Some((bid + ask) / Decimal::from(2)),
            (Some(bid), None) => Some(bid),
            (None, Some(ask)) => Some(ask),
            (None, None) => None,
        }
    }

    pub fn ask_price(&self) -> Decimal {
        self.ask.unwrap_or(Decimal::ZERO)
    }
}

/// Order structure for creating orders (before signing)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRequest {
    pub token_id: String,
    pub side: String, // "BUY" or "SELL"
    pub size: String,
    pub price: String,
    #[serde(rename = "type")]
    pub order_type: String, // "LIMIT" or "MARKET"
}

/// Signed order structure for posting to Polymarket
/// According to Polymarket docs, orders must be signed with private key before posting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedOrder {
    // Order fields
    #[serde(rename = "tokenID")]
    pub token_id: String,
    pub side: String, // "BUY" or "SELL"
    pub size: String,
    pub price: String,
    #[serde(rename = "type")]
    pub order_type: String, // "LIMIT" or "MARKET"
    
    // Signature fields (will be populated when signing)
    pub signature: Option<String>,
    pub signer: Option<String>, // Address derived from private key
    pub nonce: Option<u64>,
    pub expiration: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderResponse {
    pub order_id: Option<String>,
    pub status: String,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceResponse {
    pub balance: String,
    pub allowance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedeemResponse {
    pub success: bool,
    pub message: Option<String>,
    pub transaction_hash: Option<String>,
    pub amount_redeemed: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MarketData {
    pub condition_id: String,
    pub market_name: String,
    pub up_token: Option<TokenPrice>,
    pub down_token: Option<TokenPrice>,
}

/// Trade for momentum-based strategy (buy any token when price reaches 0.9 after 10 minutes)
#[derive(Debug, Clone)]
pub struct PendingTrade {
    pub token_id: String,             // Token ID (can be BTC Up/Down, ETH Up/Down)
    pub condition_id: String,          // Market condition ID (BTC or ETH)
    pub token_type: crate::detector::TokenType, // Type of token
    pub order_id: Option<String>,      // Associated CLOB order id (primarily for limit orders)
    pub investment_amount: f64,       // Fixed trade amount
    pub units: f64,                   // Total token shares purchased (expected)
    pub purchase_price: f64,          // Price at which token was purchased (BID)
    pub sell_price: f64,              // Target sell price (0.99 or 1.0)
    pub timestamp: std::time::Instant, // When the trade was executed
    pub market_timestamp: u64,        // The 15-minute period timestamp
    pub sold: bool,                   // Whether the token has been sold
    pub confirmed_balance: Option<f64>, // Confirmed token balance in portfolio (None = not verified yet)
    pub buy_order_confirmed: bool,    // Whether the buy order was confirmed and tokens received
    pub limit_sell_orders_placed: bool, // Whether limit sell orders have been placed (for market buys and limit buy fills)
    pub no_sell: bool,                // If true, do not place any sell orders after fill (log confirmation only)
    pub claim_on_closure: bool,       // If true, claim/redeem tokens at market closure instead of selling (e.g., insufficient liquidity)
    pub sell_attempts: u32,        // Number of times we've tried to sell (to limit retries)
    pub redemption_attempts: u32,  // Number of times we've tried to redeem (to track failed redemptions)
    pub redemption_abandoned: bool, // If true, redemption failed too many times - don't block new positions
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketToken {
    pub outcome: String,
    pub price: rust_decimal::Decimal,
    #[serde(rename = "token_id")]
    pub token_id: String,
    pub winner: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketDetails {
    #[serde(rename = "accepting_order_timestamp")]
    pub accepting_order_timestamp: Option<String>,
    #[serde(rename = "accepting_orders")]
    pub accepting_orders: bool,
    pub active: bool,
    pub archived: bool,
    pub closed: bool,
    #[serde(rename = "condition_id")]
    pub condition_id: String,
    pub description: String,
    #[serde(rename = "enable_order_book")]
    pub enable_order_book: bool,
    #[serde(rename = "end_date_iso")]
    pub end_date_iso: String,
    pub fpmm: String,
    #[serde(rename = "game_start_time")]
    pub game_start_time: Option<String>,
    pub icon: String,
    pub image: String,
    #[serde(rename = "is_50_50_outcome")]
    pub is_50_50_outcome: bool,
    #[serde(rename = "maker_base_fee")]
    pub maker_base_fee: rust_decimal::Decimal,
    #[serde(rename = "market_slug")]
    pub market_slug: String,
    #[serde(rename = "minimum_order_size")]
    pub minimum_order_size: rust_decimal::Decimal,
    #[serde(rename = "minimum_tick_size")]
    pub minimum_tick_size: rust_decimal::Decimal,
    #[serde(rename = "neg_risk")]
    pub neg_risk: bool,
    #[serde(rename = "neg_risk_market_id")]
    pub neg_risk_market_id: String,
    #[serde(rename = "neg_risk_request_id")]
    pub neg_risk_request_id: String,
    #[serde(rename = "notifications_enabled")]
    pub notifications_enabled: bool,
    pub question: String,
    #[serde(rename = "question_id")]
    pub question_id: String,
    pub rewards: Rewards,
    #[serde(rename = "seconds_delay")]
    pub seconds_delay: u32,
    pub tags: Vec<String>,
    #[serde(rename = "taker_base_fee")]
    pub taker_base_fee: rust_decimal::Decimal,
    pub tokens: Vec<MarketToken>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rewards {
    #[serde(rename = "max_spread")]
    pub max_spread: rust_decimal::Decimal,
    #[serde(rename = "min_size")]
    pub min_size: rust_decimal::Decimal,
    pub rates: Option<serde_json::Value>,
}


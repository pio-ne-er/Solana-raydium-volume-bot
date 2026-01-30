use anyhow::Result;
use clap::Parser;
use rust_decimal::Decimal;
use polymarket_arbitrage_bot::{PolymarketApi, Config};
use polymarket_arbitrage_bot::models::TokenPrice;

#[derive(Parser, Debug)]
#[command(name = "test_sell")]
#[command(about = "Test selling tokens from your portfolio")]
struct Args {
    /// Token ID to sell (optional - if not provided, will scan portfolio and list all tokens)
    #[arg(short, long)]
    token_id: Option<String>,
    
    /// Amount of shares to sell (optional - if not provided, will sell all available)
    #[arg(short, long)]
    shares: Option<f64>,
    
    /// Config file path
    #[arg(short, long, default_value = "config.json")]
    config: String,
    
    /// Just check portfolio without selling
    #[arg(long)]
    check_only: bool,
    
    /// Scan portfolio and list all tokens with balance
    #[arg(long)]
    list: bool,
    
    /// Sell all tokens in portfolio automatically
    #[arg(long)]
    sell_all: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    let args = Args::parse();
    let config_path = std::path::PathBuf::from(&args.config);
    let config = Config::load(&config_path)?;

    // Create API client
    let api = PolymarketApi::new(
        config.polymarket.gamma_api_url.clone(),
        config.polymarket.clob_api_url.clone(),
        config.polymarket.api_key.clone(),
        config.polymarket.api_secret.clone(),
        config.polymarket.api_passphrase.clone(),
        config.polymarket.private_key.clone(),
        config.polymarket.proxy_wallet_address.clone(),
        config.polymarket.signature_type,
    );

    // If --list, --sell-all flag, or no token_id provided, scan portfolio
    if args.list || args.sell_all || args.token_id.is_none() {
        println!("🔍 Scanning your portfolio for tokens with balance...\n");
        
        // Get condition IDs from config
        let btc_condition_id = config.trading.btc_condition_id.as_deref();
        let eth_condition_id = config.trading.eth_condition_id.as_deref();
        
        let tokens_result: Result<Vec<(String, f64, String)>, _> = api.get_portfolio_tokens(btc_condition_id, eth_condition_id).await;
        match tokens_result {
            Ok(tokens) => {
                if tokens.is_empty() {
                    println!("   ⚠️  No tokens found with balance > 0");
                    println!("\n💡 Tips:");
                    println!("   - Make sure you've bought tokens from your portfolio");
                    println!("   - Check that BTC/ETH condition IDs are set in config.json");
                    println!("   - Try buying a token manually and run this again");
                    return Ok(());
                }
                
                println!("📋 Found {} token(s) with balance:\n", tokens.len());
                for (idx, (token_id, balance, description)) in tokens.iter().enumerate() {
                    println!("   {}. {} - Balance: {:.6} shares", idx + 1, description, balance);
                    println!("      Token ID: {}", token_id);
                    
                    // Get current price
                    if let Ok(Some(price)) = api.get_best_price(token_id).await {
                        if let Some(bid) = price.bid {
                            println!("      Current BID (sell) price: ${:.6}", bid);
                            println!("      Estimated value: ${:.6}", f64::try_from(bid).unwrap_or(0.0) * balance);
                        }
                    }
                    println!();
                }
                
                if args.check_only || (args.list && !args.sell_all) {
                    println!("✅ Portfolio scan complete");
                    println!("\n💡 To sell all tokens, run:");
                    println!("   cargo run --bin test_sell -- --sell-all");
                    println!("\n💡 To sell a specific token, run:");
                    println!("   cargo run --bin test_sell -- --token-id <TOKEN_ID>");
                    return Ok(());
                }
                
                // If --sell-all, sell all tokens
                if args.sell_all {
                    println!("💰 Selling all {} token(s) in portfolio...\n", tokens.len());
                    let mut success_count = 0;
                    let mut fail_count = 0;
                    
                    for (token_id, balance, description) in &tokens {
                        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                        println!("Selling: {} (Balance: {:.6} shares)", description, balance);
                        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
                        
                        match sell_token(&api, token_id, *balance, None, false).await {
                            Ok(_) => {
                                success_count += 1;
                                println!("✅ Successfully sold {}\n", description);
                            }
                            Err(e) => {
                                fail_count += 1;
                                eprintln!("❌ Failed to sell {}: {}\n", description, e);
                            }
                        }
                    }
                    
                    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                    println!("📊 Summary:");
                    println!("   ✅ Successfully sold: {} token(s)", success_count);
                    println!("   ❌ Failed: {} token(s)", fail_count);
                    println!("   📦 Total: {} token(s)", tokens.len());
                    return Ok(());
                }
                
                // If no token_id specified but we have tokens, use the first one
                if args.token_id.is_none() && !tokens.is_empty() {
                    println!("💰 No token ID specified. Using first token: {}\n", tokens[0].2);
                    // Continue with first token
                    return sell_token(&api, &tokens[0].0, tokens[0].1, args.shares, args.check_only).await;
                }
            }
            Err(e) => {
                eprintln!("❌ Failed to scan portfolio: {}", e);
                eprintln!("\n💡 You can still specify a token ID manually:");
                eprintln!("   cargo run --bin test_sell -- --token-id <TOKEN_ID>");
                return Err(e);
            }
        }
    }
    
    // If token_id is provided, sell that specific token
    let token_id = args.token_id.as_ref().ok_or_else(|| anyhow::anyhow!("Token ID is required. Use --list to scan portfolio first."))?;
    
    println!("🔍 Checking your portfolio...\n");
    println!("📊 Checking token: {}\n", token_id);
    
    match api.check_balance_allowance(token_id).await {
        Ok((balance, allowance)) => {
            // Convert from base units (1e6) to actual shares
            let balance_decimal = balance / Decimal::from(1_000_000u64);
            let allowance_decimal = allowance / Decimal::from(1_000_000u64);
            let balance_f64 = f64::try_from(balance_decimal).unwrap_or(0.0);
            let allowance_f64 = f64::try_from(allowance_decimal).unwrap_or(0.0);

            println!("   ✅ Balance: {:.6} shares", balance_f64);
            println!("   ✅ Allowance: {:.6} shares", allowance_f64);

            if balance_f64 == 0.0 {
                println!("   ⚠️  No balance for this token. Nothing to sell.");
                return Ok(());
            }

            // Get current price
            if let Some(ref token_id) = args.token_id {
                if let Ok(Some(price)) = api.get_best_price(token_id).await {
                    println!("   📈 Current Prices:");
                    if let Some(bid) = price.bid {
                        println!("      BID (sell price): ${:.6}", bid);
                    }
                    if let Some(ask) = price.ask {
                        println!("      ASK (buy price): ${:.6}", ask);
                    }
                }
            }

            if args.check_only {
                println!("\n✅ Portfolio check complete (--check-only mode, not selling)");
                return Ok(());
            }

            // Sell the token
            sell_token(&api, token_id, balance_f64, args.shares, false).await?;
        }
        Err(e) => {
            eprintln!("❌ Failed to check balance: {}", e);
            return Err(e);
        }
    }

    Ok(())
}

async fn sell_token(
    api: &PolymarketApi,
    token_id: &str,
    balance: f64,
    shares: Option<f64>,
    check_only: bool,
) -> Result<()> {
    if check_only {
        return Ok(());
    }

    // Check balance and allowance before selling
    println!("   🔍 Checking token balance and allowance before selling...");
    
    // Determine how many shares to sell (will be updated after balance check)
    let mut shares_to_sell = shares.unwrap_or(balance);
    
    match api.check_balance_allowance(token_id).await {
        Ok((balance_decimal, allowance_decimal)) => {
            // Convert from base units (1e6) to actual shares
            let balance_decimal_actual = balance_decimal / Decimal::from(1_000_000u64);
            let allowance_decimal_actual = allowance_decimal / Decimal::from(1_000_000u64);
            let balance_f64 = f64::try_from(balance_decimal_actual).unwrap_or(0.0);
            let allowance_f64 = f64::try_from(allowance_decimal_actual).unwrap_or(0.0);
            
            println!("   📊 Balance & Allowance Check:");
            println!("      Token Balance: {:.6} shares", balance_f64);
            println!("      Token Allowance: {:.6} shares", allowance_f64);
            
            // Update shares_to_sell based on actual balance
            shares_to_sell = shares.unwrap_or(balance_f64);
            if shares_to_sell > balance_f64 {
                println!("   ⚠️  Requested shares ({:.6}) > balance ({:.6}), selling all available", 
                    shares_to_sell, balance_f64);
                shares_to_sell = balance_f64;
            }
            
            println!("      Shares to Sell: {:.6} shares", shares_to_sell);
            
            // Check if we have enough balance
            if balance_f64 < shares_to_sell {
                anyhow::bail!("Insufficient balance: {:.6} < {:.6}", balance_f64, shares_to_sell);
            }
            
            // Check if we have enough allowance (for proxy wallets)
            if allowance_f64 < shares_to_sell {
                println!("   ⚠️  WARNING: Token allowance ({:.6}) is less than shares to sell ({:.6})", 
                    allowance_f64, shares_to_sell);
                println!("   📝 Token Allowance Explanation:");
                println!("      - Token allowance is permission for the CLOB contract to spend your tokens");
                println!("      - Required for proxy wallets before selling tokens");
                println!("      - Current allowance: {:.6} shares", allowance_f64);
                println!("      - Required: {:.6} shares", shares_to_sell);
                println!("   🔄 The SDK should handle approval automatically when placing the order.");
                println!("   💡 If the order fails, you may need to manually approve on Polymarket UI.");
                println!("   ⚠️  Proceeding anyway - SDK may auto-approve...");
            } else {
                println!("   ✅ Balance and allowance are sufficient");
            }
        }
        Err(e) => {
            eprintln!("   ⚠️  Failed to check balance/allowance: {} - proceeding anyway", e);
            // Use original balance if check fails
            shares_to_sell = shares.unwrap_or(balance);
        }
    }

    println!("\n💰 Attempting to sell {:.6} shares...", shares_to_sell);
    
    // Get current ASK price for selling
    let price_info: Option<TokenPrice> = api.get_best_price(token_id).await?;
    let current_price = price_info
        .and_then(|p| p.ask)
        .map(|p| f64::try_from(p).unwrap_or(0.0))
        .unwrap_or(0.0);

    if current_price == 0.0 {
        anyhow::bail!("Cannot determine current price for selling. No ASK price available.");
    }

    println!("   Current ASK price: ${:.6}", current_price);
    println!("   Expected revenue: ${:.6}", current_price * shares_to_sell);

    // Attempt to sell
    match api.place_market_order(token_id, shares_to_sell, "SELL", Some("FAK")).await {
        Ok(response) => {
            println!("\n✅ SELL ORDER PLACED SUCCESSFULLY!");
            println!("   Order ID: {:?}", response.order_id);
            println!("   Status: {}", response.status);
            if let Some(msg) = &response.message {
                println!("   Message: {}", msg);
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("\n❌ SELL ORDER FAILED: {}", e);
            Err(e)
        }
    }
}

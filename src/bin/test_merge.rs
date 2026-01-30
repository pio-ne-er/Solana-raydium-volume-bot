//! Test binary for the merge function of Up and Down token amounts.
//!
//! Default: check balance of current BTC 15-minute Up/Down tokens and show merge result.
//!   cargo run --bin test_merge
//!
//! Unit tests only:
//!   cargo run --bin test_merge -- --unit
//!
//! Use a specific condition ID:
//!   cargo run --bin test_merge -- --condition-id <ID> --config config.json

use anyhow::Result;
use clap::Parser;
use polymarket_arbitrage_bot::merge::{merge_up_down_amounts, MergeResult};
use polymarket_arbitrage_bot::{Config, PolymarketApi};

#[derive(Parser, Debug)]
#[command(name = "test_merge")]
#[command(about = "Test merge logic for Up and Down token amounts (default: current BTC 15m)")]
struct Args {
    /// Run unit tests only; no API or balance check
    #[arg(long)]
    unit: bool,

    /// Condition ID of market to check (overrides default current-BTC-15m)
    #[arg(long)]
    condition_id: Option<String>,

    /// Execute merge: redeem complete sets (Up+Down) to USDC via CTF relayer
    #[arg(long)]
    merge: bool,

    /// Config file path (for balance check / current BTC market)
    #[arg(short, long, default_value = "config.json")]
    config: String,
}

fn run_unit_tests() -> bool {
    let cases: &[(f64, f64, MergeResult)] = &[
        (5.0, 5.0, MergeResult { complete_sets: 5.0, remaining_up: 0.0, remaining_down: 0.0 }),
        (5.0, 3.0, MergeResult { complete_sets: 3.0, remaining_up: 2.0, remaining_down: 0.0 }),
        (2.0, 7.0, MergeResult { complete_sets: 2.0, remaining_up: 0.0, remaining_down: 5.0 }),
        (0.0, 5.0, MergeResult { complete_sets: 0.0, remaining_up: 0.0, remaining_down: 5.0 }),
        (5.0, 0.0, MergeResult { complete_sets: 0.0, remaining_up: 5.0, remaining_down: 0.0 }),
        (0.0, 0.0, MergeResult { complete_sets: 0.0, remaining_up: 0.0, remaining_down: 0.0 }),
        (2.5, 1.5, MergeResult { complete_sets: 1.5, remaining_up: 1.0, remaining_down: 0.0 }),
    ];

    println!("═══════════════════════════════════════════════════════════");
    println!("  Merge function unit tests (Up + Down token amounts)");
    println!("═══════════════════════════════════════════════════════════\n");

    let mut ok = true;
    for (i, (up, down, expected)) in cases.iter().enumerate() {
        let r = merge_up_down_amounts(*up, *down);
        let pass = r == *expected;
        if !pass {
            ok = false;
        }
        let status = if pass { "✅" } else { "❌" };
        println!("  {} Test {}: merge_up_down_amounts({}, {})", status, i + 1, up, down);
        println!("       → complete_sets={:.2}, remaining_up={:.2}, remaining_down={:.2}", r.complete_sets, r.remaining_up, r.remaining_down);
        if !pass {
            println!("       EXPECTED: complete_sets={:.2}, remaining_up={:.2}, remaining_down={:.2}", expected.complete_sets, expected.remaining_up, expected.remaining_down);
        }
    }

    println!("\n═══════════════════════════════════════════════════════════");
    if ok {
        println!("  All merge unit tests passed.");
    } else {
        println!("  Some merge unit tests failed.");
    }
    println!("═══════════════════════════════════════════════════════════\n");
    ok
}

/// Discover the current (or most recent) BTC 15-minute Up/Down market.
async fn discover_current_btc_15m(api: &PolymarketApi) -> Result<(String, String)> {
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let rounded = (current_time / 900) * 900;

    let slug = format!("btc-updown-15m-{}", rounded);
    if let Ok(market) = api.get_market_by_slug(&slug).await {
        if market.active && !market.closed {
            return Ok((market.condition_id.clone(), market.slug));
        }
    }
    for offset in 1..=3 {
        let try_time = rounded - (offset * 900);
        let try_slug = format!("btc-updown-15m-{}", try_time);
        if let Ok(market) = api.get_market_by_slug(&try_slug).await {
            if market.active && !market.closed {
                return Ok((market.condition_id.clone(), market.slug));
            }
        }
    }
    anyhow::bail!("Could not find current or recent active BTC 15-minute market (tried btc-updown-15m-{} and 3 previous periods)", rounded)
}

/// Run merge check using API: fetch Up/Down balances for condition_id, compute merge, print result.
/// Returns the MergeResult so the caller can decide to run merge_complete_sets.
async fn run_merge_check(api: &PolymarketApi, condition_id: &str, title: &str) -> Result<MergeResult> {
    let market = api.get_market(condition_id).await?;
    let mut up_token_id: Option<String> = None;
    let mut down_token_id: Option<String> = None;
    for t in &market.tokens {
        let o = t.outcome.to_uppercase();
        if o.contains("UP") || o == "1" {
            up_token_id = Some(t.token_id.clone());
        } else if o.contains("DOWN") || o == "0" {
            down_token_id = Some(t.token_id.clone());
        }
    }

    let up_balance = match &up_token_id {
        Some(id) => {
            let (bal, _) = api.check_balance_allowance(id).await?;
            let d = bal / rust_decimal::Decimal::from(1_000_000u64);
            f64::try_from(d).unwrap_or(0.0)
        }
        None => {
            eprintln!("   No Up token found for condition {}", condition_id);
            0.0
        }
    };
    let down_balance = match &down_token_id {
        Some(id) => {
            let (bal, _) = api.check_balance_allowance(id).await?;
            let d = bal / rust_decimal::Decimal::from(1_000_000u64);
            f64::try_from(d).unwrap_or(0.0)
        }
        None => {
            eprintln!("   No Down token found for condition {}", condition_id);
            0.0
        }
    };

    let r = merge_up_down_amounts(up_balance, down_balance);

    println!("\n═══════════════════════════════════════════════════════════");
    println!("  {}", title);
    println!("═══════════════════════════════════════════════════════════");
    println!("   BTC Up balance:   {:.6} shares", up_balance);
    println!("   BTC Down balance: {:.6} shares", down_balance);
    println!("   → Complete sets (mergeable): {:.6}", r.complete_sets);
    println!("   → Remaining Up:   {:.6}", r.remaining_up);
    println!("   → Remaining Down: {:.6}", r.remaining_down);
    println!("═══════════════════════════════════════════════════════════\n");

    Ok(r)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    if args.unit {
        let ok = run_unit_tests();
        if !ok {
            std::process::exit(1);
        }
        return Ok(());
    }

    let path = std::path::PathBuf::from(&args.config);
    let config = Config::load(&path)?;
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

    let (condition_id, title) = if let Some(cid) = &args.condition_id {
        (cid.clone(), format!("Merge check for condition {}...", &cid[..cid.len().min(16)]))
    } else {
        let (cid, slug) = discover_current_btc_15m(&api).await?;
        let title = format!("Current BTC 15-minute market: {} (Up/Down balances)", slug);
        (cid, title)
    };

    let result = run_merge_check(&api, &condition_id, &title).await?;

    if args.merge && result.complete_sets > 0.0 {
        println!("🔄 Merging {:.6} complete set(s) (Up+Down → USDC)...", result.complete_sets);
        match api.merge_complete_sets(&condition_id).await {
            Ok(res) => {
                if res.success {
                    println!("   ✅ Merge submitted successfully.");
                    if let Some(msg) = &res.message {
                        println!("   {}", msg);
                    }
                    if let Some(tx) = &res.transaction_hash {
                        println!("   Transaction: {}", tx);
                    }
                } else {
                    eprintln!("   ⚠️  Merge returned success=false: {:?}", res.message);
                }
            }
            Err(e) => {
                eprintln!("   ❌ Merge failed: {}", e);
                std::process::exit(1);
            }
        }
    } else if args.merge && result.complete_sets <= 0.0 {
        println!("   ⏭️  Nothing to merge (complete_sets = 0).");
    }

    Ok(())
}

// Backtest binary: simulate trading strategies using historical price data

use polymarket_arbitrage_bot::backtest::run_backtest;
use polymarket_arbitrage_bot::config::{Args, Config};
use clap::Parser;
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    
    if !args.is_backtest() {
        eprintln!("❌ This binary is for backtest mode only. Use --backtest flag.");
        eprintln!("   Or use main_dual_limit_045 with --backtest flag.");
        std::process::exit(1);
    }

    let config = Config::load(&args.config)?;

    eprintln!("🚀 Starting Backtest Mode");
    eprintln!("═══════════════════════════════════════════════════════════");
    eprintln!("📊 Strategy: Dual Limit-Start Bot (0.45)");
    eprintln!("   Limit Price: ${:.2}", config.trading.dual_limit_price.unwrap_or(0.45));
    if let Some(shares) = config.trading.dual_limit_shares {
        eprintln!("   Shares per order: {:.6}", shares);
    } else {
        eprintln!("   Shares per order: fixed_trade_amount / limit_price");
    }
    eprintln!("   Hedge after: {} minutes", config.trading.dual_limit_hedge_after_minutes.unwrap_or(10));
    eprintln!("   Hedge price: ${:.2}", config.trading.dual_limit_hedge_price.unwrap_or(0.85));
    eprintln!("");
    eprintln!("✅ Trading enabled for:");
    eprintln!("   - BTC (always)");
    if config.trading.enable_eth_trading {
        eprintln!("   - ETH");
    }
    if config.trading.enable_solana_trading {
        eprintln!("   - Solana");
    }
    if config.trading.enable_xrp_trading {
        eprintln!("   - XRP");
    }
    eprintln!("═══════════════════════════════════════════════════════════");
    eprintln!("");

    let results = run_backtest(&config)?;

    // Print results
    eprintln!("");
    eprintln!("═══════════════════════════════════════════════════════════");
    eprintln!("📊 BACKTEST RESULTS SUMMARY");
    eprintln!("═══════════════════════════════════════════════════════════");
    eprintln!("");
    eprintln!("📈 PERFORMANCE SUMMARY:");
    eprintln!("   Total Periods Tested: {}", results.total_periods);
    eprintln!("   ✅ Total Wins (PnL > 0): {}", results.winning_periods);
    eprintln!("   ❌ Total Losses (PnL < 0): {}", results.losing_periods);
    eprintln!("   ⚪ Break-even (PnL = 0): {}", results.total_periods - results.winning_periods - results.losing_periods);
    eprintln!("");
    eprintln!("💰 FINANCIAL SUMMARY:");
    eprintln!("   Total Cost: ${:.2}", results.total_cost);
    eprintln!("   Total Value: ${:.2}", results.total_value);
    eprintln!("   Total PnL: ${:.2}", results.total_pnl);
    eprintln!("");
    
    if results.total_periods > 0 {
        let win_rate = (results.winning_periods as f64 / results.total_periods as f64) * 100.0;
        let loss_rate = (results.losing_periods as f64 / results.total_periods as f64) * 100.0;
        eprintln!("📊 STATISTICS:");
        eprintln!("   Win Rate: {:.2}%", win_rate);
        eprintln!("   Loss Rate: {:.2}%", loss_rate);
        eprintln!("   Average PnL per Period: ${:.2}", results.total_pnl / results.total_periods as f64);
        
        if results.winning_periods > 0 {
            let avg_win: f64 = results.period_results.iter()
                .filter(|r| r.pnl > 0.0)
                .map(|r| r.pnl)
                .sum::<f64>() / results.winning_periods as f64;
            eprintln!("   Average Win Amount: ${:.2}", avg_win);
        }
        
        if results.losing_periods > 0 {
            let avg_loss: f64 = results.period_results.iter()
                .filter(|r| r.pnl < 0.0)
                .map(|r| r.pnl)
                .sum::<f64>() / results.losing_periods as f64;
            eprintln!("   Average Loss Amount: ${:.2}", avg_loss);
        }
    }
    eprintln!("");
    eprintln!("═══════════════════════════════════════════════════════════");
    eprintln!("");

    // Print detailed results for each period
    eprintln!("📋 Period-by-Period Results:");
    eprintln!("");
    for (i, period) in results.period_results.iter().enumerate() {
        eprintln!("Period {} (timestamp: {}):", i + 1, period.period_timestamp);
        eprintln!("   Outcome: {} won", if period.up_won { "UP" } else { "DOWN" });
        eprintln!("   Positions: {}", period.positions.len());
        for pos in &period.positions {
            let won = match pos.token_type.as_str() {
                s if s.ends_with("_UP") => period.up_won,
                s if s.ends_with("_DOWN") => !period.up_won,
                _ => false,
            };
            eprintln!("      {}: {:.6} shares @ ${:.6} -> {}", 
                pos.token_type, pos.shares, pos.purchase_price,
                if won { "$1.00" } else { "$0.00" });
        }
        eprintln!("   Cost: ${:.2}", period.total_cost);
        eprintln!("   Value: ${:.2}", period.total_value);
        eprintln!("   PnL: ${:.2}", period.pnl);
        eprintln!("");
    }

    Ok(())
}

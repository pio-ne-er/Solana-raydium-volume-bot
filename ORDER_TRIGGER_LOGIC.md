# Order Trigger Logic - Limit Order Bot Version

## When Orders Are Triggered

### Timing Conditions
1. **Period Check**: Orders are triggered once per 15-minute market period
2. **Time Elapsed**: Orders trigger when `min_elapsed_minutes` (default: 10 minutes) has passed since the period started
3. **One-Time Only**: Orders are placed only once per period (tracked by `current_period_bought`)

### Code Location
- **Detection**: `src/detector.rs` → `detect_limit_order_opportunities()`
- **Execution**: `src/bin/main_limit.rs` → monitoring callback (lines 292-334)

---

## Market Order vs Limit Order Decision

### Decision Logic
The bot checks the **ASK price** of each token (Up/Down) and decides:

```rust
if ask >= trigger_price (0.87) && ask <= 1.0 {
    → Use MARKET ORDER
} else {
    → Use LIMIT ORDER at trigger_price (0.87)
}
```

### Detailed Conditions

#### **MARKET ORDER** is used when:
- **BTC Up**: `ask >= 0.87 && ask <= 1.0`
- **BTC Down**: `ask >= 0.87 && ask <= 1.0`
- **ETH Up**: `ask >= 0.87 && ask <= 1.0` (if ETH trading enabled)
- **ETH Down**: `ask >= 0.87 && ask <= 1.0` (if ETH trading enabled)
- **SOL Up**: `ask >= 0.87 && ask <= 1.0` (if Solana trading enabled)
- **SOL Down**: `ask >= 0.87 && ask <= 1.0` (if Solana trading enabled)

**Action**: Immediately executes market buy order at current ask price

#### **LIMIT ORDER** is used when:
- **Any token**: `ask < 0.87` OR `ask > 1.0`

**Action**: Places limit buy order at `trigger_price` (0.87)
- If `ask < 0.87`: Order will wait in order book until price reaches 0.87
- If `ask > 1.0`: Order will fill immediately (standard exchange behavior - limit orders fill if limit price is better than market)

---

## Execution Flow

### Step-by-Step Process

1. **Market Snapshot Received** (every few seconds)
   - Location: `src/bin/main_limit.rs:292` - monitoring callback

2. **Check Timing** (`src/detector.rs:325`)
   ```rust
   if time_elapsed_seconds < min_elapsed_seconds {
       return; // Not time yet
   }
   ```

3. **Check If Already Placed** (`src/detector.rs:332`)
   ```rust
   if bought.contains(&period_key) {
       return; // Already placed orders for this period
   }
   ```

4. **Determine Order Type** (`src/detector.rs:345-358`)
   - For each token (BTC Up/Down, ETH Up/Down, SOL Up/Down):
     - Get current `ask` price
     - Check: `ask >= 0.87 && ask <= 1.0`?
     - Set `use_market_order` flag accordingly

5. **Execute Orders** (`src/bin/main_limit.rs:320-331`)
   ```rust
   if opportunity.use_market_order {
       trader.execute_buy(&opportunity).await;  // Market order
   } else {
       trader.execute_limit_buy(&opportunity).await;  // Limit order
   }
   ```

---

## Example Scenarios

### Scenario 1: BTC Up at 0.90
- **Ask Price**: 0.90
- **Check**: `0.90 >= 0.87 && 0.90 <= 1.0` → ✅ TRUE
- **Result**: **MARKET ORDER** executed immediately at 0.90

### Scenario 2: BTC Down at 0.55
- **Ask Price**: 0.55
- **Check**: `0.55 >= 0.87 && 0.55 <= 1.0` → ❌ FALSE
- **Result**: **LIMIT ORDER** placed at 0.87 (will wait in order book)

### Scenario 3: ETH Up at 1.05
- **Ask Price**: 1.05
- **Check**: `1.05 >= 0.87 && 1.05 <= 1.0` → ❌ FALSE (exceeds 1.0)
- **Result**: **LIMIT ORDER** placed at 0.87 (will fill immediately since 1.05 > 0.87)

---

## Configuration

### Key Settings (from `config.json`)
- `min_elapsed_minutes`: Default 10 minutes (when to trigger)
- `trigger_price`: Default 0.87 (price threshold for decision)

---

## Important Notes

1. **Limit Orders Fill Immediately**: If you place a limit buy at 0.87 when ask is 0.55, the order will fill immediately at 0.55 (standard exchange behavior - you're willing to pay up to 0.87, so it fills at better price)

2. **One Order Per Period**: The bot places orders only once per 15-minute period when `min_elapsed_minutes` is reached

3. **Both Up and Down**: Orders are placed for BOTH Up and Down tokens simultaneously (if conditions are met)

4. **No Re-entry**: Once orders are placed for a period, no additional orders are placed until the next period

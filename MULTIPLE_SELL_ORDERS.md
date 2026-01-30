# Multiple Limit Sell Orders - How They Work

## Scenario: Buy at 0.87, Place TWO Limit Sell Orders

### Example Setup
- **Buy Price**: 0.87 (exactly)
- **Balance**: 5.75 shares (from $5 investment)
- **Sell Order #1**: Limit sell at 0.98 (profit target)
- **Sell Order #2**: Limit sell at 0.80 (stop-loss)

---

## How Multiple Limit Sell Orders Work

### Exchange Behavior

When you place TWO limit sell orders for the same token:

1. **Both orders are placed independently** in the order book
2. **Each order has its own order ID** and can fill independently
3. **Orders fill based on price movement**:
   - If price goes UP → Order #1 (0.98) fills first
   - If price goes DOWN → Order #2 (0.80) fills first

### Execution Order

#### Scenario A: Price Goes Up
```
Current Price: 0.87
├─ Sell Order #1 at 0.98: Waiting in order book
└─ Sell Order #2 at 0.80: Waiting in order book

Price moves: 0.87 → 0.90 → 0.95 → 0.98
→ Order #1 (0.98) FILLS FIRST
→ Order #2 (0.80) remains in order book (but you have no tokens left)
→ Balance drops to 0
```

#### Scenario B: Price Goes Down
```
Current Price: 0.87
├─ Sell Order #1 at 0.98: Waiting in order book
└─ Sell Order #2 at 0.80: Waiting in order book

Price moves: 0.87 → 0.85 → 0.82 → 0.80
→ Order #2 (0.80) FILLS FIRST
→ Order #1 (0.98) remains in order book (but you have no tokens left)
→ Balance drops to 0
```

---

## Important Points

### 1. **Only ONE Order Can Fill**
- You have 5.75 shares total
- When one order fills, it sells ALL your tokens (5.75 shares)
- The other order remains in the book but has no tokens to sell
- The exchange will automatically cancel the remaining order (or it stays unfilled)

### 2. **Which Order Fills First?**
- **Depends on price direction**:
  - Price goes UP → Higher price order (0.98) fills first
  - Price goes DOWN → Lower price order (0.80) fills first

### 3. **Partial Fills**
- If you had MORE tokens than one order size, both could partially fill
- But in this bot, we place orders for the FULL balance
- So only ONE order will fill completely

---

## Current Bot Implementation

### What the Bot Does Now
- **Only ONE limit sell order** is placed (at `sell_price` = 0.98)
- Stop-loss sell orders are **disabled** for limit order version
- The bot detects fills by checking if balance drops to 0

### Fill Detection Logic
```rust
// In check_pending_trades()
if current_balance < 0.000001 {
    // Sell order filled - balance is 0
    trade.sold = true;
}
```

---

## If You Want TWO Sell Orders

### To Enable Two Sell Orders:
1. **Modify the code** to place both orders:
   - Order #1: At `sell_price` (0.98) - profit target
   - Order #2: At `stop_loss_price` (0.80) - stop-loss

2. **Both orders will be placed** in the order book

3. **Whichever price is reached first** will fill:
   - If price goes UP to 0.98 → Profit order fills
   - If price goes DOWN to 0.80 → Stop-loss order fills

4. **The bot will detect the fill** when balance drops to 0

---

## Example Timeline

### Buy at 0.87, Two Sell Orders Placed

```
Time 0:00 - Buy executed at 0.87
├─ Balance: 5.75 shares
├─ Order #1: Sell 5.75 @ 0.98 (placed)
└─ Order #2: Sell 5.75 @ 0.80 (placed)

Time 0:05 - Price moves to 0.90
├─ Order #1: Still waiting (needs 0.98)
└─ Order #2: Still waiting (needs 0.80)

Time 0:10 - Price moves to 0.95
├─ Order #1: Still waiting (needs 0.98)
└─ Order #2: Still waiting (needs 0.80)

Time 0:15 - Price reaches 0.98
├─ Order #1: ✅ FILLS at 0.98
├─ Order #2: ❌ Cancelled (no tokens left)
└─ Balance: 0 shares
```

OR

```
Time 0:00 - Buy executed at 0.87
├─ Balance: 5.75 shares
├─ Order #1: Sell 5.75 @ 0.98 (placed)
└─ Order #2: Sell 5.75 @ 0.80 (placed)

Time 0:05 - Price moves to 0.85
├─ Order #1: Still waiting (needs 0.98)
└─ Order #2: Still waiting (needs 0.80)

Time 0:10 - Price moves to 0.82
├─ Order #1: Still waiting (needs 0.98)
└─ Order #2: Still waiting (needs 0.80)

Time 0:15 - Price reaches 0.80
├─ Order #2: ✅ FILLS at 0.80 (stop-loss)
├─ Order #1: ❌ Cancelled (no tokens left)
└─ Balance: 0 shares
```

---

## Summary

- **Multiple sell orders**: Both are placed independently
- **Fill order**: Whichever price is reached first
- **Result**: Only ONE order fills (you have limited tokens)
- **Detection**: Bot detects fill when balance drops to 0
- **Current behavior**: Only ONE sell order is placed (profit target only)

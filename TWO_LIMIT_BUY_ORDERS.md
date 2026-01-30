# Two Limit Buy Orders (Up & Down) - Execution Behavior

## Scenario: Place TWO Limit Buy Orders at 0.87

### Setup
- **BTC Up Limit Buy**: Buy at 0.87
- **BTC Down Limit Buy**: Buy at 0.87
- **Relationship**: Up + Down ≈ 1.0 (they're inversely related)

---

## How Up and Down Tokens Work

### Price Relationship
- **Up token price** + **Down token price** ≈ **$1.00**
- When Up goes UP → Down goes DOWN
- When Up goes DOWN → Down goes UP

### Examples
- If Up = 0.60 → Down = 0.40 (sum = 1.00)
- If Up = 0.87 → Down = 0.13 (sum = 1.00)
- If Up = 0.95 → Down = 0.05 (sum = 1.00)

---

## Limit Buy Order Execution Rules

### Standard Exchange Behavior
A **limit BUY order at 0.87** means:
- "I want to buy, and I'm willing to pay UP TO 0.87"
- **If current ask < 0.87** → Order fills **IMMEDIATELY** at current ask (better price!)
- **If current ask = 0.87** → Order fills at 0.87
- **If current ask > 0.87** → Order waits in order book until ask drops to 0.87

### Key Point
**Limit buy orders fill IMMEDIATELY if ask price is BELOW the limit price!**
This is standard exchange behavior - you're willing to pay up to 0.87, so if someone is selling for less (e.g., 0.70), you get the better price immediately.

---

## Scenario Analysis

### Initial State: Both Orders Placed

```
BTC Up:   Current ask = 0.60 → Limit buy @ 0.87 → ✅ FILLS IMMEDIATELY at 0.60
BTC Down: Current ask = 0.40 → Limit buy @ 0.87 → ✅ FILLS IMMEDIATELY at 0.40

Both orders fill immediately because ask prices (0.60 and 0.40) are BELOW limit price (0.87)
```

### Scenario 1: Up Token Price Rises to 0.87

```
Time 0:00 - Initial prices
├─ BTC Up:   ask = 0.60 → Limit buy @ 0.87 → ✅ FILLS IMMEDIATELY at 0.60
└─ BTC Down: ask = 0.40 → Limit buy @ 0.87 → ✅ FILLS IMMEDIATELY at 0.40

Result: BOTH orders fill immediately at better prices than limit!
- ✅ BTC Up order FILLED at 0.60 (better than 0.87 limit)
- ✅ BTC Down order FILLED at 0.40 (better than 0.87 limit)
```

**Note**: In this scenario, both orders fill immediately because both ask prices are below the limit price of 0.87.

### Scenario 2: Down Token Price Rises to 0.87

```
Time 0:00 - Initial prices
├─ BTC Up:   ask = 0.60 → Limit buy @ 0.87 → ✅ FILLS IMMEDIATELY at 0.60
└─ BTC Down: ask = 0.40 → Limit buy @ 0.87 → ✅ FILLS IMMEDIATELY at 0.40

Result: BOTH orders fill immediately!
- ✅ BTC Up order FILLED at 0.60
- ✅ BTC Down order FILLED at 0.40
```

**Note**: Same as Scenario 1 - both fill immediately because ask prices are below limit.

### Scenario 3: When Orders Actually Wait

**Orders only WAIT if ask price is ABOVE the limit price:**

```
Time 0:00 - Prices are HIGH
├─ BTC Up:   ask = 0.95 → Limit buy @ 0.87 → ⏳ WAITING (ask 0.95 > limit 0.87)
└─ BTC Down: ask = 0.05 → Limit buy @ 0.87 → ✅ FILLS IMMEDIATELY at 0.05

Time 0:05 - Up price drops
├─ BTC Up:   ask = 0.90 → Limit buy @ 0.87 → ⏳ Still waiting (ask 0.90 > limit 0.87)
└─ BTC Down: ask = 0.10 → Already filled

Time 0:10 - Up price reaches 0.87
├─ BTC Up:   ask = 0.87 → ✅ LIMIT BUY FILLS at 0.87
└─ BTC Down: Already filled

Result:
- ⏳ BTC Up order waited until price dropped to 0.87
- ✅ BTC Down order filled immediately at 0.05
```

---

## Important Observations

### 1. **When Do Orders Fill Immediately?**

**Orders fill IMMEDIATELY if ask < limit price:**
- Up ask = 0.60, limit = 0.87 → ✅ Fills at 0.60 immediately
- Down ask = 0.40, limit = 0.87 → ✅ Fills at 0.40 immediately
- Up ask = 0.70, limit = 0.87 → ✅ Fills at 0.70 immediately

**Orders WAIT if ask > limit price:**
- Up ask = 0.95, limit = 0.87 → ⏳ Waits in order book
- Down ask = 0.90, limit = 0.87 → ⏳ Waits in order book

### 2. **Most Common Scenario**

**If both tokens have ask prices BELOW 0.87:**
```
Initial: Up = 0.60, Down = 0.40
├─ Up limit buy @ 0.87 → ✅ FILLS IMMEDIATELY at 0.60
└─ Down limit buy @ 0.87 → ✅ FILLS IMMEDIATELY at 0.40

Result: BOTH orders fill immediately at better prices!
```

**If one token has ask ABOVE 0.87:**
```
Initial: Up = 0.95, Down = 0.05
├─ Up limit buy @ 0.87 → ⏳ WAITS (ask 0.95 > limit 0.87)
└─ Down limit buy @ 0.87 → ✅ FILLS IMMEDIATELY at 0.05

Result: One fills immediately, one waits
```

---

## Execution Timeline Example

### Real-World Price Movement - Both Fill Immediately

```
Period Start: BTC price is neutral
├─ BTC Up:   ask = 0.50
└─ BTC Down: ask = 0.50

Place Orders:
├─ Limit Buy Up @ 0.87: ✅ FILLS IMMEDIATELY at 0.50 (ask < limit)
└─ Limit Buy Down @ 0.87: ✅ FILLS IMMEDIATELY at 0.50 (ask < limit)

Result: Both orders fill immediately at 0.50 (better than 0.87 limit!)
- You own BTC Up tokens (bought at 0.50)
- You own BTC Down tokens (bought at 0.50)
- Total cost: $10.00 (for both)
- Total value: ~$10.00 (Up + Down ≈ $1.00 per pair)
```

### Real-World Price Movement - One Waits

```
Period Start: BTC price is high (Up token expensive)
├─ BTC Up:   ask = 0.95
└─ BTC Down: ask = 0.05

Place Orders:
├─ Limit Buy Up @ 0.87: ⏳ WAITING (ask 0.95 > limit 0.87)
└─ Limit Buy Down @ 0.87: ✅ FILLS IMMEDIATELY at 0.05 (ask < limit)

5 minutes later: BTC price drops
├─ BTC Up:   ask = 0.90 → Still waiting (ask > limit)
└─ BTC Down: Already filled

10 minutes later: BTC price drops more
├─ BTC Up:   ask = 0.87 → ✅ FILLS at 0.87
└─ BTC Down: Already filled

Final State:
├─ ✅ Own BTC Up tokens (bought at 0.87)
└─ ✅ Own BTC Down tokens (bought at 0.05)
```

---

## Key Takeaways

### 1. **Both Orders Fill Immediately (Most Common)**
- If both ask prices are BELOW 0.87 → Both fill immediately
- You get better prices than your limit (e.g., buy at 0.60 instead of 0.87)
- You end up with a hedged position (owning both Up and Down)

### 2. **One Fills Immediately, One Waits**
- If one ask is BELOW 0.87 and one is ABOVE 0.87:
  - Lower ask fills immediately
  - Higher ask waits until price drops to 0.87

### 3. **Hedged Position Math**
```
If you buy:
- Up at 0.60 → Cost: $5.00
- Down at 0.40 → Cost: $5.00
Total Cost: $10.00

If Up + Down = 1.0:
- Up value + Down value = $10.00 (approximately)
- You're essentially **neutral** - not making or losing much
```

### 4. **Why This Strategy?**

Placing limit buys on both tokens at 0.87:
- **Protects you from bad fills** - won't pay more than 0.87
- **Gets better prices** - if ask is below 0.87, you get the better price immediately
- **Hedges your position** - if both fill, you own both sides (neutral position)
- **One order fills** = You have directional exposure at a good entry price

---

## Bot's Current Behavior

### What the Bot Does
- Places limit buy orders for **BOTH** Up and Down tokens
- Both at the same limit price (trigger_price = 0.87)
- Both orders are placed simultaneously when `min_elapsed_minutes` is reached

### Expected Outcome
- **Most likely**: Only ONE order fills (whichever token's price moves to 0.87 first)
- **Less likely**: Both orders fill (if price moves dramatically in both directions)
- **Result**: You'll have exposure to one side, or a hedged position if both fill

---

## Summary

**Two limit buy orders at 0.87:**
1. ✅ Both orders are placed independently
2. ✅ **If ask prices are BELOW 0.87** → Both fill IMMEDIATELY at better prices
3. ✅ **If ask prices are ABOVE 0.87** → Orders wait until price drops to 0.87
4. ✅ **Mixed scenario** → One fills immediately, one waits
5. ✅ Most likely: **BOTH fill immediately** if initial prices are below 0.87

The key insight: **Limit buy orders fill IMMEDIATELY if ask < limit price**. This means if both tokens have ask prices below 0.87, both orders will fill right away at those better prices, giving you a hedged position immediately.

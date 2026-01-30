# BTC Down Orders - Complete Details

This document explains all scenarios where BTC Down orders are placed in the trading bot.

## Configuration Values (from config.json)
- `stop_loss_price`: 0.80
- `sell_price`: 0.98
- `fixed_trade_amount`: $5.00

## Order Calculation Formulas
- **Opposite Buy Price**: `1.0 - stop_loss_price = 1.0 - 0.80 = 0.20`
- **Opposite Sell Price**: `(1.0 - stop_loss_price) + 0.1 = 0.20 + 0.1 = 0.30`
- **Opposite Stop-Loss Price**: `(1.0 - stop_loss_price) - 0.1 = 0.20 - 0.1 = 0.10`

---

## Scenario 1: After Market Buy of BTC Up ✅

**When**: Immediately after a market buy order for BTC Up is confirmed

**Orders Placed**:
1. **Limit SELL for BTC Up** @ `sell_price` (0.98) - Profit target
2. **Limit BUY for BTC Down** @ `(1 - stop_loss_price)` (0.20) - Hedge

**BTC Down Order Details**:
- **Type**: LIMIT BUY
- **Price**: $0.20 (fixed limit price)
- **Size**: Same number of shares as BTC Up purchase
  - Example: If BTC Up buy = 5.747 shares, BTC Down buy = 5.747 shares
- **Investment**: `5.747 × $0.20 = $1.15` (not $5.00)
- **Purpose**: Hedge against BTC Up price drop
- **Tracking Key**: `{period}_{token_id}` (regular trade key)

**Code Location**: `src/trader.rs` lines 877-926

---

## Scenario 2: When BTC Up Stop-Loss Triggers (BTC Down Already Owned) ✅

**When**: BTC Up price drops to `stop_loss_price` (0.80) AND we already own BTC Down tokens

**Actions**:
1. **Market SELL BTC Up** (immediate execution)
2. **Limit SELL for BTC Down** @ `(1 - stop_loss_price + 0.1)` (0.30)

**BTC Down Order Details**:
- **Type**: LIMIT SELL
- **Price**: $0.30 (profit target for opposite token)
- **Size**: All BTC Down shares we own
- **Purpose**: Profit from the hedge position
- **Tracking Key**: `{period}_opposite_{token_id}`

**Code Location**: `src/trader.rs` lines 1348-1412

---

## Scenario 3: When BTC Up Stop-Loss Triggers (BTC Down NOT Owned) ✅

**When**: BTC Up price drops to `stop_loss_price` (0.80) AND we DON'T own BTC Down tokens yet

**Actions**:
1. **Market SELL BTC Up** (immediate execution)
2. **Limit BUY for BTC Down** @ `(1 - stop_loss_price)` (0.20)

**BTC Down Order Details**:
- **Type**: LIMIT BUY
- **Price**: $0.20 (fixed limit price)
- **Size**: Same number of shares as BTC Up we just sold
  - Example: If we sold 5.747 BTC Up shares, buy 5.747 BTC Down shares
- **Purpose**: Establish hedge after stop-loss
- **Tracking Key**: `{period}_opposite_limit_{token_id}`

**Code Location**: `src/trader.rs` lines 1419-1485

---

## Scenario 4: When BTC Down Limit Buy Fills ✅

**When**: A BTC Down limit buy order (from Scenario 1 or 3) gets filled

**Orders Placed**:
- **Limit SELL for BTC Down** @ `sell_price` (0.98) - Profit target

**BTC Down Order Details**:
- **Type**: LIMIT SELL
- **Price**: $0.98 (profit target)
- **Size**: All BTC Down shares received from the buy fill
- **Purpose**: Profit target for BTC Down position
- **Tracking**: Updates existing trade with `limit_sell_orders_placed = true`

**Code Location**: `src/trader.rs` lines 960-1044

---

## Scenario 5: BTC Down Stop-Loss Protection 🛑

**When**: BTC Down price drops below `(1 - stop_loss_price - 0.1)` (0.10)

**Actions**:
- **Market SELL BTC Down** (immediate execution at current price)

**BTC Down Order Details**:
- **Type**: MARKET SELL (FAK - Fill and Kill)
- **Price**: Current market price (whatever is available)
- **Size**: All BTC Down shares we own
- **Purpose**: Limit losses if BTC Down crashes
- **Trigger Condition**: `current_price <= 0.10`

**Code Location**: `src/trader.rs` lines 1194-1278

---

## Complete Flow Example

### Initial Setup (After BTC Up Market Buy @ $0.87)
```
BTC Up Purchase: 5.747 shares @ $0.87 = $5.00
↓
Orders Placed:
1. Limit SELL BTC Up @ $0.98 (profit target)
2. Limit BUY BTC Down @ $0.20 (hedge) - 5.747 shares
```

### If BTC Up Limit Buy Fills
```
BTC Down Received: 5.747 shares @ $0.20 = $1.15
↓
Order Placed:
- Limit SELL BTC Down @ $0.98 (profit target)
```

### If BTC Up Price Drops to $0.80 (Stop-Loss)
```
Case A: BTC Down already owned
- Market SELL BTC Up @ $0.80
- Limit SELL BTC Down @ $0.30 (profit target)

Case B: BTC Down NOT owned yet
- Market SELL BTC Up @ $0.80
- Limit BUY BTC Down @ $0.20 (hedge) - 5.747 shares
```

### If BTC Down Price Drops to $0.10 (Opposite Token Stop-Loss)
```
- Market SELL BTC Down @ current price (limit losses)
```

---

## Key Points

1. **Share Count**: BTC Down orders always use the SAME number of shares as BTC Up (not same investment amount)

2. **Price Levels**:
   - Buy: $0.20 (fixed)
   - Sell (profit): $0.30 or $0.98 (depending on scenario)
   - Stop-loss: $0.10 (emergency exit)

3. **Order Types**:
   - Limit orders: Used for planned entries/exits
   - Market orders: Used for stop-loss (immediate execution)

4. **Tracking**: All BTC Down orders are tracked in `pending_trades` with keys containing `_opposite_` or `_opposite_limit_`

5. **Monitoring**: The bot continuously monitors BTC Down price and executes stop-loss if price drops below $0.10

# First Buying Logic – Comparison

## Market-order bot (main.rs) – “before” / reference logic

**Entry point:** `detector.detect_opportunities(&snapshot)`

**Per token (via `check_token()`):**
| Check | Source |
|-------|--------|
| Price source | **BID** (token.bid) |
| Time | `time_elapsed_seconds >= min_elapsed_seconds` |
| Price range | `trigger_price <= bid_price <= max_buy_price` |
| Time remaining | `time_remaining_seconds >= min_time_remaining_seconds` |
| Reset state | After a sell, price must drop below trigger_price before allowing another buy |

**Which tokens:** Each of BTC Up, BTC Down, ETH Up, ETH Down, etc. is run through `check_token()`. An opportunity is added only if that token’s checks pass. So you can get 0, 1, or more opportunities per tick.

**Execution:** `trader.execute_buy(&opportunity)` with `fixed_trade_amount` ($5). Same token type in same period is skipped via `has_active_position`.

---

## Limit-order bot (main_limit.rs) – current logic (different)

**Entry point:** `detector.detect_limit_order_opportunities(&snapshot)`

**Behavior:**
| Check | Source |
|-------|--------|
| Price source | **ASK** (up_token.ask / down_token.ask) |
| Time | `time_elapsed_seconds >= min_elapsed_seconds` only |
| Price range | **None** (trigger_price / max_buy_price not used) |
| Time remaining | **Not used** |
| Reset state | **Not used** |
| One-shot | Uses `period_key = "{}_limit_orders"` so opportunities are only emitted once per period |

**Which tokens:** When the window opens, **both** Up and Down are always added (for each market). So the first buy logic is not the same as the market bot.

---

## Summary of differences

1. **Price:** Market bot uses **BID**, limit bot uses **ASK**.
2. **Price range:** Market bot requires **trigger_price ≤ price ≤ max_buy_price**; limit bot has **no** price filter.
3. **Time remaining:** Market bot enforces **min_time_remaining_seconds**; limit bot does not.
4. **Reset:** Market bot uses reset-after-sell; limit bot does not.
5. **Which sides:** Market bot adds a token only if `check_token()` passes; limit bot always adds both Up and Down once the time window is open.

To make the **first** buying logic match the market bot, the limit-order bot should use `detect_opportunities()` (and thus `check_token()` + BID + trigger/max_buy/remaining/reset) for that first buy instead of `detect_limit_order_opportunities()`.

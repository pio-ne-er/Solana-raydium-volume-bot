# Polymarket Trading Bot

A Rust-based trading bot for Polymarket that monitors ETH, BTC, and Solana 15-minute price prediction markets and executes trades using momentum-based strategies.

## Bot Versions

### 1. Market Order Bot (Default)
**Binary:** `polymarket-arbitrage-bot` (default)

Uses market orders (FOK - Fill-or-Kill) to buy tokens when price conditions are met.

**Strategy:**
- Buys tokens when price reaches `trigger_price` after `min_elapsed_minutes`
- Uses market orders for immediate execution
- Sells when price reaches `sell_price` or stop-loss triggers

**Run:**
```bash
# Simulation mode
cargo run -- --simulation

# Production mode
cargo run -- --no-simulation
```

### 2. Limit Order Bot
**Binary:** `polymarket-arbitrage-bot-limit`

Uses limit orders for more precise price control.

**Strategy:**
- At `min_elapsed_minutes`, places limit buy orders for both Up and Down tokens
- When a buy order fills, immediately places TWO limit sell orders for the same token:
  - One at `sell_price` (profit target)
  - One at `stop_loss_price` (stop-loss protection)
- Whichever price is hit first will execute
- Ignores `max_buy_price` and `min_time_remaining_seconds`

**Run:**
```bash
# Simulation mode
cargo run --bin polymarket-arbitrage-bot-limit -- --simulation

# Production mode
cargo run --bin polymarket-arbitrage-bot-limit -- --no-simulation
```

### 3. Price Monitor (Price Recording Only)
**Binary:** `price_monitor`

**Price monitoring and recording only - NO TRADING**

This version only monitors real-time prices and records them to history files. Perfect for data collection and analysis without any trading activity.

**Features:**
- Monitors BTC, ETH, Solana, and XRP markets
- Records prices to `history/market_<PERIOD>_prices.toml` files
- Automatically discovers new markets when 15-minute periods change
- Uses the same config.json settings for market discovery
- No authentication required (read-only price monitoring)

**Run:**
```bash
cargo run --bin price_monitor -- --config config.json
```

**Output:**
- Prices are recorded to: `history/market_<PERIOD>_prices.toml`
- Format: `[TIMESTAMP] 📊 BTC: U$bid/$ask D$bid/$ask | ETH: U$bid/$ask D$bid/$ask | SOL: U$bid/$ask D$bid/$ask | XRP: U$bid/$ask D$bid/$ask | ⏱️  TIME_REMAINING`

**Note:** This version uses the updated file naming format (period only, no condition ID) to avoid duplicate files.

### 4. Dual Limit-Start Bot (0.45)
**Binary:** `main_dual_limit_045`

Places limit buy orders for BTC and any enabled ETH/SOL/XRP markets at the start of each 15-minute market.

**Strategy:**
- At market start (first ~2 seconds of the period), place limit buys for BTC and enabled ETH/SOL/XRP Up/Down at $0.45
- Number of shares uses `trading.dual_limit_shares` if set; otherwise `fixed_trade_amount / dual_limit_price`
- No position handling after placement: when a limit order fills, it logs confirmation only (no sell orders)
- Hedge (stop-loss via opposite token): if only one side (Up/Down) fills, then after `trading.dual_limit_hedge_after_minutes` (default 10) the bot watches the unfilled token’s BUY price; when it reaches `trading.dual_limit_hedge_price` (default $0.85), it cancels the unfilled $0.45 order and places a new buy for the same shares at $0.85
- Polling interval fixed at 1s for this bot to reduce API load
- Market enable flags: `trading.enable_eth_trading`, `trading.enable_solana_trading`, `trading.enable_xrp_trading`

**Run:**
```bash
# Simulation mode
cargo run --bin main_dual_limit_045 -- --simulation

# Production mode
cargo run --bin main_dual_limit_045 -- --no-simulation

cargo run --bin backtest -- --backtest
```

### 4. Dual Limit-Start Bot (1-hour)
**Binary:** `main_dual_limit_1h`

Same strategy as the 15-minute bot, but targets 1-hour BTC/ETH/SOL/XRP up/down markets.

**Strategy:**
- At market start (first ~2 seconds of the hour), place limit buys for BTC, ETH, SOL, and XRP Up/Down at `trading.dual_limit_price` (default $0.45)
- Number of shares uses `trading.dual_limit_shares` if set; otherwise `fixed_trade_amount / dual_limit_price`
- No position handling after placement: when a limit order fills, it logs confirmation only (no sell orders)
- Polling interval fixed at 1s for this bot to reduce API load

**Run:**
```bash
# Simulation mode
cargo run --bin main_dual_limit_1h -- --simulation

# Production mode
cargo run --bin main_dual_limit_1h -- --no-simulation
```

### 5. Backtest Mode
**Binary:** `backtest`

Simulates the Dual Limit-Start Bot (0.45) strategy using historical price data from the `history/` folder.

**How it works:**
- Reads all `history/market_*_prices.toml` files
- For each 15-minute period:
  - Assumes two limit buy orders placed at start: Up at $0.45, Down at $0.45
  - Simulates order fills when ask price <= $0.45
  - Applies hedge logic at 10 minutes (if enabled)
  - Determines winner from final prices (token with ask > 0.50 wins)
  - Calculates PnL: winning token = $1.00, losing token = $0.00
- Aggregates results across all periods

**Output:**
- Total periods tested
- Win rate (winning vs losing periods)
- Total cost, total value, total PnL
- Per-period detailed results

**Run:**
```bash
cargo run --bin backtest -- --backtest
```

**Note:** Requires price history files in `history/` folder (generated by `price_monitor` binary).

## Test Cases

### 1. Test Limit Order
**Binary:** `test_limit_order`

Test placing a limit order on Polymarket.

**Usage:**
```bash
# Use defaults (BTC Up, $0.55, 5 shares, 1 min expiration)
cargo run --bin test_limit_order

# Custom price (e.g., 60 cents)
cargo run --bin test_limit_order -- --price-cents 60

# Custom shares (e.g., 10 shares)
cargo run --bin test_limit_order -- --shares 10

# Custom expiration (e.g., 5 minutes)
cargo run --bin test_limit_order -- --expiration-minutes 5

# Specify token ID directly
cargo run --bin test_limit_order -- --token-id <TOKEN_ID>

# Custom side (BUY or SELL)
cargo run --bin test_limit_order -- --side SELL
```

**Options:**
- `-t, --token-id <TOKEN_ID>` - Token ID to buy (optional - auto-discovers BTC Up if not provided)
- `-p, --price-cents <CENTS>` - Price in cents (default: 55 = $0.55)
- `-s, --shares <SHARES>` - Number of shares (default: 5)
- `-e, --expiration-minutes <MINUTES>` - Expiration time in minutes (default: 1)
- `-c, --config <PATH>` - Config file path (default: config.json)
- `--side <SIDE>` - Order side: BUY or SELL (default: BUY)

### 2. Test Redeem
**Binary:** `test_redeem`

Redeem winning tokens from your portfolio after market resolution.

**Usage:**
```bash
# Scan portfolio and list all tokens with balance
cargo run --bin test_redeem -- --list

# Redeem all winning tokens automatically
cargo run --bin test_redeem -- --redeem-all

# Redeem a specific token
cargo run --bin test_redeem -- --token-id <TOKEN_ID>

# Just check portfolio without redeeming
cargo run --bin test_redeem -- --check-only
```

**Options:**
- `-t, --token-id <TOKEN_ID>` - Token ID to redeem (optional - scans portfolio if not provided)
- `-c, --config <PATH>` - Config file path (default: config.json)
- `--check-only` - Just check portfolio without redeeming
- `--list` - Scan portfolio and list all tokens with balance
- `--redeem-all` - Redeem all winning tokens in portfolio automatically

### 3. Test Merge (Up and Down token)
**Binary:** `test_merge`

Test the merge logic for Up and Down token amounts. A "complete set" is one Up + one Down; merging N sets corresponds to N × $1 collateral.

**Usage:**
```bash
# Default: check balance of current BTC 15-minute Up/Down and show merge result
cargo run --bin test_merge

# Run unit tests only (no API)
cargo run --bin test_merge -- --unit

# Use a specific market by condition ID
cargo run --bin test_merge -- --condition-id <CONDITION_ID> --config config.json

# Execute merge: redeem complete sets (Up+Down) to USDC via CTF relayer
cargo run --bin test_merge -- --merge
```

**Options:**
- `--unit` - Run unit tests only; no API or balance check
- `--condition-id <ID>` - Use this market instead of current BTC 15m
- `--merge` - Execute merge: submit CTF redeemPositions for complete sets (Up+Down → USDC). Requires Builder API credentials in config. No-op if complete_sets = 0.
- `-c, --config <PATH>` - Config file path (default: config.json)

**Default run:** Discovers the current (or most recent) BTC 15-minute Up/Down market, fetches your Up and Down token balances via the API, runs the merge logic, and prints: **BTC Up balance**, **BTC Down balance**, **Complete sets (mergeable)**, **Remaining Up**, **Remaining Down**. With `--merge`, also submits a relayer transaction to merge that many complete sets into USDC.

**Unit test cases:** equal amounts (5,5)→5 sets; more Up than Down (5,3)→3 sets, 2 Up left; more Down than Up (2,7)→2 sets, 5 Down left; zeros and fractional amounts.

### 4. Test Allowance
**Binary:** `test_allowance`

Check balance/allowance and manage token approvals.

**Usage:**
```bash
# Set on-chain approval (required once per proxy wallet before selling)
cargo run --bin test_allowance -- --approve-only

# Run approval, then test the cache refresh
cargo run --bin test_allowance -- --approve

# List all tokens with balance and allowance
cargo run --bin test_allowance -- --list

# Test update_balance_allowance_for_sell on a token
cargo run --bin test_allowance -- --token-id <TOKEN_ID>
```

**Options:**
- `--approve` - Run setApprovalForAll first, then the update_balance_allowance test
- `--approve-only` - Only run setApprovalForAll and exit
- `-c, --config <PATH>` - Config file path (default: config.json)
- `-t, --token-id <TOKEN_ID>` - Token ID to test (auto-picks first token with balance if not provided)
- `-i, --iterations <N>` - Number of iterations for cache-refresh test
- `-d, --delay-ms <MS>` - Delay between iterations in milliseconds
- `--list` - List all tokens with balance and allowance

**Important:** `update_balance_allowance` only **refreshes** the CLOB backend's cache from the chain. It does **not** set on-chain approval. If allowance is 0, the chain has no approval → the cache stays 0. You must run **setApprovalForAll** first.

## How It Works

## Setup

1. Install Rust (if not already installed):
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. Build the project:
   ```bash
   cargo build --release
   ```

3. Configure the bot:
   - Edit `config.json` (created on first run) or use command-line arguments
   - Set `eth_condition_id` and `btc_condition_id` if you know them
   - Otherwise, the bot will attempt to discover them automatically

## Usage

### Simulation Mode (Default)
Test the bot without executing real trades:
```bash
cargo run -- --simulation
```

### Production Mode
Execute real trades (requires API key):
```bash
cargo run -- --no-simulation
```

### Configuration Options

- `--simulation` / `--no-simulation`: Toggle simulation mode
- `--config <path>`: Specify config file path (default: `config.json`)

### Configuration File

The bot creates a `config.json` file on first run with the following structure:

```json
{
  "polymarket": {
    "gamma_api_url": "https://gamma-api.polymarket.com",
    "clob_api_url": "https://clob.polymarket.com",
    "ws_url": "wss://clob-ws.polymarket.com",
    "api_key": null
  },
  "trading": {
    "min_profit_threshold": 0.01,
    "max_position_size": 100.0,
    "eth_condition_id": null,
    "btc_condition_id": null,
    "check_interval_ms": 1000
  }
}
```

**Important Settings:**
- `min_profit_threshold`: Minimum profit (in dollars) required to execute a trade
- `max_position_size`: Maximum amount to invest per trade
- `check_interval_ms`: How often to check for opportunities (in milliseconds)
- `api_key`: Your Polymarket API key (required for production mode)

## How the Bot Detects Opportunities

1. **Market Discovery**: The bot searches for active ETH and BTC 15-minute markets using Polymarket's Gamma API
2. **Price Monitoring**: Continuously fetches order book data to get current ask prices for Up/Down tokens
3. **Arbitrage Calculation**: For each combination (ETH Up + BTC Down, ETH Down + BTC Up), calculates total cost
4. **Opportunity Detection**: If total cost < $1.00 and profit >= `min_profit_threshold`, executes trade
5. **Trade Execution**: Places simultaneous buy orders for both tokens

## Testing Allowance

The `test_allowance` binary checks balance/allowance and can run **setApprovalForAll** (on-chain) and/or **update_balance_allowance** (backend cache refresh).

**Important:** `update_balance_allowance` only **refreshes** the CLOB backend’s cache from the chain. It does **not** set on-chain approval. If allowance is 0, the chain has no approval → the cache stays 0. You must run **setApprovalForAll** first.

**Set on-chain approval (required once per proxy wallet before selling):**
```bash
cargo run --bin test_allowance -- --approve-only
```

**Run approval, then test the cache refresh:**
```bash
cargo run --bin test_allowance -- --approve
```

**List all tokens with balance and allowance:**
```bash
cargo run --bin test_allowance -- --list
```

**Test `update_balance_allowance_for_sell` on a token** (only useful after `--approve-only` or `--approve` if allowance was 0):
- Auto-pick the first token with balance: `cargo run --bin test_allowance`
- Use a specific token ID: `cargo run --bin test_allowance -- --token-id <TOKEN_ID>`

**Options:**
- `--approve` — Run setApprovalForAll first, then the update_balance_allowance test
- `--approve-only` — Only run setApprovalForAll and exit
- `-c, --config <path>` — Config file (default: `config.json`)
- `-i, --iterations <N>`, `-d, --delay-ms <ms>` — For the cache-refresh test

The tool prints balance and allowance **before** and **after** the update. If allowance stays 0, it will prompt you to run with `--approve` or `--approve-only`.

## Notes

- The bot runs continuously until stopped (Ctrl+C)
- In simulation mode, all trades are logged but not executed
- The bot automatically discovers condition IDs if not provided in config
- Make sure you have sufficient balance and API permissions for production trading


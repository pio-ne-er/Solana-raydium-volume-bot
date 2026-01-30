# Test Sell Function

This document explains how to test the sell function. The test can automatically scan your portfolio and sell all tokens.

## Quick Start - Sell All Tokens

**Sell ALL tokens in your portfolio automatically:**
```bash
cargo run --bin test_sell -- --sell-all
```

This will:
1. ✅ **Automatically scan your portfolio** - Finds all tokens with balance > 0
2. ✅ **List all tokens found** - Shows BTC Up/Down, ETH Up/Down, etc. with balances
3. ✅ **Automatically sell each token** - Sells them one by one
4. ✅ **Show summary** - Displays successful/failed sales

**No need to manually find token IDs!** The test automatically discovers what tokens you own by:
- Getting current BTC and ETH markets from your config
- Checking balance for each token (BTC Up, BTC Down, ETH Up, ETH Down)
- Listing only tokens you actually own

## Other Options

**List all tokens in portfolio (no selling):**
```bash
cargo run --bin test_sell -- --list
```

**Check a specific token (no selling):**
```bash
cargo run --bin test_sell -- --token-id <TOKEN_ID> --check-only
```

**Sell a specific token:**
```bash
cargo run --bin test_sell -- --token-id <TOKEN_ID>
```

**Sell specific amount of a token:**
```bash
cargo run --bin test_sell -- --token-id <TOKEN_ID> --shares 1.0
```

## Examples

```bash
# Sell all tokens in portfolio (RECOMMENDED)
cargo run --bin test_sell -- --sell-all

# Just see what tokens you have
cargo run --bin test_sell -- --list

# Sell a specific token
cargo run --bin test_sell -- --token-id 39262267221676949796326419211008961431735960549601091803803006482409998029102
```

## What the test does

1. ✅ Checks your token balance
2. ✅ Checks token allowance (permission for CLOB contract to spend)
3. ✅ Gets current market prices (BID/ASK)
4. ✅ Attempts to sell the token using FAK (Fill-and-Kill) order type
5. ✅ Shows order result (success or failure)

## Troubleshooting

- **"No balance"**: You don't have any of this token. Buy it first.
- **"Insufficient allowance"**: The CLOB contract needs permission. The SDK should handle this automatically, but if it fails, you may need to approve manually on Polymarket UI.
- **"Failed to create Amount from shares"**: There's an issue with the share amount format. Check the logs for details.

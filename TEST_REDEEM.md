# Test Redeem Script

This script allows you to redeem winning tokens from your portfolio after market resolution.

## Usage

### Scan Portfolio and List Tokens

```bash
cargo run --bin test_redeem -- --list
```

This will:
- Scan your portfolio for all tokens with balance
- Check market resolution status for each token
- Show which tokens are winners (redeemable), losers (worth $0.00), or unresolved

### Redeem All Winning Tokens

```bash
cargo run --bin test_redeem -- --redeem-all
```

This will:
- Scan your portfolio
- Identify all winning tokens (worth $1.00)
- Automatically redeem all winning tokens
- Show redemption status for each token

### Check Portfolio Without Redeeming

```bash
cargo run --bin test_redeem -- --check-only
```

This will scan your portfolio and show status without redeeming anything.

### Redeem a Specific Token

```bash
cargo run --bin test_redeem -- --token-id <TOKEN_ID>
```

Replace `<TOKEN_ID>` with the actual token ID you want to redeem.

## How It Works

1. **Portfolio Scanning**: The script discovers current BTC and ETH 15-minute markets and checks your token balances.

2. **Market Resolution Check**: For each token, it checks:
   - If the market is closed/resolved
   - If the token is a winner (worth $1.00) or loser (worth $0.00)

3. **Redemption**: Only winning tokens can be redeemed. The script:
   - Verifies the token is a winner
   - Calls the Polymarket redemption API via Relayer Client
   - Shows redemption status

## Requirements

- Your `config.json` must have valid API credentials (`api_key`, `api_secret`, `api_passphrase`)
- Tokens must be from resolved markets
- Only winning tokens (worth $1.00) can be redeemed

## Example Output

```
🔍 Scanning your portfolio for tokens with balance...

📋 Found 2 token(s) with balance:

   1. BTC Up - Balance: 5.000000 shares
      Token ID: 70238288237322067060223039335050239483380846190014906122300314427956911294731
      Condition ID: 0xee746a8fe5f541...
      Outcome: Up
      Status: ✅ WINNING TOKEN (worth $1.00)

   2. ETH Down - Balance: 3.500000 shares
      Token ID: 32779369035003071234567890123456789012345678901234567890123456789012345678901
      Condition ID: 0xad1be889930162...
      Outcome: Down
      Status: ❌ LOSING TOKEN (worth $0.00)

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
📊 Portfolio Summary:
   ✅ Winning tokens (redeemable): 1 token(s)
   ❌ Losing tokens (worth $0.00): 1 token(s)
   ⏳ Unresolved markets: 0 token(s)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

## Troubleshooting

- **"Market is not yet resolved"**: Wait for the market to close before redeeming
- **"Token is not a winner"**: Only winning tokens (worth $1.00) can be redeemed. Losing tokens are worth $0.00
- **"Could not determine market"**: Make sure the token is from a BTC or ETH 15-minute market
- **Redemption fails**: Check your API credentials and ensure you have Builder Program access

## Notes

- Redemption uses the Relayer Client for gasless transactions
- The script only redeems tokens you actually own (checks balance)
- Redemption converts winning tokens to USDC at 1:1 ratio
- Losing tokens (worth $0.00) cannot be redeemed

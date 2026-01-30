#!/bin/bash
# Test script for selling tokens
# Usage: ./test_sell.sh <TOKEN_ID> [SHARES] [--check-only]

TOKEN_ID=$1
SHARES=${2:-""}
CHECK_ONLY=$3

if [ -z "$TOKEN_ID" ]; then
    echo "Usage: ./test_sell.sh <TOKEN_ID> [SHARES] [--check-only]"
    echo ""
    echo "Example:"
    echo "  ./test_sell.sh 39262267221676949796326419211008961431735960549601091803803006482409998029102"
    echo "  ./test_sell.sh 39262267221676949796326419211008961431735960549601091803803006482409998029102 1.0"
    echo "  ./test_sell.sh 39262267221676949796326419211008961431735960549601091803803006482409998029102 --check-only"
    exit 1
fi

CMD="cargo run --release --bin polymarket-arbitrage-bot -- --config config.json --simulation"
if [ "$CHECK_ONLY" = "--check-only" ]; then
    echo "🔍 Checking portfolio only (not selling)..."
    # We'll need to modify the main binary to support this, or create a simple test
    echo "Note: Use the Rust test binary instead: cargo run --bin test_sell -- --token-id $TOKEN_ID --check-only"
else
    echo "💰 Testing sell for token: $TOKEN_ID"
    if [ -n "$SHARES" ]; then
        echo "   Selling $SHARES shares"
    else
        echo "   Selling all available shares"
    fi
    echo ""
    echo "To test selling, please use the test_sell binary:"
    echo "  cargo run --bin test_sell -- --token-id $TOKEN_ID"
    if [ -n "$SHARES" ]; then
        echo "  cargo run --bin test_sell -- --token-id $TOKEN_ID --shares $SHARES"
    fi
fi

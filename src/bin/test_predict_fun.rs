use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Predict.fun API client for testing
struct PredictFunClient {
    client: Client,
    base_url: String,
    jwt_token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AuthMessage {
    message: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
struct AuthRequest {
    signature: String,
    message: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
struct AuthResponse {
    token: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Market {
    id: String,
    question: Option<String>,
    slug: Option<String>,
    outcomes: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
struct OrderBookEntry {
    price: String,
    size: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
struct OrderBook {
    bids: Vec<OrderBookEntry>,
    asks: Vec<OrderBookEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CreateOrderRequest {
    market_id: String,
    outcome: String, // "Yes" or "No" for binary markets
    side: String,    // "buy" or "sell"
    price: String,
    size: String,
    #[serde(rename = "type")]
    order_type: String, // "limit" or "market"
}

#[derive(Debug, Serialize, Deserialize)]
struct CreateOrderResponse {
    order_id: Option<String>,
    status: Option<String>,
    message: Option<String>,
}

impl PredictFunClient {
    fn new(base_url: String) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url,
            jwt_token: None,
        }
    }

    /// Get authentication message
    async fn get_auth_message(&self) -> Result<String> {
        // Try different possible endpoint paths
        let endpoints = vec![
            format!("{}/authorization/auth-message", self.base_url),
            format!("{}/auth/message", self.base_url),
            format!("{}/api/authorization/auth-message", self.base_url),
        ];

        for url in endpoints {
            let response = self
                .client
                .get(&url)
                .send()
                .await;

            if let Ok(resp) = response {
                let status = resp.status();
                if status.is_success() {
                    let auth_msg: AuthMessage = resp
                        .json()
                        .await
                        .context("Failed to parse auth message")?;
                    return Ok(auth_msg.message);
                }
            }
        }

        anyhow::bail!("Failed to get auth message from any endpoint. Check API documentation for correct endpoint path.")
    }

    /// Authenticate with signature and get JWT token
    /// Note: You'll need to sign the message with your private key
    async fn authenticate(&mut self, signature: String, message: String) -> Result<()> {
        let url = format!("{}/authorization/jwt", self.base_url);

        let auth_request = AuthRequest { signature, message };

        let response = self
            .client
            .post(&url)
            .json(&auth_request)
            .send()
            .await
            .context("Failed to authenticate")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Authentication failed (status: {}): {}", status, error_text);
        }

        let auth_response: AuthResponse = response
            .json()
            .await
            .context("Failed to parse auth response")?;

        self.jwt_token = Some(auth_response.token);
        println!("✅ Authentication successful! JWT token received.");
        Ok(())
    }

    /// Add JWT token to request if available
    fn add_auth_header(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(token) = &self.jwt_token {
            request.header("Authorization", format!("Bearer {}", token))
        } else {
            request
        }
    }

    /// Get all markets
    async fn get_markets(&self) -> Result<Vec<Market>> {
        let url = format!("{}/markets", self.base_url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch markets")?;

        let status = response.status();
        let response_text = response.text().await.unwrap_or_default();
        
        if !status.is_success() {
            eprintln!("   API Response: {}", response_text);
            anyhow::bail!("Failed to get markets (status: {}). Response: {}", status, response_text);
        }

        // Try to parse as array or object with data field
        let markets: Vec<Market> = if let Ok(markets) = serde_json::from_str::<Vec<Market>>(&response_text) {
            markets
        } else if let Ok(data) = serde_json::from_str::<Value>(&response_text) {
            // Try to extract from data field
            if let Some(array) = data.get("data").and_then(|d| d.as_array()) {
                serde_json::from_value(serde_json::Value::Array(array.clone()))
                    .context("Failed to parse markets from data field")?
            } else if let Some(array) = data.as_array() {
                serde_json::from_value(serde_json::Value::Array(array.clone()))
                    .context("Failed to parse markets array")?
            } else {
                anyhow::bail!("Unexpected response format: {}", response_text)
            }
        } else {
            anyhow::bail!("Failed to parse markets response: {}", response_text)
        };

        Ok(markets)
    }

    /// Get market by ID
    async fn get_market_by_id(&self, market_id: &str) -> Result<Value> {
        let url = format!("{}/markets/{}", self.base_url, market_id);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch market")?;

        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("Failed to get market (status: {})", status);
        }

        let market: Value = response
            .json()
            .await
            .context("Failed to parse market")?;

        Ok(market)
    }

    /// Get orderbook for a market
    /// This returns bid/ask prices for Up/Down tokens
    async fn get_orderbook(&self, market_id: &str) -> Result<Value> {
        let url = format!("{}/markets/{}/orderbook", self.base_url, market_id);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch orderbook")?;

        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("Failed to get orderbook (status: {})", status);
        }

        let orderbook: Value = response
            .json()
            .await
            .context("Failed to parse orderbook")?;

        Ok(orderbook)
    }

    /// Create an order (buy or sell)
    async fn create_order(
        &self,
        market_id: &str,
        outcome: &str,
        side: &str,
        price: &str,
        size: &str,
        order_type: &str,
    ) -> Result<CreateOrderResponse> {
        let url = format!("{}/orders", self.base_url);

        let order_request = CreateOrderRequest {
            market_id: market_id.to_string(),
            outcome: outcome.to_string(),
            side: side.to_string(),
            price: price.to_string(),
            size: size.to_string(),
            order_type: order_type.to_string(),
        };

        let response = self
            .add_auth_header(self.client.post(&url))
            .json(&order_request)
            .send()
            .await
            .context("Failed to create order")?;

        let status = response.status();
        let response_text = response.text().await.unwrap_or_default();

        if !status.is_success() {
            anyhow::bail!(
                "Failed to create order (status: {}): {}",
                status,
                response_text
            );
        }

        let order_response: CreateOrderResponse = serde_json::from_str(&response_text)
            .context("Failed to parse order response")?;

        Ok(order_response)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("🧪 Predict.fun API Test Script");
    println!("================================\n");

    // Use testnet for testing (no API key required)
    let base_url = "https://api-testnet.predict.fun";
    let client = PredictFunClient::new(base_url.to_string());

    println!("📡 Testing connection to: {}\n", base_url);

    // Test 1: Get markets
    println!("1️⃣  Testing: Get Markets");
    println!("   Fetching list of markets...");
    match client.get_markets().await {
        Ok(markets) => {
            println!("   ✅ Success! Found {} markets", markets.len());
            if !markets.is_empty() {
                println!("   Sample market:");
                println!("      ID: {}", markets[0].id);
                if let Some(ref question) = markets[0].question {
                    println!("      Question: {}", question);
                }
                if let Some(ref outcomes) = markets[0].outcomes {
                    println!("      Outcomes: {:?}", outcomes);
                }
            }
        }
        Err(e) => {
            println!("   ❌ Failed: {}", e);
        }
    }
    println!();

    // Test 2: Get a specific market (use first market if available)
    println!("2️⃣  Testing: Get Market by ID");
    let test_market_id = "test-market-id"; // Replace with actual market ID
    println!("   Fetching market: {}...", test_market_id);
    match client.get_market_by_id(test_market_id).await {
        Ok(market) => {
            println!("   ✅ Success!");
            println!("   Market data: {}", serde_json::to_string_pretty(&market)?);
        }
        Err(e) => {
            println!("   ⚠️  Note: Using placeholder market ID. Error: {}", e);
            println!("   💡 To test with real market, replace test_market_id with an actual market ID");
        }
    }
    println!();

    // Test 3: Get orderbook (bid/ask prices)
    println!("3️⃣  Testing: Get Orderbook (Bid/Ask Prices)");
    println!("   Fetching orderbook for market: {}...", test_market_id);
    match client.get_orderbook(test_market_id).await {
        Ok(orderbook) => {
            println!("   ✅ Success!");
            println!("   Orderbook data:");
            println!("   {}", serde_json::to_string_pretty(&orderbook)?);
            
            // Try to extract bid/ask information
            if let Some(obj) = orderbook.as_object() {
                println!("\n   📊 Parsed Orderbook:");
                for (key, value) in obj {
                    if key == "bids" || key == "asks" {
                        println!("      {}: {}", key, value);
                    }
                }
            }
        }
        Err(e) => {
            println!("   ⚠️  Note: Using placeholder market ID. Error: {}", e);
            println!("   💡 To test with real market, replace test_market_id with an actual market ID");
        }
    }
    println!();

    // Test 4: Get auth message (for authentication)
    println!("4️⃣  Testing: Get Auth Message");
    println!("   Fetching authentication message...");
    match client.get_auth_message().await {
        Ok(message) => {
            println!("   ✅ Success!");
            println!("   Auth message: {}", message);
            println!("   💡 Sign this message with your private key to authenticate");
        }
        Err(e) => {
            println!("   ❌ Failed: {}", e);
        }
    }
    println!();

    // Test 5: Create order (requires authentication)
    println!("5️⃣  Testing: Create Order (Requires Authentication)");
    println!("   ⚠️  This test requires:");
    println!("      1. Valid JWT token (call authenticate() first)");
    println!("      2. Valid market_id, outcome, price, and size");
    println!("   💡 Skipping actual order creation (would fail without auth)");
    println!("   Example usage:");
    println!("      client.authenticate(signature, message).await?;");
    println!("      client.create_order(");
    println!("          \"market_id\",");
    println!("          \"Yes\",  // or \"No\"");
    println!("          \"buy\",  // or \"sell\"");
    println!("          \"0.50\", // price");
    println!("          \"1.0\",  // size");
    println!("          \"limit\" // or \"market\"");
    println!("      ).await?;");
    println!();

    println!("✅ Test script completed!");
    println!("\n📝 Next Steps:");
    println!("   1. Get a real market ID from the markets list");
    println!("   2. Use that market ID to fetch orderbook and see bid/ask prices");
    println!("   3. Authenticate with your private key signature");
    println!("   4. Test creating orders with authenticated client");
    println!("\n💡 For mainnet, use: https://api.predict.fun");
    println!("💡 For testnet, use: https://api-testnet.predict.fun");

    Ok(())
}

use alloy::primitives::Address;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::error::Error;

const KAIROS_DATA_URL: &str = "https://kairos-timeboost.xyz/api/fetch_latest_auction_data";

#[derive(Serialize)]
struct KairosStatusBody {
    id: u64,
    jsonrpc: String,
}

#[derive(Deserialize)]
pub struct KairosRoundWinner {
    pub address: Address,
}

#[derive(Deserialize)]
pub struct KairosStatusResponse {
    pub next_holder: KairosRoundWinner,
    pub round: u32,
    pub round_end_timestamp: String,
    pub round_start_timestamp: String,
}

pub async fn kairos_express_lane_status() -> Result<KairosStatusResponse, Box<dyn Error>> {
    // Build the JSON-RPC body dynamically using serde_json
    let body = KairosStatusBody {
        id: 1,
        jsonrpc: "2.0".to_string(),
    };

    // Prepare the HTTP headers
    let headers = reqwest::header::HeaderMap::new();
    let client = Client::new();

    // Send the request
    let response = client
        .post(KAIROS_DATA_URL)
        .json(&body) // Automatically serializes the body to JSON
        .headers(headers)
        .send()
        .await?;

    // Check if the response is successful and return the body as a string
    if response.status().is_success() {
        let response_text: KairosStatusResponse = response.json().await?;
        Ok(response_text)
    } else {
        Err(format!("Request failed with status: {}", response.status()).into())
    }
}

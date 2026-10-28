use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::error::Error;

const KAIROS_STATS_URL: &str = "https://rpc.kairos-timeboost.xyz/stats";

#[derive(Serialize)]
struct KairosOrderInfoBody {
    id: u64,
    jsonrpc: String,
    method: String,
    params: Vec<String>,
}

#[derive(Deserialize)]
pub struct KairosOrderInfo {
    success: Option<bool>,
}

#[derive(Deserialize)]
pub struct KairosOrderInfoResponse {
    result: KairosOrderInfo,
}

pub async fn kairos_order_info(uuid: &String) -> Result<bool, Box<dyn Error>> {
    // Build the JSON-RPC body dynamically using serde_json
    let body = KairosOrderInfoBody {
        id: 1,
        jsonrpc: "2.0".to_string(),
        method: "timeboost_getOrderInfo".to_string(),
        params: vec![uuid.clone()],
    };

    // Prepare the HTTP headers
    let headers = reqwest::header::HeaderMap::new();
    let client = Client::new();

    // Send the request
    let response = client
        .post(KAIROS_STATS_URL)
        .json(&body) // Automatically serializes the body to JSON
        .headers(headers)
        .send()
        .await?;

    // Check if the response is successful and return the body as a string
    if response.status().is_success() {
        let response_text: KairosOrderInfoResponse = response.json().await?;
        if response_text.result.success.is_none() {
            return Ok(false);
        }
        Ok(response_text.result.success.unwrap())
    } else {
        Err(format!("Request failed with status: {}", response.status()).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_kairos_order_info_null() {
        let response = kairos_order_info(&"849d6022-a0df-4104-9ecd-3bbe9db9b18f".to_string()).await;
        assert!(!response.unwrap())
    }

    #[tokio::test]
    async fn test_kairos_order_info_true() {
        let response = kairos_order_info(&"c1924dec-5643-4ea1-9c33-0d9412f0d710".to_string()).await;
        assert!(response.unwrap())
    }
}

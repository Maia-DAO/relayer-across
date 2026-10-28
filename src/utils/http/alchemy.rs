use reqwest::Client;
use serde::Serialize;
use std::error::Error;

#[derive(Serialize)]
struct AlchemySimulateParams {
    to: String,
    from: String,
    value: String,
    data: String,
}

#[derive(Serialize)]
struct AlchemySimulateBody {
    id: u64,
    jsonrpc: String,
    method: String,
    params: Vec<AlchemySimulateParams>,
}

pub async fn alchemy_simulate(
    host: &str,
    to: &str,
    from: &str,
    value: &str,
    data: &str,
) -> Result<String, Box<dyn Error>> {
    // Build the JSON-RPC body dynamically using serde_json
    let body = AlchemySimulateBody {
        id: 1,
        jsonrpc: "2.0".to_string(),
        method: "alchemy_simulateExecution".to_string(),
        params: vec![AlchemySimulateParams {
            to: to.to_string(),
            from: from.to_string(),
            value: value.to_string(),
            data: data.to_string(),
        }],
    };

    // Prepare the HTTP headers
    let headers = reqwest::header::HeaderMap::new();
    let client = Client::new();

    // Send the request
    let response = client
        .post(host)
        .json(&body) // Automatically serializes the body to JSON
        .headers(headers)
        .send()
        .await?;

    // Check if the response is successful and return the body as a string
    if response.status().is_success() {
        let response_text = response.text().await?;
        Ok(response_text)
    } else {
        Err(format!("Request failed with status: {}", response.status()).into())
    }
}
